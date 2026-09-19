//! Plugin SDK for building nginx-lint WASM plugins
//!
//! This crate provides everything needed to create custom lint rules as WASM plugins
//! for [nginx-lint](https://github.com/walf443/nginx-lint).
//!
//! # Getting Started
//!
//! 1. Create a library crate with `crate-type = ["cdylib", "rlib"]`
//! 2. Implement the [`Plugin`] trait
//! 3. Register with [`export_component_plugin!`]
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
//! // export_component_plugin!(MyRule);  // Required for WASM build
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
/// the [`helpers`] module, and the [`export_component_plugin!`] macro.
pub mod regex_scan;

pub mod prelude {
    pub use super::export_component_plugin;
    pub use super::export_component_plugins;
    pub use super::helpers;
    pub use super::types::API_VERSION;
    pub use super::types::*;
}

/// Macro to export a plugin as a WIT component
///
/// This generates the WIT component model exports for your plugin: the
/// `plugin-rules` world, carrying this one rule. It is
/// [`export_component_plugins!`] with a single plugin.
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
        $crate::export_component_plugins!($plugin_type);
    };
}

/// Macro to export several plugins as one WIT component
///
/// The component targets the `plugin-rules` world: it carries every rule
/// listed, and the host loads each as its own rule. Rules are listed in
/// the order they are given; the config is reconstructed once per `check`
/// and shared by the rules the host asked for.
///
/// `relevant_directives` still applies: when every rule the host asked for
/// declares one, the snapshot is pruned to the union of their names.
/// One rule without a declaration means the whole config is fetched.
///
/// # Example
///
/// ```ignore
/// use nginx_lint_plugin::prelude::*;
///
/// #[derive(Default)]
/// struct ServerTokens;
/// #[derive(Default)]
/// struct Autoindex;
///
/// impl Plugin for ServerTokens { /* ... */ }
/// impl Plugin for Autoindex { /* ... */ }
///
/// export_component_plugins!(ServerTokens, Autoindex);
/// ```
#[macro_export]
macro_rules! export_component_plugins {
    ($($plugin_type:ty),+ $(,)?) => {
        #[cfg(all(target_arch = "wasm32", feature = "wit-export"))]
        const _: () = {
            // Everything this block defines is spelled to stay out of the
            // way of `$plugin_type`, which is resolved inside it: an item
            // named `Rule`, or a module named `rules`, in the plugin crate
            // would otherwise be shadowed. macro_rules hygiene does not
            // cover item names.
            static __NGINX_LINT_RULES: std::sync::OnceLock<
                Vec<$crate::wit_guest::rules::ExportedRule>,
            > = std::sync::OnceLock::new();

            fn __nginx_lint_rules() -> &'static [$crate::wit_guest::rules::ExportedRule] {
                __NGINX_LINT_RULES.get_or_init(|| {
                    vec![
                        $(
                            {
                                static __NGINX_LINT_PLUGIN: std::sync::OnceLock<$plugin_type> =
                                    std::sync::OnceLock::new();
                                fn __nginx_lint_plugin() -> &'static $plugin_type {
                                    __NGINX_LINT_PLUGIN.get_or_init(|| <$plugin_type>::default())
                                }
                                $crate::wit_guest::rules::ExportedRule {
                                    name: $crate::Plugin::spec(__nginx_lint_plugin()).name,
                                    relevant_directives: $crate::Plugin::relevant_directives(
                                        __nginx_lint_plugin(),
                                    ),
                                    spec: || $crate::Plugin::spec(__nginx_lint_plugin()),
                                    check: |config, path| {
                                        $crate::Plugin::check(__nginx_lint_plugin(), config, path)
                                    },
                                }
                            },
                        )+
                    ]
                })
            }

            struct __NginxLintExport;

            impl $crate::wit_guest::rules::Guest for __NginxLintExport {
                fn specs() -> Vec<$crate::wit_guest::nginx_lint::plugin::types::PluginSpec> {
                    $crate::wit_guest::rules::specs_of(__nginx_lint_rules())
                }

                fn check(
                    config: &$crate::wit_guest::nginx_lint::plugin::config_api::Config,
                    path: String,
                    names: Vec<String>,
                ) -> Vec<$crate::wit_guest::nginx_lint::plugin::types::LintError> {
                    $crate::wit_guest::rules::check_asked(__nginx_lint_rules(), config, &path, &names)
                }
            }

            $crate::wit_guest::rules::export_rules!(__NginxLintExport with_types_in $crate::wit_guest::rules);
        };
    };
}
