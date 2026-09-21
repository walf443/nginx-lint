//! Configuration management for nginx-lint.
//!
//! This module handles loading and validating the `.nginx-lint.toml`
//! configuration file. The main entry point is [`LintConfig`], which can be
//! loaded from a file with [`LintConfig::from_file`] or discovered
//! automatically with [`LintConfig::find_and_load`].
//!
//! The sections and values are in `types.rs`, validation in `validate.rs`,
//! the loading errors in `error.rs` and the `config init` template in
//! `template.rs`; everything is re-exported here, so
//! `nginx_lint_common::config::X` is the path for all of it.

mod error;
mod template;
#[cfg(test)]
mod tests;
mod types;
mod validate;

pub use error::ConfigError;
pub use template::DEFAULT_CONFIG_TEMPLATE;
pub use types::{
    AdditionalDirective, Color, ColorConfig, ColorMode, IncludeConfig, IndentSize, ParserConfig,
    PathMapping, PluginsConfig, RuleConfig,
};
pub use validate::ValidationError;

use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Configuration for nginx-lint loaded from `.nginx-lint.toml`.
///
/// Use [`from_file`](Self::from_file) to load from a specific path, or
/// [`find_and_load`](Self::find_and_load) to search the directory tree upward.
/// A default `LintConfig` enables all rules except those listed in
/// [`DISABLED_BY_DEFAULT`](Self::DISABLED_BY_DEFAULT).
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct LintConfig {
    /// Per-rule configuration keyed by rule name (e.g. `"indent"`, `"server-tokens-enabled"`).
    #[serde(default)]
    pub rules: HashMap<String, RuleConfig>,
    /// Color output settings.
    #[serde(default)]
    pub color: ColorConfig,
    /// Parser-level settings (e.g. additional block directives for extension modules).
    #[serde(default)]
    pub parser: ParserConfig,
    /// Include resolution settings (e.g. path mappings for include directives).
    #[serde(default)]
    pub include: IncludeConfig,
    /// Settings for plugins loaded with `--plugins`.
    #[serde(default)]
    pub plugins: PluginsConfig,
    /// Target nginx version (e.g. `"1.31.0"`).
    ///
    /// When set, rules whose declared version range does not include this
    /// version are automatically skipped. Stored as a raw string so an
    /// unparseable value does not fail config loading — the linter parses it
    /// lazily and emits a single warning if invalid.
    #[serde(default)]
    pub target_nginx_version: Option<String>,
    /// Cache directory for nginx-lint.
    ///
    /// Cacheable artifacts are stored in subdirectories beneath it (e.g.
    /// the WASM plugin compilation cache under `plugins/`). Defaults to the
    /// per-user cache directory (e.g. `~/.cache/nginx-lint` on Linux). A
    /// relative path is resolved against the directory containing the
    /// config file.
    #[serde(default)]
    pub cache_dir: Option<String>,
}

impl LintConfig {
    /// Load configuration from a file
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path).map_err(|e| ConfigError::IoError {
            path: path.to_path_buf(),
            source: e,
        })?;

        toml::from_str(&content).map_err(|e| ConfigError::ParseError {
            path: path.to_path_buf(),
            source: e,
        })
    }

    /// Parse configuration from a TOML string
    pub fn parse(content: &str) -> Result<Self, String> {
        toml::from_str(content).map_err(|e| e.to_string())
    }

    /// Find and load .nginx-lint.toml from the given directory or its parents.
    ///
    /// Returns the loaded config along with the path of the config file found.
    pub fn find_and_load(dir: &Path) -> Option<(Self, std::path::PathBuf)> {
        let mut current = dir.to_path_buf();

        loop {
            let config_path = current.join(".nginx-lint.toml");
            if config_path.exists() {
                return Self::from_file(&config_path)
                    .ok()
                    .map(|cfg| (cfg, config_path));
            }

            if !current.pop() {
                break;
            }
        }

        None
    }

    /// Rules that are disabled by default
    pub const DISABLED_BY_DEFAULT: &'static [&'static str] = &[
        "gzip-not-enabled", // gzip is not always appropriate (CDN, CPU constraints, security)
        "missing-error-log", // error_log is typically set at top level in main config
    ];

    /// Native lint rules implemented directly in the top-level crate
    /// (i.e. not packaged as plugins under `plugins/builtin/`).
    ///
    /// Exposed as a `pub const` so the drift-detection test in the top-level
    /// crate (`tests/known_rules_drift_test.rs`) can distinguish "native rule"
    /// from "stale builtin plugin entry" without duplicating this list.
    pub const NATIVE_RULE_NAMES: &'static [&'static str] = &[
        "unmatched-braces",
        "unclosed-quote",
        "missing-semicolon",
        "indent",
        "include-path-exists",
    ];

    /// All rule names recognised by `nginx-lint config validate`.
    ///
    /// This is the union of [`NATIVE_RULE_NAMES`](Self::NATIVE_RULE_NAMES) and
    /// the builtin plugin names. The builtin plugin list lives in the
    /// top-level crate (`nginx-lint`) as `BUILTIN_PLUGIN_NAMES`; because this
    /// module is a downstream dependency it cannot reference that symbol
    /// directly, so the two lists are kept in sync by a drift-detection unit
    /// test in the top-level crate (`tests/known_rules_drift_test.rs`).
    ///
    /// Order: native rules first, then builtin plugins in the same order as
    /// `BUILTIN_PLUGIN_NAMES` so review diffs against that file are obvious.
    pub const KNOWN_RULE_NAMES: &'static [&'static str] = &[
        // Native rules — must match `NATIVE_RULE_NAMES` above
        "unmatched-braces",
        "unclosed-quote",
        "missing-semicolon",
        "indent",
        "include-path-exists",
        // Builtin plugins — must match `BUILTIN_PLUGIN_NAMES` in
        // `src/plugin/mod.rs` (same order for easier review)
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
        "map-unnamed-capture",
    ];

    /// Check if a rule is enabled
    pub fn is_rule_enabled(&self, name: &str) -> bool {
        self.rules
            .get(name)
            .map(|r| r.enabled)
            .unwrap_or_else(|| !Self::DISABLED_BY_DEFAULT.contains(&name))
    }

    /// Whether the user explicitly wrote a `[rules.<name>]` section for this
    /// rule (irrespective of which options it contains). Used to distinguish
    /// "explicitly enabled" from "enabled by default" when warning about
    /// rules that fall outside the configured nginx version range.
    pub fn rule_explicitly_configured(&self, name: &str) -> bool {
        self.rules.contains_key(name)
    }

    /// Whether a rule has `skip_version_check = true` in its configuration.
    pub fn rule_skip_version_check(&self, name: &str) -> bool {
        self.rules
            .get(name)
            .map(|r| r.skip_version_check)
            .unwrap_or(false)
    }

    /// The configured target nginx version as a raw string, if any.
    pub fn target_nginx_version(&self) -> Option<&str> {
        self.target_nginx_version.as_deref()
    }

    /// Get the configuration for a specific rule
    pub fn get_rule_config(&self, name: &str) -> Option<&RuleConfig> {
        self.rules.get(name)
    }

    /// Get the color mode setting
    pub fn color_mode(&self) -> ColorMode {
        self.color.ui
    }

    /// Get additional block directives from config
    pub fn additional_block_directives(&self) -> &[String] {
        &self.parser.block_directives
    }

    /// Get include path mappings (applied in order to include patterns before resolving)
    pub fn include_path_mappings(&self) -> &[PathMapping] {
        &self.include.path_map
    }

    /// Generate the JSON Schema for the configuration file.
    ///
    /// The schema is derived from the Rust type definitions, so it automatically
    /// stays in sync with the actual configuration structure.
    pub fn json_schema() -> serde_json::Value {
        let generator = schemars::SchemaGenerator::default();
        let schema = generator.into_root_schema_for::<LintConfig>();
        serde_json::to_value(schema).unwrap()
    }

    /// Get include prefix (base directory for resolving relative include paths)
    pub fn include_prefix(&self) -> Option<&str> {
        self.include.prefix.as_deref()
    }

    /// Get the cache directory for nginx-lint (e.g. the WASM plugin
    /// compilation cache is stored under `plugins/` beneath it)
    pub fn cache_dir(&self) -> Option<&str> {
        self.cache_dir.as_deref()
    }

    /// Get additional contexts for invalid-directive-context rule
    pub fn additional_contexts(&self) -> Option<&HashMap<String, Vec<String>>> {
        self.rules
            .get("invalid-directive-context")
            .and_then(|r| r.additional_contexts.as_ref())
    }

    /// Get excluded directives for directive-inheritance rule
    pub fn directive_inheritance_excluded(&self) -> Option<&[String]> {
        self.rules
            .get("directive-inheritance")
            .and_then(|r| r.excluded_directives.as_deref())
    }

    /// Get additional directives for directive-inheritance rule
    pub fn directive_inheritance_additional(&self) -> Option<&[AdditionalDirective]> {
        self.rules
            .get("directive-inheritance")
            .and_then(|r| r.additional_directives.as_deref())
    }
}
