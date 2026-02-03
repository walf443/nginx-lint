//! Plugin SDK for building nginx-lint WASM plugins
//!
//! This module provides everything needed to create custom lint rules as WASM plugins.
//!
//! # API Versioning
//!
//! Plugins declare the API version they use via `PluginInfo::api_version`.
//! This allows the host to support multiple output formats for backward compatibility.
//! Use `PluginInfo::new()` to automatically set the current API version.
//!
//! # Example
//!
//! ```rust,ignore
//! use nginx_lint::plugin_sdk::prelude::*;
//!
//! struct MyRule;
//!
//! impl Plugin for MyRule {
//!     fn info(&self) -> PluginInfo {
//!         PluginInfo::new(
//!             "my-custom-rule",
//!             "custom",
//!             "My custom lint rule",
//!         )
//!     }
//!
//!     fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
//!         let mut errors = Vec::new();
//!         for directive in config.all_directives() {
//!             if directive.name == "dangerous_directive" {
//!                 errors.push(LintError::warning(
//!                     "my-custom-rule",
//!                     "custom",
//!                     "Avoid using dangerous_directive",
//!                     directive.span.start.line,
//!                     directive.span.start.column,
//!                 ));
//!             }
//!         }
//!         errors
//!     }
//! }
//!
//! // Register the plugin
//! export_plugin!(MyRule);
//! ```

mod types;
pub mod helpers;
pub mod testing;

pub use types::*;

/// Prelude module for convenient imports
pub mod prelude {
    pub use super::types::*;
    pub use super::export_plugin;
    pub use super::types::API_VERSION;
    pub use super::helpers;
}

/// Macro to export a plugin implementation
///
/// This macro generates all the required WASM exports for your plugin.
///
/// # Example
///
/// ```rust,ignore
/// use nginx_lint::plugin_sdk::prelude::*;
///
/// struct MyPlugin;
///
/// impl Plugin for MyPlugin {
///     fn info(&self) -> PluginInfo {
///         PluginInfo::new("my-plugin", "custom", "My plugin")
///     }
///
///     fn check(&self, config: &Config, path: &str) -> Vec<LintError> {
///         Vec::new()
///     }
/// }
///
/// export_plugin!(MyPlugin);
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($plugin_type:ty) => {
        static PLUGIN: std::sync::OnceLock<$plugin_type> = std::sync::OnceLock::new();
        static PLUGIN_INFO_CACHE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        static CHECK_RESULT_CACHE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

        fn get_plugin() -> &'static $plugin_type {
            PLUGIN.get_or_init(|| <$plugin_type>::default())
        }

        /// Allocate memory for the host to write data
        #[unsafe(no_mangle)]
        pub extern "C" fn alloc(size: u32) -> *mut u8 {
            let layout = std::alloc::Layout::from_size_align(size as usize, 1).unwrap();
            unsafe { std::alloc::alloc(layout) }
        }

        /// Deallocate memory
        #[unsafe(no_mangle)]
        pub extern "C" fn dealloc(ptr: *mut u8, size: u32) {
            let layout = std::alloc::Layout::from_size_align(size as usize, 1).unwrap();
            unsafe { std::alloc::dealloc(ptr, layout) }
        }

        /// Get the length of the plugin info JSON
        #[unsafe(no_mangle)]
        pub extern "C" fn plugin_info_len() -> u32 {
            let info = PLUGIN_INFO_CACHE.get_or_init(|| {
                let plugin = get_plugin();
                let info = $crate::plugin_sdk::Plugin::info(plugin);
                serde_json::to_string(&info).unwrap_or_default()
            });
            info.len() as u32
        }

        /// Get the plugin info JSON pointer
        #[unsafe(no_mangle)]
        pub extern "C" fn plugin_info() -> *const u8 {
            let info = PLUGIN_INFO_CACHE.get_or_init(|| {
                let plugin = get_plugin();
                let info = $crate::plugin_sdk::Plugin::info(plugin);
                serde_json::to_string(&info).unwrap_or_default()
            });
            info.as_ptr()
        }

        /// Run the lint check
        #[unsafe(no_mangle)]
        pub extern "C" fn check(
            config_ptr: *const u8,
            config_len: u32,
            path_ptr: *const u8,
            path_len: u32,
        ) -> *const u8 {
            // Read config JSON from memory
            let config_json = unsafe {
                let slice = std::slice::from_raw_parts(config_ptr, config_len as usize);
                std::str::from_utf8_unchecked(slice)
            };

            // Read path from memory
            let path = unsafe {
                let slice = std::slice::from_raw_parts(path_ptr, path_len as usize);
                std::str::from_utf8_unchecked(slice)
            };

            // Parse config
            let config: $crate::plugin_sdk::Config = match serde_json::from_str(config_json) {
                Ok(c) => c,
                Err(e) => {
                    let errors = vec![$crate::plugin_sdk::LintError::error(
                        "plugin-error",
                        "plugin",
                        &format!("Failed to parse config: {}", e),
                        0,
                        0,
                    )];
                    let result = serde_json::to_string(&errors).unwrap_or_default();
                    let mut cache = CHECK_RESULT_CACHE.lock().unwrap();
                    *cache = result;
                    return cache.as_ptr();
                }
            };

            // Run the check
            let plugin = get_plugin();
            let errors = $crate::plugin_sdk::Plugin::check(plugin, &config, path);

            // Serialize result
            let result = serde_json::to_string(&errors).unwrap_or_else(|_| "[]".to_string());
            let mut cache = CHECK_RESULT_CACHE.lock().unwrap();
            *cache = result;
            cache.as_ptr()
        }

        /// Get the length of the check result
        #[unsafe(no_mangle)]
        pub extern "C" fn check_result_len() -> u32 {
            let cache = CHECK_RESULT_CACHE.lock().unwrap();
            cache.len() as u32
        }
    };
}

pub use export_plugin;
