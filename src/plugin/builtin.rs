//! Builtin plugins embedded in the binary
//!
//! This module provides WASM plugins that are compiled into the binary.
//! Use `make build-with-plugins` to build with embedded plugins.

#[cfg(feature = "wasm-builtin-plugins")]
use super::{ComponentLintRule, PluginError, PluginLoader};
#[cfg(feature = "wasm-builtin-plugins")]
use crate::linter::LintRule;

/// Embedded WASM bytes for builtin plugins
#[cfg(feature = "wasm-builtin-plugins")]
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
    /// block-lines plugin
    pub const BLOCK_LINES: &[u8] = include_bytes!("../../target/builtin-plugins/block_lines.wasm");
    /// proxy-pass-domain plugin
    pub const PROXY_PASS_DOMAIN: &[u8] =
        include_bytes!("../../target/builtin-plugins/proxy_pass_domain.wasm");
    /// upstream-server-no-resolve plugin
    pub const UPSTREAM_SERVER_NO_RESOLVE: &[u8] =
        include_bytes!("../../target/builtin-plugins/upstream_server_no_resolve.wasm");
    /// directive-inheritance plugin
    pub const DIRECTIVE_INHERITANCE: &[u8] =
        include_bytes!("../../target/builtin-plugins/directive_inheritance.wasm");
    /// root-in-location plugin
    pub const ROOT_IN_LOCATION: &[u8] =
        include_bytes!("../../target/builtin-plugins/root_in_location.wasm");
    /// alias-location-slash-mismatch plugin
    pub const ALIAS_LOCATION_SLASH_MISMATCH: &[u8] =
        include_bytes!("../../target/builtin-plugins/alias_location_slash_mismatch.wasm");
    /// proxy-pass-with-uri plugin
    pub const PROXY_PASS_WITH_URI: &[u8] =
        include_bytes!("../../target/builtin-plugins/proxy_pass_with_uri.wasm");
    /// proxy-keepalive plugin
    pub const PROXY_KEEPALIVE: &[u8] =
        include_bytes!("../../target/builtin-plugins/proxy_keepalive.wasm");
    /// try-files-with-proxy plugin
    pub const TRY_FILES_WITH_PROXY: &[u8] =
        include_bytes!("../../target/builtin-plugins/try_files_with_proxy.wasm");
    /// if-is-evil-in-location plugin
    pub const IF_IS_EVIL_IN_LOCATION: &[u8] =
        include_bytes!("../../target/builtin-plugins/if_is_evil_in_location.wasm");
    /// unreachable-location plugin
    pub const UNREACHABLE_LOCATION: &[u8] =
        include_bytes!("../../target/builtin-plugins/unreachable_location.wasm");
    /// missing-error-log plugin
    pub const MISSING_ERROR_LOG: &[u8] =
        include_bytes!("../../target/builtin-plugins/missing_error_log.wasm");
    /// deprecated-ssl-protocol plugin
    pub const DEPRECATED_SSL_PROTOCOL: &[u8] =
        include_bytes!("../../target/builtin-plugins/deprecated_ssl_protocol.wasm");
    /// weak-ssl-ciphers plugin
    pub const WEAK_SSL_CIPHERS: &[u8] =
        include_bytes!("../../target/builtin-plugins/weak_ssl_ciphers.wasm");
    /// invalid-directive-context plugin
    pub const INVALID_DIRECTIVE_CONTEXT: &[u8] =
        include_bytes!("../../target/builtin-plugins/invalid_directive_context.wasm");
    /// map-missing-default plugin
    pub const MAP_MISSING_DEFAULT: &[u8] =
        include_bytes!("../../target/builtin-plugins/map_missing_default.wasm");
    /// map-unnamed-capture plugin
    pub const MAP_UNNAMED_CAPTURE: &[u8] =
        include_bytes!("../../target/builtin-plugins/map_unnamed_capture.wasm");
    /// ssl-on-deprecated plugin
    pub const SSL_ON_DEPRECATED: &[u8] =
        include_bytes!("../../target/builtin-plugins/ssl_on_deprecated.wasm");
    /// listen-http2-deprecated plugin
    pub const LISTEN_HTTP2_DEPRECATED: &[u8] =
        include_bytes!("../../target/builtin-plugins/listen_http2_deprecated.wasm");
    /// proxy-missing-host-header plugin
    pub const PROXY_MISSING_HOST_HEADER: &[u8] =
        include_bytes!("../../target/builtin-plugins/proxy_missing_host_header.wasm");
    /// client-max-body-size-not-set plugin
    pub const CLIENT_MAX_BODY_SIZE_NOT_SET: &[u8] =
        include_bytes!("../../target/builtin-plugins/client_max_body_size_not_set.wasm");
    /// nginx-rift plugin
    pub const NGINX_RIFT: &[u8] = include_bytes!("../../target/builtin-plugins/nginx_rift.wasm");
}

// Re-export from parent module for backward compatibility
pub use super::{BUILTIN_PLUGIN_NAMES, is_builtin_plugin};

/// Global cache for the plugin loader (Engine is expensive to create)
#[cfg(feature = "wasm-builtin-plugins")]
static PLUGIN_LOADER_CACHE: std::sync::OnceLock<PluginLoader> = std::sync::OnceLock::new();

/// Compilation cache configuration for builtin plugin loading
#[cfg(feature = "wasm-builtin-plugins")]
static BUILTIN_PLUGIN_CACHE: std::sync::OnceLock<super::CompilationCache> =
    std::sync::OnceLock::new();

/// Configure the compilation cache used when loading builtin plugins.
///
/// Must be called before the first [`load_builtin_plugins`] call to take
/// effect; later calls are ignored because the loader and the compiled
/// plugins are cached globally. When never called, builtin plugins use
/// [`CompilationCache::Default`](super::CompilationCache::Default).
#[cfg(feature = "wasm-builtin-plugins")]
pub fn configure_builtin_plugin_cache(cache: super::CompilationCache) {
    let _ = BUILTIN_PLUGIN_CACHE.set(cache);
}

/// Per-plugin cache of compiled builtin plugins, keyed by plugin name.
///
/// Compiling a WASM module dominates load time, so each builtin is compiled
/// at most once per process, on first request. Caching per plugin (rather
/// than the whole set at once) lets [`load_builtin_plugins_filtered`] skip
/// plugins the current config disables without poisoning later calls that
/// need a different subset.
#[cfg(feature = "wasm-builtin-plugins")]
static BUILTIN_PLUGINS_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<&'static str, ComponentLintRule>>,
> = std::sync::OnceLock::new();

/// Get or create the shared trusted loader for builtin plugins
/// (no execution timeout; the engine is expensive to create)
#[cfg(feature = "wasm-builtin-plugins")]
fn plugin_loader() -> &'static PluginLoader {
    PLUGIN_LOADER_CACHE.get_or_init(|| {
        let cache = BUILTIN_PLUGIN_CACHE.get().cloned().unwrap_or_default();
        PluginLoader::new_trusted_with_cache(cache).unwrap_or_else(|e| {
            // Builtin plugins are embedded and must load even when the
            // configured cache directory is unusable: warn and fall back to
            // uncached compilation instead of failing the whole lint run.
            // Any other error (e.g. engine creation) is a real failure that
            // retrying without a cache would only mask.
            if !matches!(e, PluginError::CacheError { .. }) {
                panic!("Failed to create PluginLoader: {}", e);
            }
            eprintln!("Warning: builtin plugin compilation cache disabled: {}", e);
            PluginLoader::new_trusted_with_cache(super::CompilationCache::Disabled)
                .expect("Failed to create PluginLoader")
        })
    })
}

/// Load all builtin plugins (with caching)
///
/// Each plugin is compiled at most once per process; subsequent calls clone
/// from the cache, which is much faster.
///
/// Builtin plugins use a trusted loader with the execution timeout disabled for better performance.
#[cfg(feature = "wasm-builtin-plugins")]
pub fn load_builtin_plugins() -> Result<Vec<ComponentLintRule>, PluginError> {
    load_builtin_plugins_filtered(|_| true)
}

/// Load the builtin plugins whose name passes `filter`, in declaration order.
///
/// Plugins rejected by the filter are not compiled at all, so a config that
/// disables most builtin rules only pays compilation (or cache
/// deserialization) for the rules it actually runs. Accepted plugins are
/// compiled once per process and cached individually; a later call with a
/// broader filter compiles only the plugins not yet cached.
#[cfg(feature = "wasm-builtin-plugins")]
pub fn load_builtin_plugins_filtered(
    filter: impl Fn(&str) -> bool,
) -> Result<Vec<ComponentLintRule>, PluginError> {
    use std::path::PathBuf;

    let selected: Vec<(&'static str, &'static [u8])> = PLUGIN_ENTRIES
        .iter()
        .copied()
        .filter(|(name, _)| filter(name))
        .collect();

    let cache = BUILTIN_PLUGINS_CACHE.get_or_init(Default::default);
    let mut rules: std::collections::HashMap<&'static str, ComponentLintRule> = {
        let cache = cache.lock().expect("builtin plugin cache poisoned");
        selected
            .iter()
            .filter_map(|(name, _)| cache.get(name).map(|rule| (*name, rule.clone())))
            .collect()
    };

    let missing: Vec<(&'static str, &'static [u8])> = selected
        .iter()
        .copied()
        .filter(|(name, _)| !rules.contains_key(name))
        .collect();

    if !missing.is_empty() {
        let loader = plugin_loader();

        // Compile plugins in parallel: the serial phases of each component's
        // compilation overlap across plugins, which speeds up the first run /
        // cache misses. Order is preserved. The cache lock is not held while
        // compiling; two threads racing on the same plugin at worst compile
        // it twice, and the first insert wins.
        use rayon::prelude::*;
        let results: Vec<(&'static str, Result<ComponentLintRule, PluginError>)> = missing
            .par_iter()
            .map(|(name, bytes)| {
                let result = loader
                    .load_component_from_bytes(&PathBuf::from(format!("builtin:{}", name)), bytes);
                (*name, result)
            })
            .collect();

        let mut first_error = None;
        {
            let mut cache = cache.lock().expect("builtin plugin cache poisoned");
            for (name, result) in results {
                match result {
                    Ok(rule) => {
                        debug_assert_eq!(
                            rule.name(),
                            name,
                            "builtin plugin table name and spec name must match \
                             (the enabled-set filter operates on the table name)"
                        );
                        cache.entry(name).or_insert_with(|| rule.clone());
                        rules.insert(name, rule);
                    }
                    // Remember the first failure in declaration order for a
                    // deterministic error report, but still cache the
                    // plugins that succeeded.
                    Err(e) => {
                        if first_error.is_none() {
                            first_error = Some(e);
                        }
                    }
                }
            }
        }
        if let Some(e) = first_error {
            return Err(e);
        }
    }

    Ok(selected
        .iter()
        .map(|(name, _)| {
            rules
                .remove(name)
                .expect("every selected plugin is either cached or freshly compiled")
        })
        .collect())
}

/// Embedded builtin plugins in declaration order: `(rule name, WASM bytes)`.
///
/// The rule name must match the plugin's `spec()` name and the
/// `BUILTIN_PLUGIN_NAMES` table in `plugin/mod.rs` (enforced by a test
/// below); enabled/disabled filtering happens against this name before the
/// plugin is compiled.
#[cfg(feature = "wasm-builtin-plugins")]
const PLUGIN_ENTRIES: &[(&str, &[u8])] = &[
    ("server-tokens-enabled", embedded::SERVER_TOKENS_ENABLED),
    ("autoindex-enabled", embedded::AUTOINDEX_ENABLED),
    ("gzip-not-enabled", embedded::GZIP_NOT_ENABLED),
    ("duplicate-directive", embedded::DUPLICATE_DIRECTIVE),
    ("space-before-semicolon", embedded::SPACE_BEFORE_SEMICOLON),
    ("trailing-whitespace", embedded::TRAILING_WHITESPACE),
    ("block-lines", embedded::BLOCK_LINES),
    ("proxy-pass-domain", embedded::PROXY_PASS_DOMAIN),
    (
        "upstream-server-no-resolve",
        embedded::UPSTREAM_SERVER_NO_RESOLVE,
    ),
    ("directive-inheritance", embedded::DIRECTIVE_INHERITANCE),
    ("root-in-location", embedded::ROOT_IN_LOCATION),
    (
        "alias-location-slash-mismatch",
        embedded::ALIAS_LOCATION_SLASH_MISMATCH,
    ),
    ("proxy-pass-with-uri", embedded::PROXY_PASS_WITH_URI),
    ("proxy-keepalive", embedded::PROXY_KEEPALIVE),
    ("try-files-with-proxy", embedded::TRY_FILES_WITH_PROXY),
    ("if-is-evil-in-location", embedded::IF_IS_EVIL_IN_LOCATION),
    ("unreachable-location", embedded::UNREACHABLE_LOCATION),
    ("missing-error-log", embedded::MISSING_ERROR_LOG),
    ("deprecated-ssl-protocol", embedded::DEPRECATED_SSL_PROTOCOL),
    ("weak-ssl-ciphers", embedded::WEAK_SSL_CIPHERS),
    (
        "invalid-directive-context",
        embedded::INVALID_DIRECTIVE_CONTEXT,
    ),
    ("map-missing-default", embedded::MAP_MISSING_DEFAULT),
    ("ssl-on-deprecated", embedded::SSL_ON_DEPRECATED),
    ("listen-http2-deprecated", embedded::LISTEN_HTTP2_DEPRECATED),
    (
        "proxy-missing-host-header",
        embedded::PROXY_MISSING_HOST_HEADER,
    ),
    (
        "client-max-body-size-not-set",
        embedded::CLIENT_MAX_BODY_SIZE_NOT_SET,
    ),
    ("nginx-rift", embedded::NGINX_RIFT),
    ("map-unnamed-capture", embedded::MAP_UNNAMED_CAPTURE),
];

#[cfg(all(test, feature = "wasm-builtin-plugins"))]
mod tests {
    use super::*;

    /// The enabled/disabled filter operates on the `PLUGIN_ENTRIES` names
    /// before compiling, while everything else (config validation, docs,
    /// `--list-rules`) uses `BUILTIN_PLUGIN_NAMES`. The two tables are
    /// maintained by hand; a drift would make a rule impossible to
    /// enable/disable by name.
    #[test]
    fn test_plugin_entries_match_builtin_plugin_names() {
        let entry_names: Vec<&str> = PLUGIN_ENTRIES.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            entry_names, BUILTIN_PLUGIN_NAMES,
            "PLUGIN_ENTRIES and BUILTIN_PLUGIN_NAMES must list the same rules in the same order"
        );
    }

    /// Every loaded builtin's `spec()` name must match its `PLUGIN_ENTRIES`
    /// table name: the enabled/disabled filter operates on the table name,
    /// so a mismatch would make the rule impossible to filter under the
    /// name it reports errors as. The load path only checks this with a
    /// `debug_assert`; this test covers release builds too.
    #[test]
    fn test_loaded_spec_names_match_table_names() {
        super::configure_builtin_plugin_cache(crate::plugin::CompilationCache::Disabled);

        let rules = load_builtin_plugins().expect("builtin load should succeed");
        let spec_names: Vec<&str> = rules.iter().map(|rule| rule.name()).collect();
        assert_eq!(
            spec_names, BUILTIN_PLUGIN_NAMES,
            "spec() names must match PLUGIN_ENTRIES/BUILTIN_PLUGIN_NAMES in order"
        );
    }

    /// Filtered loading must compile only the requested plugins and return
    /// them in declaration order regardless of the filter's own ordering.
    #[test]
    fn test_load_builtin_plugins_filtered_returns_declaration_order() {
        // Avoid touching the real per-user cache directory from tests. This
        // is a process-wide OnceLock; setting it here is safe because unit
        // tests share the same requirement.
        super::configure_builtin_plugin_cache(crate::plugin::CompilationCache::Disabled);

        let wanted = ["trailing-whitespace", "autoindex-enabled"];
        let rules = load_builtin_plugins_filtered(|name| wanted.contains(&name))
            .expect("filtered builtin load should succeed");
        let names: Vec<&str> = rules.iter().map(|rule| rule.name()).collect();
        // Declaration order in PLUGIN_ENTRIES: autoindex-enabled comes first
        assert_eq!(names, ["autoindex-enabled", "trailing-whitespace"]);
    }
}
