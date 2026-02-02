//! WASM plugin loader
//!
//! Handles discovering, loading, and validating WASM plugins from a directory.

use super::error::PluginError;
use super::wasm_rule::WasmLintRule;
use std::fs;
use std::path::{Path, PathBuf};
use wasmtime::{Config, Engine};

/// Memory limit for plugins (256 MB)
const MEMORY_LIMIT_BYTES: u64 = 256 * 1024 * 1024;

/// Fuel limit for CPU metering (prevents infinite loops)
const FUEL_LIMIT: u64 = 10_000_000_000;

/// Plugin loader that discovers and loads WASM plugins from a directory
pub struct PluginLoader {
    engine: Engine,
}

impl PluginLoader {
    /// Create a new plugin loader with security constraints
    pub fn new() -> Result<Self, PluginError> {
        let mut config = Config::new();

        // Enable fuel-based metering for CPU limits
        config.consume_fuel(true);

        // Limit memory
        config.max_wasm_stack(1024 * 1024); // 1 MB stack limit

        let engine = Engine::new(&config).map_err(|e| {
            PluginError::compile_error(PathBuf::new(), format!("Failed to create WASM engine: {}", e))
        })?;

        Ok(Self { engine })
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
        FUEL_LIMIT
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
        WasmLintRule::new(&self.engine, path.to_path_buf(), &wasm_bytes, self.memory_limit(), self.fuel_limit())
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
