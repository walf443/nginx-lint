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
    /// alias-location-slash-mismatch plugin
    pub const ALIAS_LOCATION_SLASH_MISMATCH: &[u8] =
        include_bytes!("../../target/builtin-plugins/alias_location_slash_mismatch.wasm");
    /// proxy-pass-with-uri plugin
    pub const PROXY_PASS_WITH_URI: &[u8] =
        include_bytes!("../../target/builtin-plugins/proxy_pass_with_uri.wasm");
    /// add-header-inheritance plugin
    pub const ADD_HEADER_INHERITANCE: &[u8] =
        include_bytes!("../../target/builtin-plugins/add_header_inheritance.wasm");
    /// proxy-keepalive plugin
    pub const PROXY_KEEPALIVE: &[u8] =
        include_bytes!("../../target/builtin-plugins/proxy_keepalive.wasm");
    /// try-files-with-proxy plugin
    pub const TRY_FILES_WITH_PROXY: &[u8] =
        include_bytes!("../../target/builtin-plugins/try_files_with_proxy.wasm");
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
    "alias-location-slash-mismatch",
    "proxy-pass-with-uri",
    "add-header-inheritance",
    "proxy-keepalive",
    "try-files-with-proxy",
];

/// Global cache for the plugin loader (Engine is expensive to create)
#[cfg(feature = "builtin-plugins")]
static PLUGIN_LOADER_CACHE: std::sync::OnceLock<PluginLoader> = std::sync::OnceLock::new();

/// Global cache for compiled builtin plugins
/// This avoids recompiling WASM modules on every Linter creation
#[cfg(feature = "builtin-plugins")]
static BUILTIN_PLUGINS_CACHE: std::sync::OnceLock<Vec<WasmLintRule>> = std::sync::OnceLock::new();

/// Load all builtin plugins (with caching)
///
/// The first call compiles all WASM modules and caches them.
/// Subsequent calls clone from the cache, which is much faster.
///
/// Builtin plugins use a trusted loader with fuel metering disabled for better performance.
#[cfg(feature = "builtin-plugins")]
pub fn load_builtin_plugins() -> Result<Vec<WasmLintRule>, PluginError> {
    // Try to get from cache first
    if let Some(cached) = BUILTIN_PLUGINS_CACHE.get() {
        return Ok(cached.clone());
    }

    // Get or create the loader (use trusted mode for builtin plugins - no fuel metering)
    let loader = PLUGIN_LOADER_CACHE.get_or_init(|| {
        PluginLoader::new_trusted().expect("Failed to create PluginLoader")
    });

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
    let fuel_enabled = loader.fuel_enabled();

    // Load server-tokens-enabled
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:server-tokens-enabled"),
        embedded::SERVER_TOKENS_ENABLED,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load autoindex-enabled
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:autoindex-enabled"),
        embedded::AUTOINDEX_ENABLED,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load gzip-not-enabled
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:gzip-not-enabled"),
        embedded::GZIP_NOT_ENABLED,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load duplicate-directive
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:duplicate-directive"),
        embedded::DUPLICATE_DIRECTIVE,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load space-before-semicolon
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:space-before-semicolon"),
        embedded::SPACE_BEFORE_SEMICOLON,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load trailing-whitespace
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:trailing-whitespace"),
        embedded::TRAILING_WHITESPACE,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load proxy-pass-domain
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:proxy-pass-domain"),
        embedded::PROXY_PASS_DOMAIN,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load upstream-server-no-resolve
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:upstream-server-no-resolve"),
        embedded::UPSTREAM_SERVER_NO_RESOLVE,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load proxy-set-header-inheritance
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:proxy-set-header-inheritance"),
        embedded::PROXY_SET_HEADER_INHERITANCE,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load root-in-location
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:root-in-location"),
        embedded::ROOT_IN_LOCATION,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load alias-location-slash-mismatch
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:alias-location-slash-mismatch"),
        embedded::ALIAS_LOCATION_SLASH_MISMATCH,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load proxy-pass-with-uri
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:proxy-pass-with-uri"),
        embedded::PROXY_PASS_WITH_URI,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load add-header-inheritance
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:add-header-inheritance"),
        embedded::ADD_HEADER_INHERITANCE,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load proxy-keepalive
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:proxy-keepalive"),
        embedded::PROXY_KEEPALIVE,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    // Load try-files-with-proxy
    plugins.push(WasmLintRule::new(
        loader.engine(),
        PathBuf::from("builtin:try-files-with-proxy"),
        embedded::TRY_FILES_WITH_PROXY,
        loader.memory_limit(),
        loader.fuel_limit(),
        fuel_enabled,
    )?);

    Ok(plugins)
}

/// Check if a rule name is a builtin plugin
pub fn is_builtin_plugin(name: &str) -> bool {
    BUILTIN_PLUGIN_NAMES.contains(&name)
}
