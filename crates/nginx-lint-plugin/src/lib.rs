//! Plugin SDK for building nginx-lint WASM plugins
//!
//! This crate provides everything needed to create custom lint rules as WASM plugins
//! for [nginx-lint](https://github.com/walf443/nginx-lint).
//!
//! # Getting Started
//!
//! 1. Create a library crate with `crate-type = ["cdylib", "rlib"]`
//! 2. Implement the [`Plugin`] trait
//! 3. Register with [`export_plugin!`]
//! 4. Build with `cargo build --target wasm32-unknown-unknown --release`
//!
//! # Modules
//!
//! - [`types`] - Core types: [`Plugin`], [`PluginSpec`], [`LintError`], [`Fix`],
//!   [`ConfigExt`], [`DirectiveExt`]
//! - [`helpers`] - Utility functions for common checks (domain names, URLs, etc.)
//! - [`testing`] - Test runner and builder: [`testing::PluginTestRunner`], [`testing::TestCase`]
//! - [`native`] - [`native::NativePluginRule`] adapter for running plugins without WASM
//! - [`prelude`] - Convenient re-exports for `use nginx_lint_plugin::prelude::*`
//!
//! # API Versioning
//!
//! Plugins declare the API version they use via [`PluginSpec::api_version`].
//! This allows the host to support multiple output formats for backward compatibility.
//! [`PluginSpec::new()`] automatically sets the current API version ([`API_VERSION`]).
//!
//! # Example
//!
//! ```
//! use nginx_lint_plugin::prelude::*;
//!
//! #[derive(Default)]
//! struct MyRule;
//!
//! impl Plugin for MyRule {
//!     fn spec(&self) -> PluginSpec {
//!         PluginSpec::new("my-custom-rule", "custom", "My custom lint rule")
//!             .with_severity("warning")
//!             .with_why("Explain why this rule matters.")
//!             .with_bad_example("server {\n    dangerous_directive on;\n}")
//!             .with_good_example("server {\n    # dangerous_directive removed\n}")
//!     }
//!
//!     fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
//!         let mut errors = Vec::new();
//!         let err = self.spec().error_builder();
//!
//!         for ctx in config.all_directives_with_context() {
//!             if ctx.directive.is("dangerous_directive") {
//!                 errors.push(
//!                     err.warning_at("Avoid using dangerous_directive", ctx.directive)
//!                 );
//!             }
//!         }
//!         errors
//!     }
//! }
//!
//! // export_plugin!(MyRule);  // Required for WASM build
//!
//! // Verify it works
//! let plugin = MyRule;
//! let config = nginx_lint_plugin::parse_string("dangerous_directive on;").unwrap();
//! let errors = plugin.check(&config, "test.conf");
//! assert_eq!(errors.len(), 1);
//! ```

pub mod helpers;
pub mod native;
pub mod testing;
mod types;

#[cfg(feature = "container-testing")]
pub mod container_testing;

#[cfg(feature = "wit-export")]
pub mod wasm_config;
#[cfg(feature = "wit-export")]
pub mod wit_guest;

pub use types::*;

// Re-export common types from nginx-lint-common
pub use nginx_lint_common::parse_string;
pub use nginx_lint_common::parser;

/// Prelude module for convenient imports.
///
/// Importing everything from this module is the recommended way to use the SDK:
///
/// ```
/// use nginx_lint_plugin::prelude::*;
///
/// // All core types are now available
/// let spec = PluginSpec::new("example", "test", "Example rule");
/// assert_eq!(spec.name, "example");
/// ```
///
/// This re-exports all core types ([`Plugin`], [`PluginSpec`], [`LintError`], [`Fix`],
/// [`Config`], [`Directive`], etc.), extension traits ([`ConfigExt`], [`DirectiveExt`]),
/// the [`helpers`] module, and the [`export_plugin!`] macro.
pub mod prelude {
    pub use super::export_component_plugin;
    pub use super::export_plugin;
    pub use super::helpers;
    pub use super::types::API_VERSION;
    pub use super::types::*;
}

/// Macro to export a plugin implementation (legacy core module format)
///
/// **Deprecated**: Use [`export_component_plugin!`] instead, which generates
/// WIT component model exports. This macro generates legacy core WASM module
/// exports and will be removed in a future version.
///
/// # Example
///
/// ```
/// use nginx_lint_plugin::prelude::*;
///
/// #[derive(Default)]
/// struct MyPlugin;
///
/// impl Plugin for MyPlugin {
///     fn spec(&self) -> PluginSpec {
///         PluginSpec::new("my-plugin", "custom", "My plugin")
///     }
///
///     fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
///         Vec::new()
///     }
/// }
///
/// // Preferred:
/// export_component_plugin!(MyPlugin);
/// ```
#[deprecated(
    since = "0.7.0",
    note = "Use export_component_plugin! instead for WIT component model support"
)]
#[doc(hidden)]
pub fn _export_plugin_deprecated() {}

#[macro_export]
macro_rules! export_plugin {
    ($plugin_type:ty) => {
        const _: fn() = $crate::_export_plugin_deprecated;

        #[cfg(all(target_arch = "wasm32", feature = "wasm-export"))]
        const _: () = {
            static PLUGIN: std::sync::OnceLock<$plugin_type> = std::sync::OnceLock::new();
            static PLUGIN_SPEC_CACHE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
            static CHECK_RESULT_CACHE: std::sync::Mutex<String> =
                std::sync::Mutex::new(String::new());

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

            /// Get the length of the plugin spec JSON
            #[unsafe(no_mangle)]
            pub extern "C" fn plugin_spec_len() -> u32 {
                let info = PLUGIN_SPEC_CACHE.get_or_init(|| {
                    let plugin = get_plugin();
                    let info = $crate::Plugin::spec(plugin);
                    serde_json::to_string(&info).unwrap_or_default()
                });
                info.len() as u32
            }

            /// Get the plugin spec JSON pointer
            #[unsafe(no_mangle)]
            pub extern "C" fn plugin_spec() -> *const u8 {
                let info = PLUGIN_SPEC_CACHE.get_or_init(|| {
                    let plugin = get_plugin();
                    let info = $crate::Plugin::spec(plugin);
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
                let config: $crate::Config = match serde_json::from_str(config_json) {
                    Ok(c) => c,
                    Err(e) => {
                        let errors = vec![$crate::LintError::error(
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
                let errors = $crate::Plugin::check(plugin, &config, path);

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
    };
}

/// Macro to export a plugin as a WIT component
///
/// This generates the WIT component model exports for your plugin.
/// Use this instead of `export_plugin!` when building component model plugins.
///
/// # Example
///
/// ```ignore
/// use nginx_lint_plugin::prelude::*;
///
/// #[derive(Default)]
/// struct MyPlugin;
///
/// impl Plugin for MyPlugin {
///     fn spec(&self) -> PluginSpec { /* ... */ }
///     fn check(&self, config: &Config, _path: &str) -> Vec<LintError> { /* ... */ }
/// }
///
/// export_component_plugin!(MyPlugin);
/// ```
#[macro_export]
macro_rules! export_component_plugin {
    ($plugin_type:ty) => {
        #[cfg(all(target_arch = "wasm32", feature = "wit-export"))]
        const _: () = {
            use $crate::wit_guest::Guest;

            static PLUGIN: std::sync::OnceLock<$plugin_type> = std::sync::OnceLock::new();

            fn get_plugin() -> &'static $plugin_type {
                PLUGIN.get_or_init(|| <$plugin_type>::default())
            }

            struct ComponentExport;

            impl Guest for ComponentExport {
                fn spec() -> $crate::wit_guest::nginx_lint::plugin::types::PluginSpec {
                    let plugin = get_plugin();
                    let sdk_spec = $crate::Plugin::spec(plugin);
                    $crate::wit_guest::convert_spec(sdk_spec)
                }

                fn check(
                    config: &$crate::wit_guest::nginx_lint::plugin::config_api::Config,
                    path: String,
                ) -> Vec<$crate::wit_guest::nginx_lint::plugin::types::LintError> {
                    let plugin = get_plugin();
                    // Reconstruct parser Config from host resource handle
                    let config = $crate::wit_guest::reconstruct_config(config);
                    let errors = $crate::Plugin::check(plugin, &config, &path);
                    errors
                        .into_iter()
                        .map($crate::wit_guest::convert_lint_error)
                        .collect()
                }
            }

    $crate::wit_guest::export!(ComponentExport with_types_in $crate::wit_guest);
        };
    };
}
