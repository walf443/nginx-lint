//! WASM plugin loader
//!
//! Handles discovering, loading, and validating WASM component model plugins from a directory.

use super::component_rule::ComponentLintRule;
use super::error::PluginError;
use crate::linter::LintRule;
use std::fs;
use std::path::Path;
use wasmtime::{Config, Engine};

/// Memory limit for plugins (256 MB)
const MEMORY_LIMIT_BYTES: u64 = 256 * 1024 * 1024;

/// Fuel limit for CPU metering (prevents infinite loops)
const FUEL_LIMIT: u64 = 10_000_000_000;

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
}

impl PluginLoader {
    /// Create a new plugin loader with security constraints (fuel metering enabled)
    pub fn new() -> Result<Self, PluginError> {
        Self::with_options(true)
    }

    /// Create a new plugin loader for trusted plugins (fuel metering disabled for performance)
    ///
    /// WARNING: Only use this for trusted, builtin plugins. External plugins should use `new()`
    /// to enable fuel metering and prevent infinite loops.
    pub fn new_trusted() -> Result<Self, PluginError> {
        Self::with_options(false)
    }

    fn with_options(enable_fuel: bool) -> Result<Self, PluginError> {
        let mut config = Config::new();

        // Enable fuel-based metering only for untrusted plugins
        config.consume_fuel(enable_fuel);
        // Enable component model support for WIT-based plugins
        config.wasm_component_model(true);
        // Enable Wasm GC support (needed for GC-based languages like wado)
        config.wasm_gc(true);

        let engine = Engine::new(&config)
            .map_err(|e| PluginError::compile_error("engine", e.to_string()))?;

        Ok(Self {
            engine,
            fuel_enabled: enable_fuel,
        })
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

    #[test]
    fn test_loader_creation() {
        let loader = PluginLoader::new();
        assert!(loader.is_ok());
    }

    #[test]
    fn test_load_plugins_empty_dir() {
        let loader = PluginLoader::new().unwrap();
        let dir = tempdir().unwrap();
        let plugins = loader.load_plugins(dir.path());
        assert!(plugins.is_ok());
        assert!(plugins.unwrap().is_empty());
    }

    #[test]
    fn test_load_plugins_nonexistent_dir() {
        let loader = PluginLoader::new().unwrap();
        let result = loader.load_plugins(Path::new("/nonexistent/path"));
        assert!(matches!(result, Err(PluginError::DirectoryNotFound { .. })));
    }

    #[test]
    fn test_invalid_wasm_file() {
        let loader = PluginLoader::new().unwrap();
        let dir = tempdir().unwrap();
        let wasm_path = dir.path().join("invalid.wasm");
        fs::write(&wasm_path, b"not a wasm file").unwrap();

        let result = loader.load_plugin(&wasm_path);
        assert!(matches!(result, Err(PluginError::InvalidWasmFile { .. })));
    }

    #[test]
    fn test_core_module_rejected() {
        let loader = PluginLoader::new().unwrap();
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
        let loader = PluginLoader::new().unwrap();
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("readme.txt"), b"hello").unwrap();
        let plugins = loader.load_plugins(dir.path()).unwrap();
        assert!(plugins.is_empty());
    }

    #[test]
    fn test_component_model_enabled() {
        let loader = PluginLoader::new().unwrap();
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
