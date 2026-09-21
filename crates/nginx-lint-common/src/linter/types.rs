//! What a rule reports: a diagnostic, its severity, and the fixes it
//! proposes.

use serde::{Deserialize, Serialize};

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
///
/// Deserialize lets a fix cross a language boundary and come back as the
/// same struct the applier consumes — the Python SDK sends fixes here to
/// test what they produce. The fields that are skipped when serializing
/// default when absent, so the round-trip is lossless.
///
/// `deny_unknown_fields` keeps that guarantee honest: were the WIT `fix`
/// record's fields ever renamed, silently dropping the unknown key would
/// default e.g. `insert_after` to false and turn an insert into a
/// whole-line replacement — the exact failure this round-trip exists to
/// catch. Rejecting the input surfaces the drift instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fix {
    /// Line number where the fix should be applied (1-indexed)
    pub line: usize,
    /// The original text to replace (if None and new_text is empty, delete the line)
    #[serde(default)]
    pub old_text: Option<String>,
    /// The new text to insert (empty string with old_text=None means delete)
    pub new_text: String,
    /// Whether to delete the entire line
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub delete_line: bool,
    /// Whether to insert new_text as a new line after the specified line
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub insert_after: bool,
    /// Start byte offset for range-based fix (0-indexed, inclusive)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<usize>,
    /// End byte offset for range-based fix (0-indexed, exclusive)
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
/// Every [`LintRule::check`](super::LintRule::check) call returns a `Vec<LintError>`. Each error
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
