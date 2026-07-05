//! Plugin system for custom lint rules
//!
//! This module provides support for loading and executing custom lint rules
//! implemented as WebAssembly component model plugins or native Rust plugins.

#[cfg(feature = "plugins")]
pub mod builtin;
#[cfg(feature = "plugins")]
mod component_rule;
#[cfg(feature = "plugins")]
mod error;
#[cfg(feature = "plugins")]
mod loader;
#[cfg(feature = "native-builtin-plugins")]
pub mod native_builtin;
#[cfg(feature = "plugins")]
pub use component_rule::ComponentLintRule;
#[cfg(feature = "plugins")]
pub use error::PluginError;
#[cfg(feature = "plugins")]
pub use loader::{CompilationCache, PluginLoader};

/// Current API version for the plugin interface.
///
/// Informational only: plugins report the SDK's version in
/// `PluginSpec.api_version`, and nothing compares it at runtime. Actual
/// compatibility is enforced structurally by WIT import resolution — a
/// plugin instantiates iff the host provides every function the plugin
/// imports. Hosts therefore stay compatible with plugins built against
/// older SDKs (the WIT interface only ever gains functions), while a
/// plugin built against a newer SDK fails to instantiate on an older host
/// with a missing-import error.
pub const API_VERSION: &str = "1.2";

/// Names of builtin plugins
pub const BUILTIN_PLUGIN_NAMES: &[&str] = &[
    "server-tokens-enabled",
    "autoindex-enabled",
    "gzip-not-enabled",
    "duplicate-directive",
    "space-before-semicolon",
    "trailing-whitespace",
    "block-lines",
    "proxy-pass-domain",
    "upstream-server-no-resolve",
    "directive-inheritance",
    "root-in-location",
    "alias-location-slash-mismatch",
    "proxy-pass-with-uri",
    "proxy-keepalive",
    "try-files-with-proxy",
    "if-is-evil-in-location",
    "unreachable-location",
    "missing-error-log",
    "deprecated-ssl-protocol",
    "weak-ssl-ciphers",
    "invalid-directive-context",
    "map-missing-default",
    "ssl-on-deprecated",
    "listen-http2-deprecated",
    "proxy-missing-host-header",
    "client-max-body-size-not-set",
    "nginx-rift",
];

/// Check if a rule name is a builtin plugin
pub fn is_builtin_plugin(name: &str) -> bool {
    BUILTIN_PLUGIN_NAMES.contains(&name)
}
