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
    #[deprecated(
        since = "0.16.0",
        note = "no longer called by the linter; the serialized config was only used by \
                legacy core-module plugins. Implement check() or check_shared() instead."
    )]
    fn check_with_serialized_config(
        &self,
        config: &Config,
        path: &Path,
        _serialized_config: &str,
    ) -> Vec<LintError> {
        self.check(config, path)
    }

    /// Whether this rule wants the config as a shared `Arc` handle.
    ///
    /// Rules that hand the config to another owner (e.g. WASM plugin rules,
    /// which store it in the sandbox's resource table) should return `true`
    /// so the linter shares one `Arc<Config>` across all such rules instead
    /// of each rule deep-cloning the AST per check.
    fn wants_shared_config(&self) -> bool {
        false
    }

    /// Run the rule with a shared config handle.
    ///
    /// The linter calls this instead of [`check`](Self::check) when
    /// [`wants_shared_config`](Self::wants_shared_config) returns `true`.
    /// Default implementation borrows the config and calls `check()`.
    fn check_shared(&self, config: &std::sync::Arc<Config>, path: &Path) -> Vec<LintError> {
        self.check(config, path)
    }

    /// Whether this rule wants the raw file content directly.
    ///
    /// Rules that need to re-derive diagnostics from the source text itself
    /// (rather than the parsed `Config`) should return `true` so the linter
    /// hands them the content it already has in memory, instead of each rule
    /// independently re-reading the file from disk and re-parsing it.
    ///
    /// Note: this does not compose with [`wants_shared_config`](Self::wants_shared_config) —
    /// the default [`check_with_content`](Self::check_with_content) delegates to
    /// [`check`](Self::check), not [`check_shared`](Self::check_shared). No current
    /// rule needs both; a future one that does would need a custom override.
    fn wants_content(&self) -> bool {
        false
    }

    /// Run the rule with the raw file content already available.
    ///
    /// The linter calls this instead of [`check`](Self::check)/[`check_shared`](Self::check_shared)
    /// when [`wants_content`](Self::wants_content) returns `true` and content is available.
    /// Default implementation ignores `content` and calls `check()`.
    fn check_with_content(&self, config: &Config, path: &Path, _content: &str) -> Vec<LintError> {
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

    /// Minimum nginx version this rule applies to (inclusive).
    ///
    /// `None` means the rule applies regardless of how old the nginx version is.
    /// Used by the linter's version-based rule filter to decide whether to
    /// run this rule against a config whose
    /// [`target_nginx_version`](crate::config::LintConfig::target_nginx_version)
    /// is set.
    fn min_nginx_version(&self) -> Option<&str> {
        None
    }

    /// Maximum nginx version this rule applies to (inclusive).
    ///
    /// `None` means the rule applies regardless of how new the nginx version is.
    fn max_nginx_version(&self) -> Option<&str> {
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
        let shared_config = std::sync::OnceLock::new();

        self.rules
            .iter()
            .flat_map(|rule| run_rule(rule.as_ref(), config, path, &shared_config))
            .collect()
    }
}

/// Run a single rule, dispatching to [`LintRule::check_shared`] with one
/// lazily-created `Arc<Config>` for rules that
/// [want a shared handle](LintRule::wants_shared_config), and to
/// [`LintRule::check`] otherwise.
///
/// The `Arc` is created at most once per `shared_config` cell (i.e. per
/// linted file), so purely native rule sets never pay for the clone. Linter
/// implementations should route every rule invocation through this function
/// so the dispatch policy stays in one place.
pub fn run_rule(
    rule: &dyn LintRule,
    config: &Config,
    path: &Path,
    shared_config: &std::sync::OnceLock<std::sync::Arc<Config>>,
) -> Vec<LintError> {
    if rule.wants_shared_config() {
        let shared = shared_config.get_or_init(|| std::sync::Arc::new(config.clone()));
        rule.check_shared(shared, path)
    } else {
        rule.check(config, path)
    }
}

/// Like [`run_rule`], but additionally dispatches to
/// [`LintRule::check_with_content`] for rules that
/// [want raw content](LintRule::wants_content), so those rules don't have to
/// re-read the file from disk when the caller already has it in memory.
/// Falls back to [`run_rule`]'s dispatch policy otherwise.
pub fn run_rule_with_content(
    rule: &dyn LintRule,
    config: &Config,
    path: &Path,
    content: &str,
    shared_config: &std::sync::OnceLock<std::sync::Arc<Config>>,
) -> Vec<LintError> {
    if rule.wants_content() {
        rule.check_with_content(config, path, content)
    } else {
        run_rule(rule, config, path, shared_config)
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

/// Result of applying fixes to content, with detailed counts.
#[derive(Debug, Clone)]
pub struct FixApplyResult {
    /// Content after applying the fixes
    pub content: String,
    /// Number of fixes applied
    pub applied: usize,
    /// Number of fixes skipped because they could not be applied: offsets out
    /// of range or not on UTF-8 character boundaries, or a line-based fix
    /// referencing a missing line or `old_text` (e.g. produced by a buggy
    /// plugin). Does not include fixes skipped due to overlap with an applied
    /// fix.
    pub skipped_invalid: usize,
}

/// Apply fixes to content string.
///
/// Convenience wrapper around [`apply_fixes_to_content_detailed`] for callers
/// that do not need the skipped-fix count.
///
/// Returns `(modified_content, number_of_fixes_applied)`.
pub fn apply_fixes_to_content(content: &str, fixes: &[&Fix]) -> (String, usize) {
    let result = apply_fixes_to_content_detailed(content, fixes);
    (result.content, result.applied)
}

/// Whether `s` is non-empty and consists entirely of whitespace — used to
/// tell a pure reformatting insert (e.g. `indent`'s fixes) apart from one
/// that inserts real content (e.g. a missing closing brace).
fn is_whitespace_only(s: &str) -> bool {
    !s.is_empty() && s.chars().all(char::is_whitespace)
}

/// Apply fixes to content string, reporting skipped fixes.
///
/// All fixes (both line-based and offset-based) are normalized to offset-based,
/// then applied in reverse order to avoid index shifts. Overlapping fixes are skipped.
/// Fixes that cannot be applied (invalid offsets, or line-based fixes that fail
/// normalization) are skipped and counted in [`FixApplyResult::skipped_invalid`].
pub fn apply_fixes_to_content_detailed(content: &str, fixes: &[&Fix]) -> FixApplyResult {
    let line_starts = compute_line_starts(content);
    let mut skipped_invalid = 0;

    // Normalize all fixes to range-based
    let mut range_fixes: Vec<Fix> = Vec::with_capacity(fixes.len());
    for fix in fixes {
        if fix.is_range_based() {
            range_fixes.push((*fix).clone());
        } else if let Some(normalized) = normalize_line_fix(fix, content, &line_starts) {
            range_fixes.push(normalized);
        } else {
            skipped_invalid += 1;
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
    // (start, end, was this insert's new_text whitespace-only)
    let mut applied_ranges: Vec<(usize, usize, bool)> = Vec::new();

    for fix in &range_fixes {
        let start = fix.start_offset.unwrap();
        let end = fix.end_offset.unwrap();
        let is_insert = start == end;

        // Check if this range overlaps with any already applied range
        let overlaps = applied_ranges
            .iter()
            .any(|(s, e, _)| start < *e && end > *s);

        // Two zero-width inserts at the identical point don't trip the
        // check above (touching, not overlapping) — which is intentional
        // when both are pure whitespace (e.g. two `indent` fixes for the
        // same line combine into the right total indentation). But
        // stacking a whitespace-only reformatting insert next to one that
        // inserts real content (e.g. `unmatched-braces` inserting a missing
        // `}`) produces nonsensical interleaved output — the whitespace fix
        // was computed against a structure this other fix is about to
        // change anyway, so drop it.
        let conflicts_with_structural_insert = is_insert
            && is_whitespace_only(&fix.new_text)
            && applied_ranges.iter().any(|(s, _, ws)| *s == start && !*ws);

        if overlaps || conflicts_with_structural_insert {
            continue;
        }

        // Offsets must lie on UTF-8 char boundaries: replace_range panics
        // otherwise, and plugin-provided fixes are untrusted input.
        if start <= end
            && end <= result.len()
            && result.is_char_boundary(start)
            && result.is_char_boundary(end)
        {
            result.replace_range(start..end, &fix.new_text);
            applied_ranges.push((
                start,
                start + fix.new_text.len(),
                is_insert && is_whitespace_only(&fix.new_text),
            ));
            fix_count += 1;
        } else {
            skipped_invalid += 1;
        }
    }

    // Ensure trailing newline
    if !result.ends_with('\n') {
        result.push('\n');
    }

    FixApplyResult {
        content: result,
        applied: fix_count,
        skipped_invalid,
    }
}

#[cfg(test)]
mod fix_tests {
    use super::*;

    #[test]
    fn test_compute_line_starts() {
        let starts = compute_line_starts("abc\ndef\nghi");
        // line 1 starts at 0, line 2 at 4, line 3 at 8, sentinel at 11
        assert_eq!(starts, vec![0, 4, 8, 11]);
    }

    #[test]
    fn test_compute_line_starts_trailing_newline() {
        let starts = compute_line_starts("abc\n");
        // line 1 at 0, line 2 at 4 (empty), sentinel at 4
        assert_eq!(starts, vec![0, 4, 4]);
    }

    #[test]
    #[allow(deprecated)]
    fn test_normalize_replace() {
        let content = "listen 80;\nserver_name example.com;\n";
        let line_starts = compute_line_starts(content);
        let fix = Fix::replace(1, "80", "8080");
        let normalized = normalize_line_fix(&fix, content, &line_starts).unwrap();
        assert!(normalized.is_range_based());
        assert_eq!(normalized.start_offset, Some(7));
        assert_eq!(normalized.end_offset, Some(9));
        assert_eq!(normalized.new_text, "8080");
    }

    #[test]
    #[allow(deprecated)]
    fn test_normalize_delete() {
        let content = "line1\nline2\nline3\n";
        let line_starts = compute_line_starts(content);
        let fix = Fix::delete(2);
        let normalized = normalize_line_fix(&fix, content, &line_starts).unwrap();
        assert!(normalized.is_range_based());
        // Should delete "line2\n" (offset 6..12)
        assert_eq!(normalized.start_offset, Some(6));
        assert_eq!(normalized.end_offset, Some(12));
    }

    #[test]
    #[allow(deprecated)]
    fn test_normalize_insert_after() {
        let content = "line1\nline2\n";
        let line_starts = compute_line_starts(content);
        let fix = Fix::insert_after(1, "inserted");
        let normalized = normalize_line_fix(&fix, content, &line_starts).unwrap();
        assert!(normalized.is_range_based());
        // Insert at offset 6 (start of line 2)
        assert_eq!(normalized.start_offset, Some(6));
        assert_eq!(normalized.end_offset, Some(6));
        assert_eq!(normalized.new_text, "inserted\n");
    }

    #[test]
    #[allow(deprecated)]
    fn test_normalize_out_of_range() {
        let content = "line1\n";
        let line_starts = compute_line_starts(content);
        let fix = Fix::delete(99);
        assert!(normalize_line_fix(&fix, content, &line_starts).is_none());
    }

    #[test]
    #[allow(deprecated)]
    fn test_normalize_replace_not_found() {
        let content = "listen 80;\n";
        let line_starts = compute_line_starts(content);
        let fix = Fix::replace(1, "nonexistent", "new");
        assert!(normalize_line_fix(&fix, content, &line_starts).is_none());
    }

    #[test]
    fn test_apply_range_fix() {
        let content = "listen 80;\n";
        let fix = Fix::replace_range(7, 9, "8080");
        let fixes: Vec<&Fix> = vec![&fix];
        let (result, count) = apply_fixes_to_content(content, &fixes);
        assert_eq!(result, "listen 8080;\n");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_apply_multiple_fixes_same_line() {
        // Two fixes on the same line should both apply
        let content = "proxy_set_header Host $host;\n";
        let fix1 = Fix::replace_range(17, 21, "X-Real-IP");
        let fix2 = Fix::replace_range(22, 27, "$remote_addr");
        let fixes: Vec<&Fix> = vec![&fix1, &fix2];
        let (result, count) = apply_fixes_to_content(content, &fixes);
        assert_eq!(result, "proxy_set_header X-Real-IP $remote_addr;\n");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_apply_overlapping_fixes_skips() {
        let content = "abcdef\n";
        let fix1 = Fix::replace_range(0, 3, "XYZ"); // replace "abc"
        let fix2 = Fix::replace_range(2, 5, "QQQ"); // overlaps with fix1
        let fixes: Vec<&Fix> = vec![&fix1, &fix2];
        let (_, count) = apply_fixes_to_content(content, &fixes);
        // Only one fix should apply (the other is skipped due to overlap)
        assert_eq!(count, 1);
    }

    #[test]
    fn test_apply_fix_non_char_boundary_skipped() {
        // "あ" is 3 bytes (0..3); offsets 1 and 2 are not char boundaries.
        // Such fixes (e.g. from a malicious plugin) must be skipped, not panic.
        let content = "あいう;\n";
        let fix = Fix::replace_range(1, 2, "x");
        let fixes: Vec<&Fix> = vec![&fix];
        let (result, count) = apply_fixes_to_content(content, &fixes);
        assert_eq!(result, content);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_apply_fix_non_char_boundary_end_skipped() {
        // start is on a boundary but end is mid-character
        let content = "あいう;\n";
        let fix = Fix::replace_range(0, 4, "x");
        let fixes: Vec<&Fix> = vec![&fix];
        let (result, count) = apply_fixes_to_content(content, &fixes);
        assert_eq!(result, content);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_apply_fix_multibyte_on_boundary_applies() {
        // Offsets on char boundaries within multibyte content still work
        let content = "あいう;\n";
        let fix = Fix::replace_range(3, 6, "x");
        let fixes: Vec<&Fix> = vec![&fix];
        let (result, count) = apply_fixes_to_content(content, &fixes);
        assert_eq!(result, "あxう;\n");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_detailed_counts_invalid_fixes() {
        // One valid fix, one non-boundary fix, one out-of-range fix
        let content = "あいう;\n";
        let valid = Fix::replace_range(3, 6, "x");
        let non_boundary = Fix::replace_range(1, 2, "y");
        let out_of_range = Fix::replace_range(100, 200, "z");
        let fixes: Vec<&Fix> = vec![&valid, &non_boundary, &out_of_range];
        let result = apply_fixes_to_content_detailed(content, &fixes);
        assert_eq!(result.content, "あxう;\n");
        assert_eq!(result.applied, 1);
        assert_eq!(result.skipped_invalid, 2);
    }

    #[test]
    #[allow(deprecated)]
    fn test_detailed_counts_failed_line_normalization() {
        let content = "listen 80;\n";
        let missing_old_text = Fix::replace(1, "nonexistent", "x");
        let out_of_range_line = Fix::replace(99, "listen", "x");
        let fixes: Vec<&Fix> = vec![&missing_old_text, &out_of_range_line];
        let result = apply_fixes_to_content_detailed(content, &fixes);
        assert_eq!(result.content, content);
        assert_eq!(result.applied, 0);
        assert_eq!(result.skipped_invalid, 2);
    }

    #[test]
    fn test_detailed_overlap_not_counted_as_invalid() {
        let content = "abcdef\n";
        let fix1 = Fix::replace_range(0, 3, "XYZ");
        let fix2 = Fix::replace_range(2, 5, "QQQ"); // overlaps with fix1
        let fixes: Vec<&Fix> = vec![&fix1, &fix2];
        let result = apply_fixes_to_content_detailed(content, &fixes);
        assert_eq!(result.applied, 1);
        assert_eq!(result.skipped_invalid, 0);
    }

    /// Two whitespace-only inserts at the exact same point (e.g. two
    /// `indent` errors reconciling to the same total indentation) must
    /// still stack in ascending-indent order — this is the legitimate use
    /// of same-point insertion the conflict check below must not break.
    #[test]
    fn test_same_point_whitespace_only_inserts_stack() {
        let content = "#note\n";
        let four_spaces = Fix::replace_range(0, 0, "    ");
        let two_spaces = Fix::replace_range(0, 0, "  ");
        let fixes: Vec<&Fix> = vec![&four_spaces, &two_spaces];
        let result = apply_fixes_to_content_detailed(content, &fixes);
        assert_eq!(result.content, "      #note\n");
        assert_eq!(
            result.applied, 2,
            "both whitespace-only inserts should apply"
        );
    }

    /// Regression test for https://github.com/walf443/nginx-lint/issues/296.
    ///
    /// A structural insert (e.g. `unmatched-braces` inserting a missing
    /// `}`) and a whitespace-only reformatting insert (e.g. `indent`
    /// reformatting the very line the brace is being inserted before) at
    /// the exact same point don't trip the ordinary range-overlap check
    /// (both are zero-width, touching but not overlapping) — without a
    /// dedicated conflict check they get concatenated in whatever order the
    /// sort happens to produce, yielding nonsensical interleaved output
    /// (e.g. `      }` — 6 spaces of indentation matching neither fix's own
    /// intent). The whitespace-only fix must be dropped instead, since it
    /// was computed against content this other fix is about to change
    /// immediately adjacent to it anyway.
    #[test]
    fn test_whitespace_only_insert_skipped_when_it_conflicts_with_structural_insert() {
        let content = "# Missing closing brace for http\n";
        let close_brace = Fix::replace_range(0, 0, "}\n");
        let reindent = Fix::replace_range(0, 0, "      ");
        let fixes: Vec<&Fix> = vec![&close_brace, &reindent];
        let result = apply_fixes_to_content_detailed(content, &fixes);
        assert_eq!(
            result.content, "}\n# Missing closing brace for http\n",
            "the whitespace-only fix must be dropped, not interleaved with the brace"
        );
        assert_eq!(result.applied, 1);
    }

    #[test]
    #[allow(deprecated)]
    fn test_apply_deprecated_fix_via_normalization() {
        let content = "listen 80;\nserver_name old;\n";
        let fix = Fix::replace(2, "old", "new");
        let fixes: Vec<&Fix> = vec![&fix];
        let (result, count) = apply_fixes_to_content(content, &fixes);
        assert_eq!(result, "listen 80;\nserver_name new;\n");
        assert_eq!(count, 1);
    }
}
