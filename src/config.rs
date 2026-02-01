use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Configuration for nginx-lint loaded from .nginx-lint.toml
#[derive(Debug, Default, Deserialize)]
pub struct LintConfig {
    #[serde(default)]
    pub rules: HashMap<String, RuleConfig>,
    #[serde(default)]
    pub color: ColorConfig,
}

/// Color output configuration
#[derive(Debug, Clone, Deserialize)]
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
    /// Color for info messages (default: "blue")
    #[serde(default = "default_info_color")]
    pub info: Color,
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            ui: ColorMode::Auto,
            error: Color::Red,
            warning: Color::Yellow,
            info: Color::Blue,
        }
    }
}

fn default_error_color() -> Color {
    Color::Red
}

fn default_warning_color() -> Color {
    Color::Yellow
}

fn default_info_color() -> Color {
    Color::Blue
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

/// Configuration for a specific lint rule
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RuleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// For inconsistent-indentation rule
    pub indent_size: Option<usize>,
    /// For deprecated-ssl-protocol rule: allowed protocols (default: ["TLSv1.2", "TLSv1.3"])
    pub allowed_protocols: Option<Vec<String>>,
    /// For weak-ssl-ciphers rule: weak cipher patterns to detect
    pub weak_ciphers: Option<Vec<String>>,
    /// For weak-ssl-ciphers rule: required exclusion patterns
    pub required_exclusions: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
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

    /// Find and load .nginx-lint.toml from the given directory or its parents
    pub fn find_and_load(dir: &Path) -> Option<Self> {
        let mut current = dir.to_path_buf();

        loop {
            let config_path = current.join(".nginx-lint.toml");
            if config_path.exists() {
                return Self::from_file(&config_path).ok();
            }

            if !current.pop() {
                break;
            }
        }

        None
    }

    /// Check if a rule is enabled
    pub fn is_rule_enabled(&self, name: &str) -> bool {
        self.rules
            .get(name)
            .map(|r| r.enabled)
            .unwrap_or(true)
    }

    /// Get the configuration for a specific rule
    pub fn get_rule_config(&self, name: &str) -> Option<&RuleConfig> {
        self.rules.get(name)
    }

    /// Get the color mode setting
    pub fn color_mode(&self) -> ColorMode {
        self.color.ui
    }

    /// Validate a configuration file and return any errors
    pub fn validate_file(path: &Path) -> Result<Vec<ValidationError>, ConfigError> {
        let content = fs::read_to_string(path).map_err(|e| ConfigError::IoError {
            path: path.to_path_buf(),
            source: e,
        })?;

        Self::validate_content(&content, path)
    }

    /// Validate configuration content and return any errors
    fn validate_content(content: &str, path: &Path) -> Result<Vec<ValidationError>, ConfigError> {
        let value: toml::Value =
            toml::from_str(content).map_err(|e| ConfigError::ParseError {
                path: path.to_path_buf(),
                source: e,
            })?;

        let mut errors = Vec::new();

        if let toml::Value::Table(root) = value {
            // Known top-level keys
            let known_top_level: HashSet<&str> = ["rules", "color"].into_iter().collect();

            for key in root.keys() {
                if !known_top_level.contains(key.as_str()) {
                    errors.push(ValidationError::UnknownField {
                        path: key.clone(),
                        suggestion: suggest_field(key, &known_top_level),
                    });
                }
            }

            // Validate [color] section
            if let Some(toml::Value::Table(color)) = root.get("color") {
                let known_color_keys: HashSet<&str> =
                    ["ui", "error", "warning", "info"].into_iter().collect();

                for key in color.keys() {
                    if !known_color_keys.contains(key.as_str()) {
                        errors.push(ValidationError::UnknownField {
                            path: format!("color.{}", key),
                            suggestion: suggest_field(key, &known_color_keys),
                        });
                    }
                }
            }

            // Validate [rules.*] sections
            if let Some(toml::Value::Table(rules)) = root.get("rules") {
                let known_rules: HashSet<&str> = [
                    "duplicate-directive",
                    "unmatched-braces",
                    "unclosed-quote",
                    "missing-semicolon",
                    "deprecated-ssl-protocol",
                    "server-tokens-enabled",
                    "autoindex-enabled",
                    "weak-ssl-ciphers",
                    "inconsistent-indentation",
                    "gzip-not-enabled",
                    "missing-error-log",
                ]
                .into_iter()
                .collect();

                for (rule_name, rule_value) in rules {
                    if !known_rules.contains(rule_name.as_str()) {
                        errors.push(ValidationError::UnknownRule {
                            name: rule_name.clone(),
                            suggestion: suggest_field(rule_name, &known_rules),
                        });
                        continue;
                    }

                    // Validate rule options
                    if let toml::Value::Table(rule_config) = rule_value {
                        let known_rule_options = get_known_rule_options(rule_name);

                        for key in rule_config.keys() {
                            if !known_rule_options.contains(key.as_str()) {
                                errors.push(ValidationError::UnknownRuleOption {
                                    rule: rule_name.clone(),
                                    option: key.clone(),
                                    suggestion: suggest_field(key, &known_rule_options),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(errors)
    }
}

/// Get known options for a specific rule
fn get_known_rule_options(rule_name: &str) -> HashSet<&'static str> {
    let mut options: HashSet<&str> = ["enabled"].into_iter().collect();

    match rule_name {
        "inconsistent-indentation" => {
            options.insert("indent_size");
        }
        "deprecated-ssl-protocol" => {
            options.insert("allowed_protocols");
        }
        "weak-ssl-ciphers" => {
            options.insert("weak_ciphers");
            options.insert("required_exclusions");
        }
        _ => {}
    }

    options
}

/// Suggest a similar field name if one exists
fn suggest_field(input: &str, known: &HashSet<&str>) -> Option<String> {
    let input_lower = input.to_lowercase();

    // Find the closest match using simple edit distance
    known
        .iter()
        .filter(|&&k| {
            let k_lower = k.to_lowercase();
            // Simple heuristic: check if strings are similar
            k_lower.contains(&input_lower)
                || input_lower.contains(&k_lower)
                || levenshtein_distance(&input_lower, &k_lower) <= 2
        })
        .min_by_key(|&&k| levenshtein_distance(&input.to_lowercase(), &k.to_lowercase()))
        .map(|&s| s.to_string())
}

/// Simple Levenshtein distance implementation
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut matrix = vec![vec![0; b_len + 1]; a_len + 1];

    for (i, row) in matrix.iter_mut().enumerate().take(a_len + 1) {
        row[0] = i;
    }
    for (j, cell) in matrix[0].iter_mut().enumerate().take(b_len + 1) {
        *cell = j;
    }

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = usize::from(a_chars[i - 1] != b_chars[j - 1]);
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }

    matrix[a_len][b_len]
}

/// Validation error for configuration files
#[derive(Debug, Clone)]
pub enum ValidationError {
    UnknownField {
        path: String,
        suggestion: Option<String>,
    },
    UnknownRule {
        name: String,
        suggestion: Option<String>,
    },
    UnknownRuleOption {
        rule: String,
        option: String,
        suggestion: Option<String>,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::UnknownField { path, suggestion } => {
                write!(f, "unknown field '{}'", path)?;
                if let Some(s) = suggestion {
                    write!(f, ", did you mean '{}'?", s)?;
                }
                Ok(())
            }
            ValidationError::UnknownRule { name, suggestion } => {
                write!(f, "unknown rule '{}'", name)?;
                if let Some(s) = suggestion {
                    write!(f, ", did you mean '{}'?", s)?;
                }
                Ok(())
            }
            ValidationError::UnknownRuleOption {
                rule,
                option,
                suggestion,
            } => {
                write!(f, "unknown option '{}' for rule '{}'", option, rule)?;
                if let Some(s) = suggestion {
                    write!(f, ", did you mean '{}'?", s)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    IoError {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    ParseError {
        path: std::path::PathBuf,
        source: toml::de::Error,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::IoError { path, source } => {
                write!(f, "Failed to read config file '{}': {}", path.display(), source)
            }
            ConfigError::ParseError { path, source } => {
                write!(f, "Failed to parse config file '{}': {}", path.display(), source)
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::IoError { source, .. } => Some(source),
            ConfigError::ParseError { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = LintConfig::default();
        assert!(config.is_rule_enabled("any-rule"));
    }

    #[test]
    fn test_parse_config() {
        let toml_content = r#"
[rules.inconsistent-indentation]
enabled = true
indent_size = 2

[rules.server-tokens-enabled]
enabled = false
"#;
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", toml_content).unwrap();

        let config = LintConfig::from_file(file.path()).unwrap();

        assert!(config.is_rule_enabled("inconsistent-indentation"));
        assert!(!config.is_rule_enabled("server-tokens-enabled"));
        assert!(config.is_rule_enabled("unknown-rule"));

        let indent_config = config.get_rule_config("inconsistent-indentation").unwrap();
        assert_eq!(indent_config.indent_size, Some(2));
    }

    #[test]
    fn test_empty_config() {
        let toml_content = "";
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", toml_content).unwrap();

        let config = LintConfig::from_file(file.path()).unwrap();
        assert!(config.is_rule_enabled("any-rule"));
    }

    #[test]
    fn test_color_config_default() {
        let config = LintConfig::default();
        assert_eq!(config.color_mode(), ColorMode::Auto);
    }

    #[test]
    fn test_color_config_auto() {
        let toml_content = r#"
[color]
ui = "auto"
"#;
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", toml_content).unwrap();

        let config = LintConfig::from_file(file.path()).unwrap();
        assert_eq!(config.color_mode(), ColorMode::Auto);
    }

    #[test]
    fn test_color_config_never() {
        let toml_content = r#"
[color]
ui = "never"
"#;
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", toml_content).unwrap();

        let config = LintConfig::from_file(file.path()).unwrap();
        assert_eq!(config.color_mode(), ColorMode::Never);
    }

    #[test]
    fn test_color_config_always() {
        let toml_content = r#"
[color]
ui = "always"
"#;
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", toml_content).unwrap();

        let config = LintConfig::from_file(file.path()).unwrap();
        assert_eq!(config.color_mode(), ColorMode::Always);
    }

    #[test]
    fn test_color_config_default_colors() {
        let config = LintConfig::default();
        assert_eq!(config.color.error, Color::Red);
        assert_eq!(config.color.warning, Color::Yellow);
        assert_eq!(config.color.info, Color::Blue);
    }

    #[test]
    fn test_color_config_custom_colors() {
        let toml_content = r#"
[color]
error = "magenta"
warning = "cyan"
info = "green"
"#;
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", toml_content).unwrap();

        let config = LintConfig::from_file(file.path()).unwrap();
        assert_eq!(config.color.error, Color::Magenta);
        assert_eq!(config.color.warning, Color::Cyan);
        assert_eq!(config.color.info, Color::Green);
    }

    #[test]
    fn test_color_config_bright_colors() {
        let toml_content = r#"
[color]
error = "bright_red"
warning = "bright_yellow"
info = "bright_blue"
"#;
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", toml_content).unwrap();

        let config = LintConfig::from_file(file.path()).unwrap();
        assert_eq!(config.color.error, Color::BrightRed);
        assert_eq!(config.color.warning, Color::BrightYellow);
        assert_eq!(config.color.info, Color::BrightBlue);
    }
}
