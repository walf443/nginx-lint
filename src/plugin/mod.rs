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
pub use loader::PluginLoader;

/// Current API version for the plugin interface.
///
/// This version covers:
/// - Input: The Config/AST structure passed to plugins via WIT resource handles
/// - Output: The LintError structure returned by plugins via WIT types
///
/// Plugins declare which API version they use, and the host can support
/// multiple versions for backward compatibility.
pub const API_VERSION: &str = "1.0";

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
];

/// Check if a rule name is a builtin plugin
pub fn is_builtin_plugin(name: &str) -> bool {
    BUILTIN_PLUGIN_NAMES.contains(&name)
}
