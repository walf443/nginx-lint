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
    /// trailing-whitespace plugin
    pub const TRAILING_WHITESPACE: &[u8] =
        include_bytes!("../../target/builtin-plugins/trailing_whitespace.wasm");
    /// proxy-pass-domain plugin
    pub const PROXY_PASS_DOMAIN: &[u8] =
        include_bytes!("../../target/builtin-plugins/proxy_pass_domain.wasm");
    /// upstream-server-no-resolve plugin
    pub const UPSTREAM_SERVER_NO_RESOLVE: &[u8] =
        include_bytes!("../../target/builtin-plugins/upstream_server_no_resolve.wasm");
    /// proxy-set-header-inheritance plugin
    pub const PROXY_SET_HEADER_INHERITANCE: &[u8] =
        include_bytes!("../../target/builtin-plugins/proxy_set_header_inheritance.wasm");
    /// root-in-location plugin
    pub const ROOT_IN_LOCATION: &[u8] =
        include_bytes!("../../target/builtin-plugins/root_in_location.wasm");
    /// alias-trailing-slash plugin
    pub const ALIAS_TRAILING_SLASH: &[u8] =
        include_bytes!("../../target/builtin-plugins/alias_trailing_slash.wasm");
}

/// Names of builtin plugins (used to skip native rules when builtin is enabled)
pub const BUILTIN_PLUGIN_NAMES: &[&str] = &[
    "server-tokens-enabled",
    "autoindex-enabled",
    "gzip-not-enabled",
    "duplicate-directive",
    "space-before-semicolon",
    "trailing-whitespace",
    "proxy-pass-domain",
    "upstream-server-no-resolve",
    "proxy-set-header-inheritance",
    "root-in-location",
    "alias-trailing-slash",
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

    // Load trailing-whitespace
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:trailing-whitespace"),
        embedded::TRAILING_WHITESPACE,
        loader.memory_limit(),
        loader.fuel_limit(),
    )?);

    // Load proxy-pass-domain
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:proxy-pass-domain"),
        embedded::PROXY_PASS_DOMAIN,
        loader.memory_limit(),
        loader.fuel_limit(),
    )?);

    // Load upstream-server-no-resolve
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:upstream-server-no-resolve"),
        embedded::UPSTREAM_SERVER_NO_RESOLVE,
        loader.memory_limit(),
        loader.fuel_limit(),
    )?);

    // Load proxy-set-header-inheritance
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:proxy-set-header-inheritance"),
        embedded::PROXY_SET_HEADER_INHERITANCE,
        loader.memory_limit(),
        loader.fuel_limit(),
    )?);

    // Load root-in-location
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:root-in-location"),
        embedded::ROOT_IN_LOCATION,
        loader.memory_limit(),
        loader.fuel_limit(),
    )?);

    // Load alias-trailing-slash
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:alias-trailing-slash"),
        embedded::ALIAS_TRAILING_SLASH,
        loader.memory_limit(),
        loader.fuel_limit(),
    )?);

    Ok(plugins)
}

/// Check if a rule name is a builtin plugin
pub fn is_builtin_plugin(name: &str) -> bool {
    BUILTIN_PLUGIN_NAMES.contains(&name)
}
