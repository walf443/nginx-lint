use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Configuration for nginx-lint loaded from .nginx-lint.toml
#[derive(Debug, Default, Deserialize)]
pub struct LintConfig {
    #[serde(default)]
    pub rules: HashMap<String, RuleConfig>,
}

/// Configuration for a specific lint rule
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RuleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub indent_size: Option<usize>,
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
}
