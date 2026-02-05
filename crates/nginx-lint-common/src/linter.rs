use crate::parser::ast::Config;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "ERROR"),
            Severity::Warning => write!(f, "WARNING"),
            Severity::Info => write!(f, "INFO"),
        }
    }
}

/// Represents a fix that can be applied to resolve a lint error
#[derive(Debug, Clone, Serialize)]
pub struct Fix {
    /// Line number where the fix should be applied (1-indexed)
    pub line: usize,
    /// The original text to replace (if None and new_text is empty, delete the line)
    pub old_text: Option<String>,
    /// The new text to insert (empty string with old_text=None means delete)
    pub new_text: String,
    /// Whether to delete the entire line
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub delete_line: bool,
    /// Whether to insert new_text as a new line after the specified line
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub insert_after: bool,
    /// Start byte offset for range-based fix (0-indexed, inclusive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<usize>,
    /// End byte offset for range-based fix (0-indexed, exclusive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_offset: Option<usize>,
}

impl Fix {
    /// Create a fix that replaces text on a specific line
    pub fn replace(line: usize, old_text: &str, new_text: &str) -> Self {
        Self {
            line,
            old_text: Some(old_text.to_string()),
            new_text: new_text.to_string(),
            delete_line: false,
            insert_after: false,
            start_offset: None,
            end_offset: None,
        }
    }

    /// Create a fix that replaces an entire line
    pub fn replace_line(line: usize, new_text: &str) -> Self {
        Self {
            line,
            old_text: None,
            new_text: new_text.to_string(),
            delete_line: false,
            insert_after: false,
            start_offset: None,
            end_offset: None,
        }
    }

    /// Create a fix that deletes an entire line
    pub fn delete(line: usize) -> Self {
        Self {
            line,
            old_text: None,
            new_text: String::new(),
            delete_line: true,
            insert_after: false,
            start_offset: None,
            end_offset: None,
        }
    }

    /// Create a fix that inserts a new line after the specified line
    pub fn insert_after(line: usize, new_text: &str) -> Self {
        Self {
            line,
            old_text: None,
            new_text: new_text.to_string(),
            delete_line: false,
            insert_after: true,
            start_offset: None,
            end_offset: None,
        }
    }

    /// Create a range-based fix that replaces bytes from start to end offset
    ///
    /// This allows multiple fixes on the same line as long as their ranges don't overlap.
    pub fn replace_range(start_offset: usize, end_offset: usize, new_text: &str) -> Self {
        Self {
            line: 0, // Not used for range-based fixes
            old_text: None,
            new_text: new_text.to_string(),
            delete_line: false,
            insert_after: false,
            start_offset: Some(start_offset),
            end_offset: Some(end_offset),
        }
    }

    /// Check if this is a range-based fix
    pub fn is_range_based(&self) -> bool {
        self.start_offset.is_some() && self.end_offset.is_some()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LintError {
    pub rule: String,
    pub category: String,
    pub message: String,
    pub severity: Severity,
    pub line: Option<usize>,
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

impl LintError {
    pub fn new(rule: &str, category: &str, message: &str, severity: Severity) -> Self {
        Self {
            rule: rule.to_string(),
            category: category.to_string(),
            message: message.to_string(),
            severity,
            line: None,
            column: None,
            fix: None,
        }
    }

    pub fn with_location(mut self, line: usize, column: usize) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }
}

pub trait LintRule: Send + Sync {
    fn name(&self) -> &'static str;
    fn category(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn check(&self, config: &Config, path: &Path) -> Vec<LintError>;

    /// Check with pre-serialized config JSON (optimization for WASM plugins)
    ///
    /// This method allows passing a pre-serialized config JSON to avoid
    /// repeated serialization when running multiple plugins.
    /// Default implementation ignores the serialized config and calls check().
    fn check_with_serialized_config(
        &self,
        config: &Config,
        path: &Path,
        _serialized_config: &str,
    ) -> Vec<LintError> {
        self.check(config, path)
    }

    /// Get detailed explanation of why this rule exists
    fn why(&self) -> Option<&str> {
        None
    }

    /// Get example of bad configuration
    fn bad_example(&self) -> Option<&str> {
        None
    }

    /// Get example of good configuration
    fn good_example(&self) -> Option<&str> {
        None
    }

    /// Get reference URLs
    fn references(&self) -> Option<Vec<String>> {
        None
    }

    /// Get severity level (for plugins)
    fn severity(&self) -> Option<&str> {
        None
    }
}

/// Basic Linter that holds rules and runs them
pub struct Linter {
    rules: Vec<Box<dyn LintRule>>,
}

impl Linter {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_rule(&mut self, rule: Box<dyn LintRule>) {
        self.rules.push(rule);
    }

    /// Remove rules that match the predicate
    pub fn remove_rules_by_name<F>(&mut self, should_remove: F)
    where
        F: Fn(&str) -> bool,
    {
        self.rules.retain(|rule| !should_remove(rule.name()));
    }

    /// Get a reference to all rules
    pub fn rules(&self) -> &[Box<dyn LintRule>] {
        &self.rules
    }

    /// Run all lint rules and collect errors (sequential version)
    pub fn lint(&self, config: &Config, path: &Path) -> Vec<LintError> {
        // Pre-serialize config once for all rules (optimization for WASM plugins)
        let serialized_config = serde_json::to_string(config).unwrap_or_default();

        self.rules
            .iter()
            .flat_map(|rule| rule.check_with_serialized_config(config, path, &serialized_config))
            .collect()
    }
}

impl Default for Linter {
    fn default() -> Self {
        Self::new()
    }
}
