//! WASM plugin loader
//!
//! Handles discovering, loading, and validating WASM plugins from a directory.

use super::error::PluginError;
use super::wasm_rule::WasmLintRule;
use std::fs;
use std::path::Path;
use wasmi::{Config, Engine};

/// Memory limit for plugins (256 MB)
const MEMORY_LIMIT_BYTES: u64 = 256 * 1024 * 1024;

/// Fuel limit for CPU metering (prevents infinite loops)
const FUEL_LIMIT: u64 = 10_000_000_000;

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
        let mut config = Config::default();

        // Enable fuel-based metering only for untrusted plugins
        config.consume_fuel(enable_fuel);

        let engine = Engine::new(&config);

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
    pub fn load_plugins(&self, dir: &Path) -> Result<Vec<WasmLintRule>, PluginError> {
        if !dir.exists() {
            return Err(PluginError::directory_not_found(dir));
        }

        if !dir.is_dir() {
            return Err(PluginError::directory_not_found(dir));
        }

        let mut plugins = Vec::new();

        let entries = fs::read_dir(dir).map_err(|e| PluginError::io_error(dir, e))?;

        for entry in entries {
            let entry = entry.map_err(|e| PluginError::io_error(dir, e))?;
            let path = entry.path();

            // Only load .wasm files
            if path.extension().is_some_and(|ext| ext == "wasm") {
                match self.load_plugin(&path) {
                    Ok(plugin) => plugins.push(plugin),
                    Err(e) => {
                        // Log error but continue loading other plugins
                        eprintln!("Warning: Failed to load plugin {:?}: {}", path, e);
                    }
                }
            }
        }

        Ok(plugins)
    }

    /// Load a single WASM plugin from a file
    pub fn load_plugin(&self, path: &Path) -> Result<WasmLintRule, PluginError> {
        // Read the WASM bytes
        let wasm_bytes = fs::read(path).map_err(|e| PluginError::io_error(path, e))?;

        // Validate it's a WASM file (magic number: \0asm)
        if wasm_bytes.len() < 4 || &wasm_bytes[0..4] != b"\0asm" {
            return Err(PluginError::invalid_wasm_file(path));
        }

        // Create the WASM lint rule
        let fuel_limit = if self.fuel_enabled {
            self.fuel_limit()
        } else {
            0 // No fuel limit for trusted plugins
        };
        WasmLintRule::new(
            &self.engine,
            path.to_path_buf(),
            &wasm_bytes,
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
}
