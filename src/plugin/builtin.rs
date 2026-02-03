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
    /// duplicate-directive plugin
    pub const DUPLICATE_DIRECTIVE: &[u8] =
        include_bytes!("../../target/builtin-plugins/duplicate_directive.wasm");
    /// space-before-semicolon plugin
    pub const SPACE_BEFORE_SEMICOLON: &[u8] =
        include_bytes!("../../target/builtin-plugins/space_before_semicolon.wasm");
}

/// Names of builtin plugins (used to skip native rules when builtin is enabled)
pub const BUILTIN_PLUGIN_NAMES: &[&str] = &[
    "server-tokens-enabled",
    "autoindex-enabled",
    "gzip-not-enabled",
    "duplicate-directive",
    "space-before-semicolon",
];

/// Global cache for compiled builtin plugins
/// This avoids recompiling WASM modules on every Linter creation
#[cfg(feature = "builtin-plugins")]
static BUILTIN_PLUGINS_CACHE: std::sync::OnceLock<Vec<WasmLintRule>> = std::sync::OnceLock::new();

/// Load all builtin plugins (with caching)
///
/// The first call compiles all WASM modules and caches them.
/// Subsequent calls clone from the cache, which is much faster.
#[cfg(feature = "builtin-plugins")]
pub fn load_builtin_plugins(loader: &PluginLoader) -> Result<Vec<WasmLintRule>, PluginError> {
    // Try to get from cache first
    if let Some(cached) = BUILTIN_PLUGINS_CACHE.get() {
        return Ok(cached.clone());
    }

    // Compile plugins
    let plugins = compile_builtin_plugins(loader)?;

    // Try to store in cache (ignore if another thread beat us)
    let _ = BUILTIN_PLUGINS_CACHE.set(plugins.clone());

    Ok(plugins)
}

/// Compile all builtin plugins from embedded WASM bytes
#[cfg(feature = "builtin-plugins")]
fn compile_builtin_plugins(loader: &PluginLoader) -> Result<Vec<WasmLintRule>, PluginError> {
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

    // Load duplicate-directive
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:duplicate-directive"),
        embedded::DUPLICATE_DIRECTIVE,
        loader.memory_limit(),
        loader.fuel_limit(),
    )?);

    // Load space-before-semicolon
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:space-before-semicolon"),
        embedded::SPACE_BEFORE_SEMICOLON,
        loader.memory_limit(),
        loader.fuel_limit(),
    )?);

    Ok(plugins)
}

/// Check if a rule name is a builtin plugin
pub fn is_builtin_plugin(name: &str) -> bool {
    BUILTIN_PLUGIN_NAMES.contains(&name)
}
