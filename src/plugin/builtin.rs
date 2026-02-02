//! Builtin plugins embedded in the binary
//!
//! This module provides WASM plugins that are compiled into the binary.
//! Use `make build-with-plugins` to build with embedded plugins.

#[cfg(feature = "builtin-plugins")]
use super::{PluginError, PluginLoader, WasmLintRule};

/// Embedded WASM bytes for builtin plugins
#[cfg(feature = "builtin-plugins")]
mod embedded {
    /// server-tokens-enabled plugin
    pub const SERVER_TOKENS_ENABLED: &[u8] =
        include_bytes!("../../target/builtin-plugins/server_tokens_enabled.wasm");
    /// autoindex-enabled plugin
    pub const AUTOINDEX_ENABLED: &[u8] =
        include_bytes!("../../target/builtin-plugins/autoindex_enabled.wasm");
    /// gzip-not-enabled plugin
    pub const GZIP_NOT_ENABLED: &[u8] =
        include_bytes!("../../target/builtin-plugins/gzip_not_enabled.wasm");
}

/// Names of builtin plugins (used to skip native rules when builtin is enabled)
pub const BUILTIN_PLUGIN_NAMES: &[&str] = &[
    "server-tokens-enabled",
    "autoindex-enabled",
    "gzip-not-enabled",
];

/// Load all builtin plugins
#[cfg(feature = "builtin-plugins")]
pub fn load_builtin_plugins(loader: &PluginLoader) -> Result<Vec<WasmLintRule>, PluginError> {
    use std::path::PathBuf;

    let mut plugins = Vec::new();

    // Load server-tokens-enabled
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:server-tokens-enabled"),
        embedded::SERVER_TOKENS_ENABLED,
        loader.memory_limit(),
        loader.fuel_limit(),
    )?);

    // Load autoindex-enabled
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:autoindex-enabled"),
        embedded::AUTOINDEX_ENABLED,
        loader.memory_limit(),
        loader.fuel_limit(),
    )?);

    // Load gzip-not-enabled
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:gzip-not-enabled"),
        embedded::GZIP_NOT_ENABLED,
        loader.memory_limit(),
        loader.fuel_limit(),
    )?);

    Ok(plugins)
}

/// Check if a rule name is a builtin plugin
pub fn is_builtin_plugin(name: &str) -> bool {
    BUILTIN_PLUGIN_NAMES.contains(&name)
}
