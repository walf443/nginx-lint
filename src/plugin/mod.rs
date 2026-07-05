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

#[cfg(test)]
mod tests {
    use super::API_VERSION;

    /// Extract the value of an `API_VERSION = "..."` declaration from source
    /// code, tolerating Rust and TypeScript syntax.
    fn extract_api_version(source: &str) -> Option<String> {
        let line = source
            .lines()
            .find(|line| line.contains("API_VERSION") && line.contains("= \""))?;
        let start = line.find("= \"")? + 3;
        let end = line[start..].find('"')? + start;
        Some(line[start..end].to_string())
    }

    /// The plugin API version is declared in three places (this host
    /// constant, the Rust SDK, and the TypeScript SDK) that have already
    /// drifted apart once. Enforce that they stay identical. The SDK files
    /// are read from the workspace, so this only runs on a repo checkout.
    #[test]
    fn test_api_version_constants_stay_in_sync() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let declarations = [
            (
                "Rust SDK (crates/nginx-lint-plugin/src/types.rs)",
                workspace_root.join("crates/nginx-lint-plugin/src/types.rs"),
            ),
            (
                "TypeScript SDK (plugins/typescript/nginx-lint-plugin/src/index.ts)",
                workspace_root.join("plugins/typescript/nginx-lint-plugin/src/index.ts"),
            ),
        ];

        for (name, path) in declarations {
            let Ok(source) = std::fs::read_to_string(&path) else {
                // Not a full repo checkout (e.g. a published crate) — skip
                continue;
            };
            let version = extract_api_version(&source)
                .unwrap_or_else(|| panic!("no API_VERSION declaration found in {}", name));
            assert_eq!(
                version, API_VERSION,
                "{} declares API_VERSION \"{}\" but the host declares \"{}\" — \
                 bump them together",
                name, version, API_VERSION
            );
        }
    }
}
