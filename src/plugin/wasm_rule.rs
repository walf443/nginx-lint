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
use wasmtime::{Engine, Instance, Linker, Memory, Module, Store, TypedFunc};

/// Plugin info returned by the plugin_info export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub category: String,
    pub description: String,
}

/// Plugin lint error format (simplified for JSON transfer)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginLintError {
    pub rule: String,
    pub category: String,
    pub message: String,
    pub severity: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
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

        error
    }
}

/// Store data for WASM execution
struct StoreData {
    #[allow(dead_code)]
    memory_limit: u64,
}

/// Cached instance data for reuse
struct CachedInstance {
    store: Store<StoreData>,
    #[allow(dead_code)]
    instance: Instance,
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

/// A lint rule implemented as a WASM module
pub struct WasmLintRule {
    /// Path to the WASM file (for error reporting and cache key)
    path: PathBuf,
    /// Plugin metadata (kept for potential future use)
    #[allow(dead_code)]
    info: PluginInfo,
    /// Compiled WASM module (shared across threads)
    module: Arc<Module>,
    /// WASM engine reference (shared across threads)
    engine: Engine,
    /// Memory limit in bytes
    memory_limit: u64,
    /// Fuel limit for CPU metering
    fuel_limit: u64,
    /// Leaked static strings for LintRule trait
    name: &'static str,
    category: &'static str,
    description: &'static str,
}

impl WasmLintRule {
    /// Create a new WASM lint rule from compiled bytes
    pub fn new(
        engine: &Engine,
        path: PathBuf,
        wasm_bytes: &[u8],
        memory_limit: u64,
        fuel_limit: u64,
    ) -> Result<Self, PluginError> {
        // Compile the module
        let module = Module::new(engine, wasm_bytes)
            .map_err(|e| PluginError::compile_error(&path, e.to_string()))?;

        // Validate required exports exist
        Self::validate_exports(&module, &path)?;

        // Get plugin info by instantiating temporarily
        let info = Self::get_plugin_info(engine, &module, &path, memory_limit, fuel_limit)?;

        // Leak strings for 'static lifetime (these live for the program duration)
        let name: &'static str = Box::leak(info.name.clone().into_boxed_str());
        let category: &'static str = Box::leak(info.category.clone().into_boxed_str());
        let description: &'static str = Box::leak(info.description.clone().into_boxed_str());

        Ok(Self {
            path,
            info,
            module: Arc::new(module),
            engine: engine.clone(),
            memory_limit,
            fuel_limit,
            name,
            category,
            description,
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

        for export in &required_exports {
            if module.get_export(export).is_none() {
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
        memory_limit: u64,
        fuel_limit: u64,
    ) -> Result<PluginInfo, PluginError> {
        let mut store = Store::new(engine, StoreData { memory_limit });
        store.set_fuel(fuel_limit).map_err(|e| {
            PluginError::execution_error(path, format!("Failed to set fuel: {}", e))
        })?;

        let linker = Linker::new(engine);
        let instance = linker.instantiate(&mut store, module).map_err(|e| {
            PluginError::instantiate_error(path, e.to_string())
        })?;

        // Get memory
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| PluginError::missing_export(path, "memory"))?;

        // Get functions
        let plugin_info_len: TypedFunc<(), u32> = instance
            .get_typed_func(&mut store, "plugin_info_len")
            .map_err(|e| PluginError::missing_export(path, format!("plugin_info_len: {}", e)))?;

        let plugin_info: TypedFunc<(), u32> = instance
            .get_typed_func(&mut store, "plugin_info")
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
        let mut store = Store::new(&self.engine, StoreData {
            memory_limit: self.memory_limit,
        });
        store.set_fuel(self.fuel_limit).map_err(|e| {
            PluginError::execution_error(&self.path, format!("Failed to set fuel: {}", e))
        })?;

        let linker = Linker::new(&self.engine);
        let instance = linker.instantiate(&mut store, &self.module).map_err(|e| {
            PluginError::instantiate_error(&self.path, e.to_string())
        })?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| PluginError::missing_export(&self.path, "memory"))?;

        let alloc: TypedFunc<u32, u32> = instance
            .get_typed_func(&mut store, "alloc")
            .map_err(|e| PluginError::missing_export(&self.path, format!("alloc: {}", e)))?;

        let dealloc: TypedFunc<(u32, u32), ()> = instance
            .get_typed_func(&mut store, "dealloc")
            .map_err(|e| PluginError::missing_export(&self.path, format!("dealloc: {}", e)))?;

        let check: TypedFunc<(u32, u32, u32, u32), u32> = instance
            .get_typed_func(&mut store, "check")
            .map_err(|e| PluginError::missing_export(&self.path, format!("check: {}", e)))?;

        let check_result_len: TypedFunc<(), u32> = instance
            .get_typed_func(&mut store, "check_result_len")
            .map_err(|e| PluginError::missing_export(&self.path, format!("check_result_len: {}", e)))?;

        Ok(CachedInstance {
            store,
            instance,
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

        // Serialize config to JSON
        let config_json = serde_json::to_string(config).map_err(|e| {
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
                if e.to_string().contains("fuel") {
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

        // Parse result
        let plugin_errors: Vec<PluginLintError> = serde_json::from_str(&result_json).map_err(|e| {
            PluginError::result_parse_error(&self.path, format!("Invalid result JSON: {}", e))
        })?;

        Ok(plugin_errors.into_iter().map(|e| e.into_lint_error()).collect())
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
