//! WASM-based lint rule implementation
//!
//! This module implements the LintRule trait for WASM plugins.

use super::error::PluginError;
use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wasmi::{Engine, Linker, Memory, Module, Store, TypedFunc};

/// Plugin info returned by the plugin_info export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub category: String,
    pub description: String,
    /// API version the plugin uses (defaults to "1.0" for backward compatibility)
    #[serde(default = "default_api_version")]
    pub api_version: String,
    /// Severity level (error, warning, info)
    #[serde(default)]
    pub severity: Option<String>,
    /// Why this rule exists (detailed explanation)
    #[serde(default)]
    pub why: Option<String>,
    /// Example of bad configuration
    #[serde(default)]
    pub bad_example: Option<String>,
    /// Example of good configuration
    #[serde(default)]
    pub good_example: Option<String>,
    /// References (URLs, documentation links)
    #[serde(default)]
    pub references: Option<Vec<String>>,
}

fn default_api_version() -> String {
    "1.0".to_string()
}

/// Plugin fix format (matches the SDK Fix struct)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginFix {
    pub line: usize,
    #[serde(default)]
    pub old_text: Option<String>,
    pub new_text: String,
    #[serde(default)]
    pub delete_line: bool,
    #[serde(default)]
    pub insert_after: bool,
}

impl PluginFix {
    fn into_fix(self) -> crate::linter::Fix {
        crate::linter::Fix {
            line: self.line,
            old_text: self.old_text,
            new_text: self.new_text,
            delete_line: self.delete_line,
            insert_after: self.insert_after,
        }
    }
}

/// Plugin lint error format (simplified for JSON transfer)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginLintError {
    pub rule: String,
    pub category: String,
    pub message: String,
    pub severity: String,
    #[serde(default)]
    pub line: Option<usize>,
    #[serde(default)]
    pub column: Option<usize>,
    #[serde(default)]
    pub fix: Option<PluginFix>,
}

impl PluginLintError {
    fn into_lint_error(self) -> LintError {
        let severity = match self.severity.to_lowercase().as_str() {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            _ => Severity::Info,
        };

        let mut error = LintError::new(&self.rule, &self.category, &self.message, severity);

        if let (Some(line), Some(column)) = (self.line, self.column) {
            error = error.with_location(line, column);
        } else if let Some(line) = self.line {
            error = error.with_location(line, 1);
        }

        if let Some(fix) = self.fix {
            error = error.with_fix(fix.into_fix());
        }

        error
    }
}

/// Parse plugin output based on API version
/// Currently supports version 1.0 only
fn parse_plugin_output(json: &str, api_version: &str, path: &Path) -> Result<Vec<LintError>, PluginError> {
    match api_version {
        "1.0" => {
            let plugin_errors: Vec<PluginLintError> = serde_json::from_str(json).map_err(|e| {
                PluginError::result_parse_error(path, format!("Invalid result JSON: {}", e))
            })?;
            Ok(plugin_errors.into_iter().map(|e| e.into_lint_error()).collect())
        }
        _ => {
            // Unknown version - try to parse as v1.0 with a warning
            let plugin_errors: Vec<PluginLintError> = serde_json::from_str(json).map_err(|e| {
                PluginError::result_parse_error(
                    path,
                    format!("Unknown API version '{}', failed to parse as v1.0: {}", api_version, e),
                )
            })?;
            Ok(plugin_errors.into_iter().map(|e| e.into_lint_error()).collect())
        }
    }
}

/// Store data for WASM execution
#[derive(Default)]
struct StoreData {}

/// Cached instance data for reuse
struct CachedInstance {
    store: Store<StoreData>,
    memory: Memory,
    alloc: TypedFunc<u32, u32>,
    dealloc: TypedFunc<(u32, u32), ()>,
    check: TypedFunc<(u32, u32, u32, u32), u32>,
    check_result_len: TypedFunc<(), u32>,
}

// Thread-local cache for WASM instances
// Key: plugin path as string
thread_local! {
    static INSTANCE_CACHE: RefCell<std::collections::HashMap<String, CachedInstance>> =
        RefCell::new(std::collections::HashMap::new());
}

// Thread-local cache for serialized configs
// Caches the last serialized config to avoid re-serialization when multiple plugins
// check the same config. Uses the Config pointer as a key.
thread_local! {
    static SERIALIZED_CONFIG_CACHE: RefCell<Option<(usize, String)>> = const { RefCell::new(None) };
}

/// Get or create serialized JSON for a Config
/// Uses the Config's pointer address as a cache key
fn get_serialized_config(config: &Config) -> Result<String, serde_json::Error> {
    let config_ptr = config as *const Config as usize;

    SERIALIZED_CONFIG_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();

        // Check if we have a cached serialization for this exact Config
        if let Some((cached_ptr, ref json)) = *cache {
            if cached_ptr == config_ptr {
                return Ok(json.clone());
            }
        }

        // Serialize and cache
        let json = serde_json::to_string(config)?;
        *cache = Some((config_ptr, json.clone()));
        Ok(json)
    })
}

/// A lint rule implemented as a WASM module
#[derive(Clone)]
pub struct WasmLintRule {
    /// Path to the WASM file (for error reporting and cache key)
    path: PathBuf,
    /// Plugin metadata
    info: PluginInfo,
    /// Compiled WASM module (shared across threads)
    module: Arc<Module>,
    /// WASM engine reference (shared across threads)
    engine: Engine,
    /// Fuel limit for CPU metering
    fuel_limit: u64,
    /// Leaked static strings for LintRule trait
    name: &'static str,
    category: &'static str,
    description: &'static str,
    /// API version the plugin uses (for output parsing)
    api_version: String,
}

impl WasmLintRule {
    /// Create a new WASM lint rule from compiled bytes
    pub fn new(
        engine: &Engine,
        path: PathBuf,
        wasm_bytes: &[u8],
        _memory_limit: u64,
        fuel_limit: u64,
    ) -> Result<Self, PluginError> {
        // Compile the module
        let module = Module::new(engine, wasm_bytes)
            .map_err(|e| PluginError::compile_error(&path, e.to_string()))?;

        // Validate required exports exist
        Self::validate_exports(&module, &path)?;

        // Get plugin info by instantiating temporarily
        let info = Self::get_plugin_info(engine, &module, &path, fuel_limit)?;

        // Leak strings for 'static lifetime (these live for the program duration)
        let name: &'static str = Box::leak(info.name.clone().into_boxed_str());
        let category: &'static str = Box::leak(info.category.clone().into_boxed_str());
        let description: &'static str = Box::leak(info.description.clone().into_boxed_str());
        let api_version = info.api_version.clone();

        Ok(Self {
            path,
            info,
            module: Arc::new(module),
            engine: engine.clone(),
            fuel_limit,
            name,
            category,
            description,
            api_version,
        })
    }

    /// Validate that the WASM module has all required exports
    fn validate_exports(module: &Module, path: &Path) -> Result<(), PluginError> {
        let required_exports = [
            "plugin_info",
            "plugin_info_len",
            "check",
            "check_result_len",
            "alloc",
            "dealloc",
        ];

        let exports: Vec<_> = module.exports().map(|e| e.name().to_string()).collect();

        for export in &required_exports {
            if !exports.iter().any(|e| e == *export) {
                return Err(PluginError::missing_export(path, *export));
            }
        }

        Ok(())
    }

    /// Get plugin info by calling the plugin_info export
    fn get_plugin_info(
        engine: &Engine,
        module: &Module,
        path: &Path,
        fuel_limit: u64,
    ) -> Result<PluginInfo, PluginError> {
        let mut store = Store::new(engine, StoreData::default());
        store.set_fuel(fuel_limit).map_err(|e| {
            PluginError::execution_error(path, format!("Failed to set fuel: {}", e))
        })?;

        let linker = Linker::<StoreData>::new(engine);
        let instance = linker
            .instantiate_and_start(&mut store, module)
            .map_err(|e| PluginError::instantiate_error(path, e.to_string()))?;

        // Get memory
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| PluginError::missing_export(path, "memory"))?;

        // Get functions
        let plugin_info_len = instance
            .get_typed_func::<(), u32>(&store, "plugin_info_len")
            .map_err(|e| PluginError::missing_export(path, format!("plugin_info_len: {}", e)))?;

        let plugin_info = instance
            .get_typed_func::<(), u32>(&store, "plugin_info")
            .map_err(|e| PluginError::missing_export(path, format!("plugin_info: {}", e)))?;

        // Call plugin_info_len to get the length
        let len = plugin_info_len.call(&mut store, ()).map_err(|e| {
            PluginError::execution_error(path, format!("plugin_info_len failed: {}", e))
        })? as usize;

        // Call plugin_info to get the pointer
        let ptr = plugin_info.call(&mut store, ()).map_err(|e| {
            PluginError::execution_error(path, format!("plugin_info failed: {}", e))
        })? as usize;

        // Read the JSON string from memory
        let json_str = Self::read_string_from_memory(&store, &memory, ptr, len, path)?;

        // Parse the JSON
        let info: PluginInfo = serde_json::from_str(&json_str).map_err(|e| {
            PluginError::invalid_plugin_info(path, format!("Invalid JSON: {}", e))
        })?;

        Ok(info)
    }

    /// Read a string from WASM memory
    fn read_string_from_memory(
        store: &Store<StoreData>,
        memory: &Memory,
        ptr: usize,
        len: usize,
        path: &Path,
    ) -> Result<String, PluginError> {
        let data = memory.data(store);

        if ptr + len > data.len() {
            return Err(PluginError::execution_error(
                path,
                format!(
                    "Memory access out of bounds: ptr={}, len={}, memory_size={}",
                    ptr,
                    len,
                    data.len()
                ),
            ));
        }

        let bytes = &data[ptr..ptr + len];
        String::from_utf8(bytes.to_vec())
            .map_err(|e| PluginError::execution_error(path, format!("Invalid UTF-8: {}", e)))
    }

    /// Create a new cached instance
    fn create_instance(&self) -> Result<CachedInstance, PluginError> {
        let mut store = Store::new(&self.engine, StoreData::default());
        store.set_fuel(self.fuel_limit).map_err(|e| {
            PluginError::execution_error(&self.path, format!("Failed to set fuel: {}", e))
        })?;

        let linker = Linker::<StoreData>::new(&self.engine);
        let instance = linker
            .instantiate_and_start(&mut store, &self.module)
            .map_err(|e| PluginError::instantiate_error(&self.path, e.to_string()))?;

        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| PluginError::missing_export(&self.path, "memory"))?;

        let alloc = instance
            .get_typed_func::<u32, u32>(&store, "alloc")
            .map_err(|e| PluginError::missing_export(&self.path, format!("alloc: {}", e)))?;

        let dealloc = instance
            .get_typed_func::<(u32, u32), ()>(&store, "dealloc")
            .map_err(|e| PluginError::missing_export(&self.path, format!("dealloc: {}", e)))?;

        let check = instance
            .get_typed_func::<(u32, u32, u32, u32), u32>(&store, "check")
            .map_err(|e| PluginError::missing_export(&self.path, format!("check: {}", e)))?;

        let check_result_len = instance
            .get_typed_func::<(), u32>(&store, "check_result_len")
            .map_err(|e| PluginError::missing_export(&self.path, format!("check_result_len: {}", e)))?;

        Ok(CachedInstance {
            store,
            memory,
            alloc,
            dealloc,
            check,
            check_result_len,
        })
    }

    /// Execute check using cached instance
    fn execute_check_cached(
        &self,
        cached: &mut CachedInstance,
        config: &Config,
        file_path: &Path,
    ) -> Result<Vec<LintError>, PluginError> {
        // Reset fuel for this check
        cached.store.set_fuel(self.fuel_limit).map_err(|e| {
            PluginError::execution_error(&self.path, format!("Failed to reset fuel: {}", e))
        })?;

        // Get serialized config (cached if same Config was serialized recently)
        let config_json = get_serialized_config(config).map_err(|e| {
            PluginError::execution_error(&self.path, format!("Failed to serialize config: {}", e))
        })?;

        // Serialize path to string
        let path_str = file_path.to_string_lossy().to_string();

        // Allocate and write config
        let config_ptr = cached.alloc.call(&mut cached.store, config_json.len() as u32).map_err(|e| {
            PluginError::execution_error(&self.path, format!("alloc failed: {}", e))
        })?;
        {
            let mem_data = cached.memory.data_mut(&mut cached.store);
            let ptr = config_ptr as usize;
            if ptr + config_json.len() > mem_data.len() {
                return Err(PluginError::execution_error(&self.path, "Memory out of bounds"));
            }
            mem_data[ptr..ptr + config_json.len()].copy_from_slice(config_json.as_bytes());
        }

        // Allocate and write path
        let path_ptr = cached.alloc.call(&mut cached.store, path_str.len() as u32).map_err(|e| {
            PluginError::execution_error(&self.path, format!("alloc failed: {}", e))
        })?;
        {
            let mem_data = cached.memory.data_mut(&mut cached.store);
            let ptr = path_ptr as usize;
            if ptr + path_str.len() > mem_data.len() {
                return Err(PluginError::execution_error(&self.path, "Memory out of bounds"));
            }
            mem_data[ptr..ptr + path_str.len()].copy_from_slice(path_str.as_bytes());
        }

        // Call check
        let result_ptr = cached.check
            .call(
                &mut cached.store,
                (
                    config_ptr,
                    config_json.len() as u32,
                    path_ptr,
                    path_str.len() as u32,
                ),
            )
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("fuel") || err_str.contains("Fuel") {
                    PluginError::timeout(&self.path)
                } else {
                    PluginError::execution_error(&self.path, format!("check failed: {}", e))
                }
            })?;

        // Get result length
        let result_len = cached.check_result_len.call(&mut cached.store, ()).map_err(|e| {
            PluginError::execution_error(&self.path, format!("check_result_len failed: {}", e))
        })? as usize;

        // Read result from memory
        let result_json = {
            let data = cached.memory.data(&cached.store);
            let ptr = result_ptr as usize;
            if ptr + result_len > data.len() {
                return Err(PluginError::execution_error(
                    &self.path,
                    format!("Memory out of bounds reading result: ptr={}, len={}", ptr, result_len),
                ));
            }
            String::from_utf8(data[ptr..ptr + result_len].to_vec())
                .map_err(|e| PluginError::execution_error(&self.path, format!("Invalid UTF-8: {}", e)))?
        };

        // Deallocate memory
        let _ = cached.dealloc.call(&mut cached.store, (config_ptr, config_json.len() as u32));
        let _ = cached.dealloc.call(&mut cached.store, (path_ptr, path_str.len() as u32));
        let _ = cached.dealloc.call(&mut cached.store, (result_ptr, result_len as u32));

        // Parse result based on plugin's API version
        parse_plugin_output(&result_json, &self.api_version, &self.path)
    }

    /// Execute the check function with instance caching
    fn execute_check(&self, config: &Config, file_path: &Path) -> Result<Vec<LintError>, PluginError> {
        let cache_key = self.path.to_string_lossy().to_string();

        INSTANCE_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();

            // Get or create cached instance
            if !cache.contains_key(&cache_key) {
                let instance = self.create_instance()?;
                cache.insert(cache_key.clone(), instance);
            }

            let cached = cache.get_mut(&cache_key).unwrap();
            self.execute_check_cached(cached, config, file_path)
        })
    }
}

impl WasmLintRule {
    /// Get the API version this plugin uses
    pub fn api_version(&self) -> &str {
        &self.api_version
    }

    /// Get the severity level from plugin info
    pub fn severity(&self) -> Option<&str> {
        self.info.severity.as_deref()
    }

    /// Get the why documentation from plugin info
    pub fn why(&self) -> Option<&str> {
        self.info.why.as_deref()
    }

    /// Get the bad example from plugin info
    pub fn bad_example(&self) -> Option<&str> {
        self.info.bad_example.as_deref()
    }

    /// Get the good example from plugin info
    pub fn good_example(&self) -> Option<&str> {
        self.info.good_example.as_deref()
    }

    /// Get the references from plugin info
    pub fn references(&self) -> Option<Vec<String>> {
        self.info.references.clone()
    }
}

impl LintRule for WasmLintRule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn category(&self) -> &'static str {
        self.category
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn check(&self, config: &Config, path: &Path) -> Vec<LintError> {
        match self.execute_check(config, path) {
            Ok(errors) => errors,
            Err(e) => {
                // Return a single error describing the plugin failure
                vec![LintError::new(
                    self.name,
                    self.category,
                    &format!("Plugin execution failed: {}", e),
                    Severity::Error,
                )]
            }
        }
    }
}

// WasmLintRule is Send + Sync because:
// - Engine is Send + Sync
// - Module (wrapped in Arc) is Send + Sync
// - Instance cache is thread-local (no sharing between threads)
// - All other fields are primitive or owned types
unsafe impl Send for WasmLintRule {}
unsafe impl Sync for WasmLintRule {}
