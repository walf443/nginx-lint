//! Checking a configuration file for keys and rule names nothing reads,
//! with a suggestion for each — what `nginx-lint config validate` reports.

use super::{ConfigError, LintConfig};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

impl LintConfig {
    /// Validate a configuration file and return any errors
    pub fn validate_file(path: &Path) -> Result<Vec<ValidationError>, ConfigError> {
        Self::validate_file_with_rules(path, &HashSet::new())
    }

    /// Validate a configuration file, accepting `[rules.<name>]` sections
    /// for `extra_rules` as well as for the builtin rules. The rules of a
    /// `--plugins` directory are only known to the caller.
    pub fn validate_file_with_rules(
        path: &Path,
        extra_rules: &HashSet<String>,
    ) -> Result<Vec<ValidationError>, ConfigError> {
        let content = fs::read_to_string(path).map_err(|e| ConfigError::IoError {
            path: path.to_path_buf(),
            source: e,
        })?;

        Self::validate_content(&content, path, extra_rules)
    }

    /// Validate configuration content and return any errors
    fn validate_content(
        content: &str,
        path: &Path,
        extra_rules: &HashSet<String>,
    ) -> Result<Vec<ValidationError>, ConfigError> {
        let value: toml::Value = toml::from_str(content).map_err(|e| ConfigError::ParseError {
            path: path.to_path_buf(),
            source: e,
        })?;

        let mut errors = Vec::new();

        if let toml::Value::Table(root) = value {
            // Known top-level keys
            let known_top_level: HashSet<&str> = [
                "rules",
                "color",
                "parser",
                "include",
                "plugins",
                "target_nginx_version",
                "cache_dir",
            ]
            .into_iter()
            .collect();

            for key in root.keys() {
                if !known_top_level.contains(key.as_str()) {
                    let line = find_key_line(content, None, key);
                    errors.push(ValidationError::UnknownField {
                        path: key.clone(),
                        line,
                        suggestion: suggest_field(key, &known_top_level),
                    });
                }
            }

            // Validate [color] section
            if let Some(toml::Value::Table(color)) = root.get("color") {
                let known_color_keys: HashSet<&str> =
                    ["ui", "error", "warning"].into_iter().collect();

                for key in color.keys() {
                    if !known_color_keys.contains(key.as_str()) {
                        let line = find_key_line(content, Some("color"), key);
                        errors.push(ValidationError::UnknownField {
                            path: format!("color.{}", key),
                            line,
                            suggestion: suggest_field(key, &known_color_keys),
                        });
                    }
                }
            }

            // Validate [parser] section
            if let Some(toml::Value::Table(parser)) = root.get("parser") {
                let known_parser_keys: HashSet<&str> = ["block_directives"].into_iter().collect();

                for key in parser.keys() {
                    if !known_parser_keys.contains(key.as_str()) {
                        let line = find_key_line(content, Some("parser"), key);
                        errors.push(ValidationError::UnknownField {
                            path: format!("parser.{}", key),
                            line,
                            suggestion: suggest_field(key, &known_parser_keys),
                        });
                    }
                }
            }

            // Validate [include] section
            if let Some(toml::Value::Table(include)) = root.get("include") {
                let known_include_keys: HashSet<&str> =
                    ["path_map", "prefix"].into_iter().collect();

                for key in include.keys() {
                    if !known_include_keys.contains(key.as_str()) {
                        let line = find_key_line(content, Some("include"), key);
                        errors.push(ValidationError::UnknownField {
                            path: format!("include.{}", key),
                            line,
                            suggestion: suggest_field(key, &known_include_keys),
                        });
                    }
                }
            }

            // Validate [plugins] section
            if let Some(toml::Value::Table(plugins)) = root.get("plugins") {
                let known_plugins_keys: HashSet<&str> =
                    ["allow_wasi_plugins"].into_iter().collect();

                for key in plugins.keys() {
                    if !known_plugins_keys.contains(key.as_str()) {
                        let line = find_key_line(content, Some("plugins"), key);
                        errors.push(ValidationError::UnknownField {
                            path: format!("plugins.{}", key),
                            line,
                            suggestion: suggest_field(key, &known_plugins_keys),
                        });
                    }
                }
            }

            // Validate [rules.*] sections
            if let Some(toml::Value::Table(rules)) = root.get("rules") {
                let known_rules: HashSet<&str> = Self::KNOWN_RULE_NAMES
                    .iter()
                    .copied()
                    .chain(extra_rules.iter().map(String::as_str))
                    .collect();

                for (rule_name, rule_value) in rules {
                    if !known_rules.contains(rule_name.as_str()) {
                        let line = find_key_line(content, Some("rules"), rule_name);
                        errors.push(ValidationError::UnknownRule {
                            name: rule_name.clone(),
                            line,
                            suggestion: suggest_field(rule_name, &known_rules),
                        });
                        continue;
                    }

                    // Validate rule options
                    if let toml::Value::Table(rule_config) = rule_value {
                        let known_rule_options = get_known_rule_options(rule_name);
                        let section = format!("rules.{}", rule_name);

                        for key in rule_config.keys() {
                            if !known_rule_options.contains(key.as_str()) {
                                let line = find_key_line(content, Some(&section), key);
                                errors.push(ValidationError::UnknownRuleOption {
                                    rule: rule_name.clone(),
                                    option: key.clone(),
                                    line,
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

/// Find the line number where a key is defined in the TOML content
fn find_key_line(content: &str, section: Option<&str>, key: &str) -> Option<usize> {
    let lines: Vec<&str> = content.lines().collect();

    // For top-level sections (section is None), look for [key]
    if section.is_none() {
        let section_header = format!("[{}]", key);
        for (i, line) in lines.iter().enumerate() {
            if line.trim() == section_header {
                return Some(i + 1);
            }
        }
        return None;
    }

    let target_section = section.unwrap();
    let mut in_section = false;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Check for section header [section] or [section.subsection]
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section_name = &trimmed[1..trimmed.len() - 1];

            // Check if this is a [rules.rule-name] style section
            let full_section = format!("{}.{}", target_section, key);
            if section_name == full_section {
                return Some(i + 1);
            }

            in_section = section_name == target_section
                || section_name.starts_with(&format!("{}.", target_section));
            continue;
        }

        // Check for key = value within the section
        if in_section && let Some((k, _)) = trimmed.split_once('=') {
            let k = k.trim();
            if k == key {
                return Some(i + 1);
            }
        }
    }

    None
}

/// Get known options for a specific rule
fn get_known_rule_options(rule_name: &str) -> HashSet<&'static str> {
    let mut options: HashSet<&str> = ["enabled", "skip_version_check"].into_iter().collect();

    match rule_name {
        "indent" => {
            options.insert("indent_size");
        }
        "deprecated-ssl-protocol" => {
            options.insert("allowed_protocols");
        }
        "weak-ssl-ciphers" => {
            options.insert("weak_ciphers");
            options.insert("required_exclusions");
        }
        "block-lines" => {
            options.insert("max_block_lines");
        }
        "directive-inheritance" => {
            options.insert("excluded_directives");
            options.insert("additional_directives");
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

/// Validation error for configuration files.
///
/// Returned by [`LintConfig::validate_file`] when the TOML file contains
/// unrecognised keys or rule names. Each variant includes an optional
/// `suggestion` produced by fuzzy matching.
#[derive(Debug, Clone)]
pub enum ValidationError {
    /// An unrecognised top-level or section-level field (e.g. `[colour]` instead of `[color]`).
    UnknownField {
        /// Dotted path to the field (e.g. `"color.ui_mode"`).
        path: String,
        /// 1-indexed line number where the field appears, if found.
        line: Option<usize>,
        /// A similar known field name, if one is close enough.
        suggestion: Option<String>,
    },
    /// A `[rules.<name>]` section where `<name>` is not a known rule.
    UnknownRule {
        /// The unrecognised rule name.
        name: String,
        /// 1-indexed line number where the rule section appears, if found.
        line: Option<usize>,
        /// A similar known rule name, if one is close enough.
        suggestion: Option<String>,
    },
    /// An unrecognised option inside a rule section (e.g. `indent_sizee` in `[rules.indent]`).
    UnknownRuleOption {
        /// The rule name containing the unknown option.
        rule: String,
        /// The unrecognised option key.
        option: String,
        /// 1-indexed line number where the option appears, if found.
        line: Option<usize>,
        /// A similar known option name, if one is close enough.
        suggestion: Option<String>,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::UnknownField {
                path,
                line,
                suggestion,
            } => {
                if let Some(l) = line {
                    write!(f, "line {}: ", l)?;
                }
                write!(f, "unknown field '{}'", path)?;
                if let Some(s) = suggestion {
                    write!(f, ", did you mean '{}'?", s)?;
                }
                Ok(())
            }
            ValidationError::UnknownRule {
                name,
                line,
                suggestion,
            } => {
                if let Some(l) = line {
                    write!(f, "line {}: ", l)?;
                }
                write!(f, "unknown rule '{}'", name)?;
                if let Some(s) = suggestion {
                    write!(f, ", did you mean '{}'?", s)?;
                }
                Ok(())
            }
            ValidationError::UnknownRuleOption {
                rule,
                option,
                line,
                suggestion,
            } => {
                if let Some(l) = line {
                    write!(f, "line {}: ", l)?;
                }
                write!(f, "unknown option '{}' for rule '{}'", option, rule)?;
                if let Some(s) = suggestion {
                    write!(f, ", did you mean '{}'?", s)?;
                }
                Ok(())
            }
        }
    }
}
