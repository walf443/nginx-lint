//! The sections and values of `.nginx-lint.toml`, as [`LintConfig`]
//! holds them.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::fmt;

/// Indent size configuration: either a fixed number or "auto" for auto-detection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndentSize {
    /// Auto-detect indent size from the first indented line
    #[default]
    Auto,
    /// Fixed indent size (number of spaces)
    Fixed(usize),
}

impl fmt::Display for IndentSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IndentSize::Auto => write!(f, "auto"),
            IndentSize::Fixed(n) => write!(f, "{}", n),
        }
    }
}

impl<'de> Deserialize<'de> for IndentSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{self, Visitor};

        struct IndentSizeVisitor;

        impl<'de> Visitor<'de> for IndentSizeVisitor {
            type Value = IndentSize;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a positive integer or \"auto\"")
            }

            fn visit_u64<E>(self, value: u64) -> Result<IndentSize, E>
            where
                E: de::Error,
            {
                Ok(IndentSize::Fixed(value as usize))
            }

            fn visit_i64<E>(self, value: i64) -> Result<IndentSize, E>
            where
                E: de::Error,
            {
                if value > 0 {
                    Ok(IndentSize::Fixed(value as usize))
                } else {
                    Err(de::Error::custom("indent_size must be positive"))
                }
            }

            fn visit_str<E>(self, value: &str) -> Result<IndentSize, E>
            where
                E: de::Error,
            {
                if value.eq_ignore_ascii_case("auto") {
                    Ok(IndentSize::Auto)
                } else {
                    Err(de::Error::custom(
                        "expected \"auto\" or a positive integer for indent_size",
                    ))
                }
            }
        }

        deserializer.deserialize_any(IndentSizeVisitor)
    }
}

impl JsonSchema for IndentSize {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "IndentSize".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "description": "Indentation size: a positive integer or \"auto\" for auto-detection",
            "default": "auto",
            "oneOf": [
                { "type": "integer", "minimum": 1 },
                { "type": "string", "enum": ["auto"] }
            ]
        }))
        .unwrap()
    }
}

/// Plugin configuration.
///
/// Only settings *about* plugins live here — never which plugins to load.
/// The directory is `--plugins` on the command line and nothing else, so a
/// configuration file discovered in a checked-out repository cannot cause any
/// WebAssembly to run.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PluginsConfig {
    /// Allow plugins that import WASI, the same grant as `--allow-wasi-plugins`.
    ///
    /// Named after the flag rather than shortened to `allow_wasi`, so the
    /// setting and the flag are searchable as one thing. It applies only to
    /// plugins the command line already asked for, and the flag and this
    /// setting are OR'd: once either says yes, WASI is linked. There is
    /// deliberately no way to say no from the command line — a project that
    /// needs this needs it on every run.
    #[serde(default)]
    pub allow_wasi_plugins: bool,
}

/// Parser configuration
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ParserConfig {
    /// Additional block directives (extension modules, etc.)
    /// These are added to the built-in list of block directives
    #[serde(default)]
    pub block_directives: Vec<String>,
}

/// A single path mapping rule for include directive resolution.
///
/// When an `include` directive's path contains path segment(s) that exactly
/// match `from`, they are replaced with `to` before the path is resolved on
/// disk.  Matching is performed at the path-component level (split by `/`),
/// so `from = "sites-enabled"` matches `.../sites-enabled/...` but does NOT
/// match `.../asites-enabled/...`.  Multi-segment values like
/// `from = "nginx/sites-enabled"` match consecutive components.
///
/// This is useful when the production config references a directory
/// (e.g. `sites-enabled`) that is only populated at runtime via symlinks,
/// and you want nginx-lint to evaluate the actual source files
/// (e.g. `sites-available`) instead.
///
/// Mappings are applied in declaration order and chained, so the output of one
/// mapping is fed into the next.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PathMapping {
    /// Path segment(s) to match in the include path (compared component-wise)
    pub from: String,
    /// Replacement path segment(s)
    pub to: String,
}

/// Include resolution configuration.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct IncludeConfig {
    /// Path mappings applied to include patterns before resolving them.
    /// Applied in declaration order; each mapping receives the output of the previous one.
    #[serde(default)]
    pub path_map: Vec<PathMapping>,
    /// Base directory for resolving relative include paths (similar to nginx `-p` prefix).
    /// When set, all relative include paths are resolved from this directory
    /// instead of the directory containing the config file with the include directive.
    pub prefix: Option<String>,
}

/// Color output configuration
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ColorConfig {
    /// Color mode: "auto" (default), "always", or "never"
    #[serde(default)]
    pub ui: ColorMode,
    /// Color for error messages (default: "red")
    #[serde(default = "default_error_color")]
    pub error: Color,
    /// Color for warning messages (default: "yellow")
    #[serde(default = "default_warning_color")]
    pub warning: Color,
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            ui: ColorMode::Auto,
            error: Color::Red,
            warning: Color::Yellow,
        }
    }
}

fn default_error_color() -> Color {
    Color::Red
}

fn default_warning_color() -> Color {
    Color::Yellow
}

/// Available colors for output
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    #[default]
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

impl JsonSchema for Color {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Color".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "string",
            "enum": [
                "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
                "bright_black", "bright_red", "bright_green", "bright_yellow",
                "bright_blue", "bright_magenta", "bright_cyan", "bright_white"
            ]
        }))
        .unwrap()
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "black" => Ok(Color::Black),
            "red" => Ok(Color::Red),
            "green" => Ok(Color::Green),
            "yellow" => Ok(Color::Yellow),
            "blue" => Ok(Color::Blue),
            "magenta" => Ok(Color::Magenta),
            "cyan" => Ok(Color::Cyan),
            "white" => Ok(Color::White),
            "bright_black" | "brightblack" => Ok(Color::BrightBlack),
            "bright_red" | "brightred" => Ok(Color::BrightRed),
            "bright_green" | "brightgreen" => Ok(Color::BrightGreen),
            "bright_yellow" | "brightyellow" => Ok(Color::BrightYellow),
            "bright_blue" | "brightblue" => Ok(Color::BrightBlue),
            "bright_magenta" | "brightmagenta" => Ok(Color::BrightMagenta),
            "bright_cyan" | "brightcyan" => Ok(Color::BrightCyan),
            "bright_white" | "brightwhite" => Ok(Color::BrightWhite),
            _ => Err(D::Error::custom(format!(
                "invalid color '{}', expected one of: black, red, green, yellow, blue, magenta, cyan, white, \
                 bright_black, bright_red, bright_green, bright_yellow, bright_blue, bright_magenta, bright_cyan, bright_white",
                s
            ))),
        }
    }
}

/// Color mode for output
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// Automatically detect (default) - respects NO_COLOR env and terminal detection
    #[default]
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}

impl JsonSchema for ColorMode {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ColorMode".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "string",
            "description": "Color mode: \"auto\" respects NO_COLOR env and terminal detection, \"always\" forces colors, \"never\" disables colors",
            "default": "auto",
            "enum": ["auto", "always", "never"]
        }))
        .unwrap()
    }
}

impl<'de> Deserialize<'de> for ColorMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "auto" => Ok(ColorMode::Auto),
            "always" => Ok(ColorMode::Always),
            "never" => Ok(ColorMode::Never),
            _ => Err(D::Error::custom(format!(
                "invalid color mode '{}', expected 'auto', 'always', or 'never'",
                s
            ))),
        }
    }
}

/// An additional directive to check for inheritance issues.
///
/// Used in `[rules.directive-inheritance]` configuration.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AdditionalDirective {
    /// The directive name (e.g., "proxy_set_cookie")
    pub name: String,
    /// Whether the first argument key comparison is case-insensitive (default: false)
    #[serde(default)]
    pub case_insensitive: bool,
    /// If true, all numeric arguments are separate keys like error_page (default: false)
    #[serde(default)]
    pub multi_key: bool,
}

/// Configuration for a specific lint rule.
///
/// Every `[rules.<name>]` section in `.nginx-lint.toml` is deserialized into
/// a `RuleConfig`. The only universal field is [`enabled`](Self::enabled);
/// the remaining fields are rule-specific options.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RuleConfig {
    /// Whether this rule is active (`true` by default for most rules).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// When true, run this rule regardless of the configured
    /// [`target_nginx_version`](super::LintConfig::target_nginx_version) and the rule's
    /// declared version range. Useful for opt-in overrides on rules that would
    /// otherwise be filtered out as not applicable to the target version.
    #[serde(default)]
    pub skip_version_check: bool,
    /// For indent rule: number or "auto" for auto-detection
    pub indent_size: Option<IndentSize>,
    /// For deprecated-ssl-protocol rule: allowed protocols (default: ["TLSv1.2", "TLSv1.3"])
    pub allowed_protocols: Option<Vec<String>>,
    /// For weak-ssl-ciphers rule: weak cipher patterns to detect
    pub weak_ciphers: Option<Vec<String>>,
    /// For weak-ssl-ciphers rule: required exclusion patterns
    pub required_exclusions: Option<Vec<String>>,
    /// For invalid-directive-context rule: additional valid parent contexts
    /// Format: { "server" = ["rtmp"], "upstream" = ["rtmp"] }
    pub additional_contexts: Option<HashMap<String, Vec<String>>>,
    /// For block-lines rule: maximum number of lines allowed in a block
    pub max_block_lines: Option<usize>,
    /// For directive-inheritance rule: directives to exclude from checking
    pub excluded_directives: Option<Vec<String>>,
    /// For directive-inheritance rule: additional directives to check
    pub additional_directives: Option<Vec<AdditionalDirective>>,
}

fn default_true() -> bool {
    true
}
