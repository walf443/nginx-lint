//! Ignore comment support for nginx-lint
//!
//! This module provides support for inline disable comments that ignore
//! lint errors on specific lines.
//!
//! # Comment Formats
//!
//! ## Comment-only line (targets next line)
//! ```nginx
//! # nginx-lint:disable rule-name reason
//! server_tokens on;
//! ```
//!
//! ## Inline comment (targets current line)
//! ```nginx
//! server_tokens on; # nginx-lint:disable rule-name reason
//! ```
//!
//! - `rule-name`: Required. The name of the rule to ignore
//! - `reason`: Required. A reason explaining why the rule is ignored

use std::collections::{HashMap, HashSet};

use crate::linter::{LintError, Severity};

/// A warning generated from parsing ignore comments
#[derive(Debug, Clone)]
pub struct IgnoreWarning {
    /// Line number where the warning occurred
    pub line: usize,
    /// Warning message
    pub message: String,
}

/// Tracks ignored rules per line
#[derive(Debug, Default)]
pub struct IgnoreTracker {
    /// Map from line number to set of ignored rule names
    ignored_lines: HashMap<usize, HashSet<String>>,
}

impl IgnoreTracker {
    /// Create a new empty ignore tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a rule is ignored on a specific line
    pub fn is_ignored(&self, rule: &str, line: usize) -> bool {
        self.ignored_lines
            .get(&line)
            .map(|rules| rules.contains(rule))
            .unwrap_or(false)
    }

    /// Build an ignore tracker from content, returning any warnings
    pub fn from_content(content: &str) -> (Self, Vec<IgnoreWarning>) {
        let mut tracker = Self::new();
        let mut warnings = Vec::new();

        for (line_number, line) in content.lines().enumerate() {
            let line_number = line_number + 1; // Convert to 1-indexed

            if let Some(result) = parse_disable_comment(line, line_number) {
                match result {
                    Ok((rule_name, target_line)) => {
                        tracker
                            .ignored_lines
                            .entry(target_line)
                            .or_default()
                            .insert(rule_name);
                    }
                    Err(warning) => {
                        warnings.push(warning);
                    }
                }
            }
        }

        (tracker, warnings)
    }

    /// Add an ignore rule for a specific line
    #[cfg(test)]
    pub fn add_ignore(&mut self, rule: &str, line: usize) {
        self.ignored_lines
            .entry(line)
            .or_default()
            .insert(rule.to_string());
    }
}

/// Parse a disable comment from a line
///
/// Supports two formats:
/// 1. Comment-only line: `# nginx-lint:disable rule-name reason` → targets next line
/// 2. Inline comment: `directive; # nginx-lint:disable rule-name reason` → targets current line
///
/// Returns:
/// - `None` if the line does not contain a disable comment
/// - `Some(Ok((rule_name, target_line)))` if valid
/// - `Some(Err(warning))` if the comment is malformed
fn parse_disable_comment(
    line: &str,
    line_number: usize,
) -> Option<Result<(String, usize), IgnoreWarning>> {
    const DISABLE_PREFIX: &str = "nginx-lint:disable";

    // Find the comment marker
    let comment_start = line.find('#')?;
    let comment_part = &line[comment_start..];
    let comment = comment_part.trim_start_matches('#').trim();

    // Check for nginx-lint:disable prefix
    let rest = comment.strip_prefix(DISABLE_PREFIX)?;
    let rest = rest.trim();

    // Determine if this is a comment-only line or inline comment
    let before_comment = line[..comment_start].trim();
    let is_inline = !before_comment.is_empty();

    // Parse rule name and reason
    let parts: Vec<&str> = rest.splitn(2, |c: char| c.is_whitespace()).collect();

    // Check for missing rule name
    if parts.is_empty() || parts[0].is_empty() {
        return Some(Err(IgnoreWarning {
            line: line_number,
            message: "nginx-lint:disable requires a rule name".to_string(),
        }));
    }

    let rule_name = parts[0].to_string();

    // Check for missing reason
    if parts.len() < 2 || parts[1].trim().is_empty() {
        return Some(Err(IgnoreWarning {
            line: line_number,
            message: format!(
                "nginx-lint:disable {} requires a reason",
                rule_name
            ),
        }));
    }

    // Inline comment targets current line, comment-only line targets next line
    let target_line = if is_inline {
        line_number
    } else {
        line_number + 1
    };

    Some(Ok((rule_name, target_line)))
}

/// Result of filtering errors with ignore tracker
#[derive(Debug)]
pub struct FilterResult {
    /// Errors that were not ignored
    pub errors: Vec<LintError>,
    /// Number of errors that were ignored
    pub ignored_count: usize,
}

/// Filter errors using an ignore tracker, returning remaining errors and ignored count
pub fn filter_errors(errors: Vec<LintError>, tracker: &IgnoreTracker) -> FilterResult {
    let mut remaining = Vec::new();
    let mut ignored_count = 0;

    for error in errors {
        if let Some(line) = error.line
            && tracker.is_ignored(&error.rule, line)
        {
            ignored_count += 1;
            continue;
        }
        remaining.push(error);
    }

    FilterResult {
        errors: remaining,
        ignored_count,
    }
}

/// Convert ignore warnings to lint errors
pub fn warnings_to_errors(warnings: Vec<IgnoreWarning>) -> Vec<LintError> {
    warnings
        .into_iter()
        .map(|warning| {
            LintError::new(
                "ignore-comment",
                "ignore",
                &warning.message,
                Severity::Warning,
            )
            .with_location(warning.line, 1)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_disable_comment() {
        let result = parse_disable_comment(
            "# nginx-lint:disable server-tokens-enabled for dev environment",
            5,
        );
        assert!(result.is_some());
        let (rule, line) = result.unwrap().unwrap();
        assert_eq!(rule, "server-tokens-enabled");
        assert_eq!(line, 6); // Next line
    }

    #[test]
    fn test_parse_disable_comment_with_japanese_reason() {
        let result = parse_disable_comment(
            "# nginx-lint:disable server-tokens-enabled 開発環境用",
            5,
        );
        assert!(result.is_some());
        let (rule, line) = result.unwrap().unwrap();
        assert_eq!(rule, "server-tokens-enabled");
        assert_eq!(line, 6);
    }

    #[test]
    fn test_parse_missing_rule_name() {
        let result = parse_disable_comment("# nginx-lint:disable", 5);
        assert!(result.is_some());
        let warning = result.unwrap().unwrap_err();
        assert_eq!(warning.line, 5);
        assert!(warning
            .message
            .contains("nginx-lint:disable requires a rule name"));
    }

    #[test]
    fn test_parse_missing_reason() {
        let result = parse_disable_comment("# nginx-lint:disable server-tokens-enabled", 5);
        assert!(result.is_some());
        let warning = result.unwrap().unwrap_err();
        assert_eq!(warning.line, 5);
        assert!(warning
            .message
            .contains("nginx-lint:disable server-tokens-enabled requires a reason"));
    }

    #[test]
    fn test_parse_not_a_comment() {
        let result = parse_disable_comment("server_tokens on;", 5);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_regular_comment() {
        let result = parse_disable_comment("# This is a regular comment", 5);
        assert!(result.is_none());
    }

    #[test]
    fn test_ignore_tracker_is_ignored() {
        let mut tracker = IgnoreTracker::new();
        tracker.add_ignore("server-tokens-enabled", 10);

        assert!(tracker.is_ignored("server-tokens-enabled", 10));
        assert!(!tracker.is_ignored("server-tokens-enabled", 11));
        assert!(!tracker.is_ignored("other-rule", 10));
    }

    #[test]
    fn test_ignore_tracker_from_content() {
        let content = r#"
# nginx-lint:disable server-tokens-enabled dev environment
server_tokens on;
"#;
        let (tracker, warnings) = IgnoreTracker::from_content(content);
        assert!(warnings.is_empty());
        assert!(tracker.is_ignored("server-tokens-enabled", 3));
        assert!(!tracker.is_ignored("server-tokens-enabled", 2));
    }

    #[test]
    fn test_ignore_tracker_from_content_with_warnings() {
        let content = r#"
# nginx-lint:disable
server_tokens on;
"#;
        let (_, warnings) = IgnoreTracker::from_content(content);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("requires a rule name"));
    }

    #[test]
    fn test_filter_errors() {
        let mut tracker = IgnoreTracker::new();
        tracker.add_ignore("server-tokens-enabled", 5);

        let errors = vec![
            LintError::new(
                "server-tokens-enabled",
                "security",
                "test error",
                Severity::Warning,
            )
            .with_location(5, 1),
            LintError::new(
                "server-tokens-enabled",
                "security",
                "test error",
                Severity::Warning,
            )
            .with_location(6, 1),
            LintError::new("other-rule", "security", "test error", Severity::Warning)
                .with_location(5, 1),
        ];

        let result = filter_errors(errors, &tracker);
        assert_eq!(result.errors.len(), 2);
        assert_eq!(result.ignored_count, 1);
        // Line 5 server-tokens-enabled should be filtered out
        assert!(result
            .errors
            .iter()
            .all(|e| !(e.rule == "server-tokens-enabled" && e.line == Some(5))));
    }

    #[test]
    fn test_filter_errors_without_line_info() {
        let mut tracker = IgnoreTracker::new();
        tracker.add_ignore("some-rule", 5);

        let errors = vec![LintError::new(
            "some-rule",
            "test",
            "error without line",
            Severity::Warning,
        )];

        let result = filter_errors(errors, &tracker);
        assert_eq!(result.errors.len(), 1); // Should not be filtered
        assert_eq!(result.ignored_count, 0);
    }

    #[test]
    fn test_only_affects_next_line() {
        let content = r#"
# nginx-lint:disable server-tokens-enabled reason
server_tokens on;
server_tokens on;
"#;
        let (tracker, warnings) = IgnoreTracker::from_content(content);
        assert!(warnings.is_empty());
        assert!(tracker.is_ignored("server-tokens-enabled", 3)); // Line after comment
        assert!(!tracker.is_ignored("server-tokens-enabled", 4)); // Second occurrence
    }

    #[test]
    fn test_warnings_to_errors() {
        let warnings = vec![IgnoreWarning {
            line: 5,
            message: "test warning".to_string(),
        }];

        let errors = warnings_to_errors(warnings);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].rule, "ignore-comment");
        assert_eq!(errors[0].category, "ignore");
        assert_eq!(errors[0].message, "test warning");
        assert_eq!(errors[0].severity, Severity::Warning);
        assert_eq!(errors[0].line, Some(5));
    }

    #[test]
    fn test_parse_inline_comment() {
        let result = parse_disable_comment(
            "server_tokens on; # nginx-lint:disable server-tokens-enabled dev environment",
            5,
        );
        assert!(result.is_some());
        let (rule, line) = result.unwrap().unwrap();
        assert_eq!(rule, "server-tokens-enabled");
        assert_eq!(line, 5); // Same line (inline)
    }

    #[test]
    fn test_parse_inline_comment_with_japanese_reason() {
        let result = parse_disable_comment(
            "server_tokens on; # nginx-lint:disable server-tokens-enabled 開発環境用",
            5,
        );
        assert!(result.is_some());
        let (rule, line) = result.unwrap().unwrap();
        assert_eq!(rule, "server-tokens-enabled");
        assert_eq!(line, 5); // Same line (inline)
    }

    #[test]
    fn test_inline_comment_missing_reason() {
        let result = parse_disable_comment(
            "server_tokens on; # nginx-lint:disable server-tokens-enabled",
            5,
        );
        assert!(result.is_some());
        let warning = result.unwrap().unwrap_err();
        assert_eq!(warning.line, 5);
        assert!(warning.message.contains("requires a reason"));
    }

    #[test]
    fn test_ignore_tracker_inline_comment() {
        let content = r#"
server_tokens on; # nginx-lint:disable server-tokens-enabled dev environment
"#;
        let (tracker, warnings) = IgnoreTracker::from_content(content);
        assert!(warnings.is_empty());
        assert!(tracker.is_ignored("server-tokens-enabled", 2)); // Same line
        assert!(!tracker.is_ignored("server-tokens-enabled", 3));
    }

    #[test]
    fn test_both_comment_styles() {
        let content = r#"
# nginx-lint:disable server-tokens-enabled reason for next line
server_tokens on;
autoindex on; # nginx-lint:disable autoindex-enabled reason for this line
"#;
        let (tracker, warnings) = IgnoreTracker::from_content(content);
        assert!(warnings.is_empty());
        // Comment-only line targets next line
        assert!(tracker.is_ignored("server-tokens-enabled", 3));
        // Inline comment targets same line
        assert!(tracker.is_ignored("autoindex-enabled", 4));
    }
}
