//! Core types for the lint engine: rule definitions, error reporting, and fix proposals.
//!
//! This module contains the fundamental abstractions used by both native Rust
//! rules (in `src/rules/`) and WASM plugin rules:
//!
//! - [`LintRule`] — trait that every rule implements
//! - [`LintError`] — a single diagnostic produced by a rule
//! - [`Severity`] — error vs. warning classification
//! - [`Fix`] — an auto-fix action attached to a diagnostic
//! - [`Linter`] — collects rules and runs them against a parsed config

use crate::parser::ast::Config;
use serde::Serialize;
use std::path::Path;

/// Display-ordered list of rule categories for UI output.
///
/// Used by the CLI and documentation generator to group rules consistently.
pub const RULE_CATEGORIES: &[&str] = &[
    "style",
    "syntax",
    "security",
    "best-practices",
    "deprecation",
];

/// Severity level of a lint diagnostic.
///
/// # Variants
///
/// - `Error` — the configuration is broken or has a critical security issue.
/// - `Warning` — the configuration works but uses discouraged settings or could be improved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Severity {
    /// The configuration will not work correctly, or there is a critical security issue.
    Error,
    /// A discouraged setting, potential problem, or improvement suggestion.
    Warning,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "ERROR"),
            Severity::Warning => write!(f, "WARNING"),
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

/// A single lint diagnostic produced by a rule.
///
/// Every [`LintRule::check`] call returns a `Vec<LintError>`. Each error
/// carries the rule name, category, a human-readable message, severity, an
/// optional source location, and zero or more [`Fix`] proposals.
///
/// # Building errors
///
/// ```
/// use nginx_lint_common::linter::{LintError, Severity, Fix};
///
/// let error = LintError::new("my-rule", "style", "trailing whitespace", Severity::Warning)
///     .with_location(10, 1)
///     .with_fix(Fix::replace(10, "value  ", "value"));
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct LintError {
    /// Rule identifier (e.g. `"server-tokens-enabled"`).
    pub rule: String,
    /// Category the rule belongs to (e.g. `"security"`, `"style"`).
    pub category: String,
    /// Human-readable description of the problem.
    pub message: String,
    /// Whether this is an error or a warning.
    pub severity: Severity,
    /// 1-indexed line number where the problem was detected.
    pub line: Option<usize>,
    /// 1-indexed column number where the problem was detected.
    pub column: Option<usize>,
    /// Auto-fix proposals that can resolve this diagnostic.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<Fix>,
}

impl LintError {
    /// Create a new lint error without a source location.
    ///
    /// Use [`with_location`](Self::with_location) to attach line/column info
    /// and [`with_fix`](Self::with_fix) to attach auto-fix proposals.
    pub fn new(rule: &str, category: &str, message: &str, severity: Severity) -> Self {
        Self {
            rule: rule.to_string(),
            category: category.to_string(),
            message: message.to_string(),
            severity,
            line: None,
            column: None,
            fixes: Vec::new(),
        }
    }

    /// Attach a source location (1-indexed line and column) to this error.
    pub fn with_location(mut self, line: usize, column: usize) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    /// Append a single [`Fix`] proposal to this error.
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fixes.push(fix);
        self
    }

    /// Append multiple [`Fix`] proposals to this error.
    pub fn with_fixes(mut self, fixes: Vec<Fix>) -> Self {
        self.fixes.extend(fixes);
        self
    }
}

/// A lint rule that can be checked against a parsed nginx configuration.
///
/// Every rule — whether implemented as a native Rust struct or as a WASM
/// plugin — implements this trait. The four required methods supply metadata
/// and the check logic; the optional methods provide documentation and
/// plugin-specific overrides.
///
/// # Required methods
///
/// | Method | Purpose |
/// |--------|---------|
/// | [`name`](Self::name) | Unique rule identifier (e.g. `"server-tokens-enabled"`) |
/// | [`category`](Self::category) | Category for grouping (e.g. `"security"`) |
/// | [`description`](Self::description) | One-line human-readable summary |
/// | [`check`](Self::check) | Run the rule and return diagnostics |
pub trait LintRule: Send + Sync {
    /// Unique identifier for this rule (e.g. `"server-tokens-enabled"`).
    fn name(&self) -> &'static str;
    /// Category this rule belongs to (e.g. `"security"`, `"style"`).
    fn category(&self) -> &'static str;
    /// One-line human-readable description of what this rule checks.
    fn description(&self) -> &'static str;
    /// Run the rule against `config` (parsed from `path`) and return diagnostics.
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

/// Container that holds [`LintRule`]s and runs them against a parsed config.
///
/// Create a `Linter`, register rules with [`add_rule`](Self::add_rule), then
/// call [`lint`](Self::lint) to collect all diagnostics.
pub struct Linter {
    rules: Vec<Box<dyn LintRule>>,
}

impl Linter {
    /// Create an empty linter with no rules registered.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Register a lint rule. Rules are executed in registration order.
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
