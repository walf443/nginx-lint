//! WASM plugin loader
//!
//! Handles discovering, loading, and validating WASM component model plugins from a directory.

use super::component_rule::ComponentLintRule;
use super::error::PluginError;
use crate::linter::LintRule;
use std::fs;
use std::path::{Path, PathBuf};
use wasmtime::{Cache, CacheConfig, Config, Engine};

/// Memory limit for plugins (256 MB)
const MEMORY_LIMIT_BYTES: u64 = 256 * 1024 * 1024;

/// Fuel limit for CPU metering (prevents infinite loops)
const FUEL_LIMIT: u64 = 10_000_000_000;

/// Compilation cache configuration for the plugin loader.
///
/// Compiling a WASM plugin to native code dominates plugin load time. With a
/// disk cache enabled, compiled artifacts are keyed by the plugin bytes and
/// compiler configuration, so subsequent runs skip compilation entirely and
/// only deserialize the cached native code.
///
/// The cache lives in the [`crate::cache::PLUGIN_CACHE_SUBDIR`] subdirectory
/// of the nginx-lint cache root, so the same root can be shared with other
/// (future) cache consumers.
#[derive(Debug, Clone, Default)]
pub enum CompilationCache {
    /// Use the default per-user cache root
    /// (e.g. `~/.cache/nginx-lint` on Linux); see
    /// [`crate::cache::default_cache_root`].
    #[default]
    Default,
    /// Compile plugins on every run without caching.
    Disabled,
    /// Use the given directory as the cache root, creating it if missing.
    /// A relative path is resolved against the current working directory.
    Directory(PathBuf),
}

/// Detect whether a WASM binary is a component model file
fn is_component_model(bytes: &[u8]) -> Option<bool> {
    if bytes.len() < 8 {
        return None;
    }
    // Check magic number: \0asm
    if &bytes[0..4] != b"\0asm" {
        return None;
    }
    // Check version field (bytes 4-7)
    match &bytes[4..8] {
        [0x0d, 0x00, 0x01, 0x00] => Some(true),
        [0x01, 0x00, 0x00, 0x00] => Some(false), // Core module (no longer supported)
        _ => None,
    }
}

/// Plugin loader that discovers and loads WASM plugins from a directory
pub struct PluginLoader {
    engine: Engine,
    /// Whether fuel metering is enabled (for untrusted plugins)
    fuel_enabled: bool,
    /// Compilation cache handle, kept for reporting hit/miss statistics
    cache: Option<Cache>,
}

impl PluginLoader {
    /// Create a new plugin loader with security constraints (fuel metering enabled)
    pub fn new() -> Result<Self, PluginError> {
        Self::with_options(true, CompilationCache::Default)
    }

    /// Create a new plugin loader for trusted plugins (fuel metering disabled for performance)
    ///
    /// WARNING: Only use this for trusted, builtin plugins. External plugins should use `new()`
    /// to enable fuel metering and prevent infinite loops.
    pub fn new_trusted() -> Result<Self, PluginError> {
        Self::with_options(false, CompilationCache::Default)
    }

    /// Create a trusted plugin loader (see [`new_trusted`](Self::new_trusted))
    /// with an explicit compilation cache configuration
    pub fn new_trusted_with_cache(cache: CompilationCache) -> Result<Self, PluginError> {
        Self::with_options(false, cache)
    }

    /// Create a new plugin loader with security constraints and an explicit
    /// compilation cache configuration
    pub fn new_with_cache(cache: CompilationCache) -> Result<Self, PluginError> {
        Self::with_options(true, cache)
    }

    fn with_options(enable_fuel: bool, cache: CompilationCache) -> Result<Self, PluginError> {
        let cache = Self::build_cache(cache)?;
        let mut config = Config::new();

        // Enable fuel-based metering only for untrusted plugins
        config.consume_fuel(enable_fuel);
        // Enable component model support for WIT-based plugins
        config.wasm_component_model(true);
        // Enable Wasm GC support (needed for GC-based languages like wado)
        config.wasm_gc(true);
        // The cache key includes the compiler configuration, so trusted and
        // untrusted loaders never share cache entries.
        config.cache(cache.clone());

        let engine = Engine::new(&config)
            .map_err(|e| PluginError::compile_error("engine", e.to_string()))?;

        Ok(Self {
            engine,
            fuel_enabled: enable_fuel,
            cache,
        })
    }

    fn build_cache(cache: CompilationCache) -> Result<Option<Cache>, PluginError> {
        match cache {
            CompilationCache::Disabled => Ok(None),
            // The default cache root can be unavailable (e.g. no home
            // directory); linting should still work, just without the cache.
            CompilationCache::Default => {
                let Some(root) = crate::cache::default_cache_root() else {
                    eprintln!(
                        "Warning: plugin compilation cache disabled (could not determine the user cache directory)"
                    );
                    return Ok(None);
                };
                match Self::cache_in_root(&root) {
                    Ok(cache) => Ok(Some(cache)),
                    Err(e) => {
                        eprintln!(
                            "Warning: plugin compilation cache disabled (failed to initialize): {}",
                            e
                        );
                        Ok(None)
                    }
                }
            }
            // An explicitly requested directory that cannot be used is an error.
            CompilationCache::Directory(root) => Self::cache_in_root(&root).map(Some),
        }
    }

    /// Create a compilation cache in the plugin subdirectory of `root`
    fn cache_in_root(root: &Path) -> Result<Cache, PluginError> {
        let dir = crate::cache::plugin_cache_dir(root);
        // wasmtime requires an absolute cache directory path
        let abs_dir =
            std::path::absolute(&dir).map_err(|e| PluginError::cache_error(&dir, e.to_string()))?;
        let mut config = CacheConfig::new();
        config.with_directory(abs_dir);
        Cache::new(config).map_err(|e| PluginError::cache_error(&dir, e.to_string()))
    }

    /// Get the compilation cache directory, if caching is enabled
    pub fn cache_directory(&self) -> Option<&PathBuf> {
        self.cache.as_ref().map(|c| c.directory())
    }

    /// Get compilation cache statistics as `(hits, misses)`, if caching is enabled
    pub fn cache_stats(&self) -> Option<(usize, usize)> {
        self.cache
            .as_ref()
            .map(|c| (c.cache_hits(), c.cache_misses()))
    }

    /// Get the WASM engine
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Get the memory limit in bytes
    pub fn memory_limit(&self) -> u64 {
        MEMORY_LIMIT_BYTES
    }

    /// Get the fuel limit for CPU metering
    pub fn fuel_limit(&self) -> u64 {
        if self.fuel_enabled { FUEL_LIMIT } else { 0 }
    }

    /// Check if fuel metering is enabled
    pub fn fuel_enabled(&self) -> bool {
        self.fuel_enabled
    }

    /// Load all WASM plugins from a directory
    pub fn load_plugins(&self, dir: &Path) -> Result<Vec<Box<dyn LintRule>>, PluginError> {
        if !dir.exists() || !dir.is_dir() {
            return Err(PluginError::directory_not_found(dir));
        }

        let mut plugins: Vec<Box<dyn LintRule>> = Vec::new();
        let entries = fs::read_dir(dir).map_err(|e| PluginError::io_error(dir, e))?;

        for entry in entries {
            let entry = entry.map_err(|e| PluginError::io_error(dir, e))?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "wasm") {
                match self.load_plugin(&path) {
                    Ok(plugin) => plugins.push(plugin),
                    Err(e) => {
                        eprintln!("Warning: Failed to load plugin {:?}: {}", path, e);
                    }
                }
            }
        }

        Ok(plugins)
    }

    /// Load a single WASM plugin from a file
    pub fn load_plugin(&self, path: &Path) -> Result<Box<dyn LintRule>, PluginError> {
        let wasm_bytes = fs::read(path).map_err(|e| PluginError::io_error(path, e))?;

        match is_component_model(&wasm_bytes) {
            Some(true) => {
                let rule = self.load_component_from_bytes(path, &wasm_bytes)?;
                Ok(Box::new(rule))
            }
            Some(false) => Err(PluginError::unsupported_format(
                path,
                "Legacy core WASM modules are no longer supported. Please rebuild your plugin with export_component_plugin! and wasm-tools component new.",
            )),
            None => Err(PluginError::invalid_wasm_file(path)),
        }
    }

    /// Load a component from bytes
    pub fn load_component_from_bytes(
        &self,
        path: &Path,
        component_bytes: &[u8],
    ) -> Result<ComponentLintRule, PluginError> {
        let fuel_limit = if self.fuel_enabled {
            self.fuel_limit()
        } else {
            0
        };
        ComponentLintRule::new(
            &self.engine,
            path.to_path_buf(),
            component_bytes,
            self.memory_limit(),
            fuel_limit,
            self.fuel_enabled,
        )
    }
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self::new().expect("Failed to create default PluginLoader")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Loader for tests: cache disabled so `cargo test` never creates or
    /// writes the real per-user cache directory.
    fn test_loader() -> PluginLoader {
        PluginLoader::new_with_cache(CompilationCache::Disabled).unwrap()
    }

    #[test]
    fn test_loader_creation() {
        let loader = PluginLoader::new_with_cache(CompilationCache::Disabled);
        assert!(loader.is_ok());
    }

    #[test]
    fn test_load_plugins_empty_dir() {
        let loader = test_loader();
        let dir = tempdir().unwrap();
        let plugins = loader.load_plugins(dir.path());
        assert!(plugins.is_ok());
        assert!(plugins.unwrap().is_empty());
    }

    #[test]
    fn test_load_plugins_nonexistent_dir() {
        let loader = test_loader();
        let result = loader.load_plugins(Path::new("/nonexistent/path"));
        assert!(matches!(result, Err(PluginError::DirectoryNotFound { .. })));
    }

    #[test]
    fn test_invalid_wasm_file() {
        let loader = test_loader();
        let dir = tempdir().unwrap();
        let wasm_path = dir.path().join("invalid.wasm");
        fs::write(&wasm_path, b"not a wasm file").unwrap();

        let result = loader.load_plugin(&wasm_path);
        assert!(matches!(result, Err(PluginError::InvalidWasmFile { .. })));
    }

    #[test]
    fn test_core_module_rejected() {
        let loader = test_loader();
        let dir = tempdir().unwrap();
        let wasm_path = dir.path().join("legacy.wasm");
        // Core module: magic + version 01 00 00 00
        fs::write(&wasm_path, b"\0asm\x01\x00\x00\x00").unwrap();

        let result = loader.load_plugin(&wasm_path);
        assert!(matches!(result, Err(PluginError::UnsupportedFormat { .. })));
    }

    #[test]
    fn test_detect_component() {
        // Component: magic + version 0d 00 01 00
        let bytes = b"\0asm\x0d\x00\x01\x00";
        assert_eq!(is_component_model(bytes), Some(true));
    }

    #[test]
    fn test_detect_core_module() {
        let bytes = b"\0asm\x01\x00\x00\x00";
        assert_eq!(is_component_model(bytes), Some(false));
    }

    #[test]
    fn test_detect_invalid() {
        let bytes = b"not wasm";
        assert!(is_component_model(bytes).is_none());
    }

    #[test]
    fn test_detect_too_short() {
        let bytes = b"\0asm";
        assert!(is_component_model(bytes).is_none());
    }

    #[test]
    fn test_detect_unknown_version() {
        let bytes = b"\0asm\x02\x00\x00\x00";
        assert!(is_component_model(bytes).is_none());
    }

    #[test]
    fn test_load_plugins_skips_non_wasm() {
        let loader = test_loader();
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("readme.txt"), b"hello").unwrap();
        let plugins = loader.load_plugins(dir.path()).unwrap();
        assert!(plugins.is_empty());
    }

    #[test]
    fn test_new_with_cache_custom_directory() {
        let dir = tempdir().unwrap();
        let cache_root = dir.path().join("nginx-lint-cache");
        let loader =
            PluginLoader::new_with_cache(CompilationCache::Directory(cache_root.clone())).unwrap();
        // The plugin cache lives in the "plugins" subdirectory of the root,
        // which is created and reported back (in canonicalized form)
        let plugin_cache = cache_root.join("plugins");
        assert!(plugin_cache.is_dir());
        let reported = loader.cache_directory().expect("cache should be enabled");
        assert_eq!(reported, &fs::canonicalize(&plugin_cache).unwrap());
        assert_eq!(loader.cache_stats(), Some((0, 0)));
    }

    #[test]
    fn test_new_with_cache_disabled() {
        let loader = PluginLoader::new_with_cache(CompilationCache::Disabled).unwrap();
        assert!(loader.cache_directory().is_none());
        assert!(loader.cache_stats().is_none());
    }

    #[test]
    fn test_cache_round_trip() {
        let dir = tempdir().unwrap();
        let make_loader = || {
            PluginLoader::new_with_cache(CompilationCache::Directory(dir.path().to_path_buf()))
                .unwrap()
        };

        // First compilation populates the cache
        let loader = make_loader();
        wasmtime::component::Component::new(loader.engine(), "(component)").unwrap();
        assert_eq!(loader.cache_stats(), Some((0, 1)));

        // A fresh loader with the same cache directory and configuration
        // hits the cache instead of recompiling
        let loader = make_loader();
        wasmtime::component::Component::new(loader.engine(), "(component)").unwrap();
        assert_eq!(loader.cache_stats(), Some((1, 0)));
    }

    #[test]
    fn test_component_model_enabled() {
        let loader = test_loader();
        let bytes = b"\0asm\x0d\x00\x01\x00";
        let result = wasmtime::component::Component::new(loader.engine(), bytes);
        if let Err(e) = result {
            let msg = e.to_string();
            assert!(
                !msg.contains("component model"),
                "Component model should be enabled, but got: {}",
                msg
            );
        }
    }
}
