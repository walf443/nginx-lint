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
    #[deprecated(note = "Use Fix::replace_range() for offset-based fixes instead")]
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
    #[deprecated(note = "Use Fix::replace_range() for offset-based fixes instead")]
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
    #[deprecated(note = "Use Fix::replace_range() for offset-based fixes instead")]
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
    #[deprecated(note = "Use Fix::replace_range() for offset-based fixes instead")]
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

/// Compute the byte offset of the start of each line (1-indexed).
///
/// Returns a vector where `line_starts[0]` is always `0` (start of line 1),
/// `line_starts[1]` is the byte offset of line 2, etc.
/// An extra entry at the end equals `content.len()` for convenience.
pub fn compute_line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts.push(content.len());
    starts
}

/// Convert a line-based [`Fix`] into an offset-based one using precomputed line starts.
///
/// Line-based fixes (created via deprecated `Fix::replace`, `Fix::delete`, etc.) are
/// normalized to `Fix::replace_range` using the provided `line_starts` offsets.
///
/// Returns `None` if the fix references an out-of-range line or the `old_text` is not found.
pub fn normalize_line_fix(fix: &Fix, content: &str, line_starts: &[usize]) -> Option<Fix> {
    if fix.line == 0 {
        return None;
    }

    let num_lines = line_starts.len() - 1; // last entry is content.len()

    if fix.delete_line {
        if fix.line > num_lines {
            return None;
        }
        let start = line_starts[fix.line - 1];
        let end = if fix.line < num_lines {
            line_starts[fix.line] // includes the trailing \n
        } else {
            // Last line: also remove the preceding \n if there is one
            let end = line_starts[fix.line]; // == content.len()
            if start > 0 && content.as_bytes().get(start - 1) == Some(&b'\n') {
                return Some(Fix::replace_range(start - 1, end, ""));
            }
            end
        };
        return Some(Fix::replace_range(start, end, ""));
    }

    if fix.insert_after {
        if fix.line > num_lines {
            return None;
        }
        // Insert point: right after the \n at end of the target line
        let insert_offset = if fix.line < num_lines {
            line_starts[fix.line]
        } else {
            content.len()
        };
        let new_text = if insert_offset == content.len() && !content.ends_with('\n') {
            format!("\n{}", fix.new_text)
        } else {
            format!("{}\n", fix.new_text)
        };
        return Some(Fix::replace_range(insert_offset, insert_offset, &new_text));
    }

    if fix.line > num_lines {
        return None;
    }

    let line_start = line_starts[fix.line - 1];
    let line_end_with_newline = line_starts[fix.line];
    // Line content without trailing newline
    let line_end = if line_end_with_newline > line_start
        && content.as_bytes().get(line_end_with_newline - 1) == Some(&b'\n')
    {
        line_end_with_newline - 1
    } else {
        line_end_with_newline
    };

    if let Some(ref old_text) = fix.old_text {
        // Replace first occurrence of old_text within the line
        let line_content = &content[line_start..line_end];
        if let Some(pos) = line_content.find(old_text.as_str()) {
            let start = line_start + pos;
            let end = start + old_text.len();
            return Some(Fix::replace_range(start, end, &fix.new_text));
        }
        return None;
    }

    // Replace entire line content (not including newline)
    Some(Fix::replace_range(line_start, line_end, &fix.new_text))
}

/// Apply fixes to content string.
///
/// All fixes (both line-based and offset-based) are normalized to offset-based,
/// then applied in reverse order to avoid index shifts. Overlapping fixes are skipped.
///
/// Returns `(modified_content, number_of_fixes_applied)`.
pub fn apply_fixes_to_content(content: &str, fixes: &[&Fix]) -> (String, usize) {
    let line_starts = compute_line_starts(content);

    // Normalize all fixes to range-based
    let mut range_fixes: Vec<Fix> = Vec::with_capacity(fixes.len());
    for fix in fixes {
        if fix.is_range_based() {
            range_fixes.push((*fix).clone());
        } else if let Some(normalized) = normalize_line_fix(fix, content, &line_starts) {
            range_fixes.push(normalized);
        }
    }

    // Sort by start_offset descending to avoid index shifts.
    // For same-offset insertions (start == end), sort by indent ascending so that
    // the more-indented text is processed last and ends up first in the file.
    range_fixes.sort_by(|a, b| {
        let a_start = a.start_offset.unwrap();
        let b_start = b.start_offset.unwrap();
        match b_start.cmp(&a_start) {
            std::cmp::Ordering::Equal => {
                let a_is_insert = a.end_offset.unwrap() == a_start;
                let b_is_insert = b.end_offset.unwrap() == b_start;
                if a_is_insert && b_is_insert {
                    // For insertions at the same point: ascending indent order
                    // so more-indented text is processed last (appears first in output)
                    let a_indent = a.new_text.len() - a.new_text.trim_start().len();
                    let b_indent = b.new_text.len() - b.new_text.trim_start().len();
                    a_indent.cmp(&b_indent)
                } else {
                    std::cmp::Ordering::Equal
                }
            }
            other => other,
        }
    });

    let mut fix_count = 0;
    let mut result = content.to_string();
    let mut applied_ranges: Vec<(usize, usize)> = Vec::new();

    for fix in &range_fixes {
        let start = fix.start_offset.unwrap();
        let end = fix.end_offset.unwrap();

        // Check if this range overlaps with any already applied range
        let overlaps = applied_ranges.iter().any(|(s, e)| start < *e && end > *s);
        if overlaps {
            continue;
        }

        if start <= result.len() && end <= result.len() && start <= end {
            result.replace_range(start..end, &fix.new_text);
            applied_ranges.push((start, start + fix.new_text.len()));
            fix_count += 1;
        }
    }

    // Ensure trailing newline
    if !result.ends_with('\n') {
        result.push('\n');
    }

    (result, fix_count)
}
