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

use crate::linter::{Fix, LintError, Severity};

/// A warning generated from parsing ignore comments
#[derive(Debug, Clone)]
pub struct IgnoreWarning {
    /// Line number where the warning occurred
    pub line: usize,
    /// Warning message
    pub message: String,
    /// Optional fix for the warning
    pub fix: Option<Fix>,
}

/// Information about a single ignore directive
#[derive(Debug, Clone)]
struct IgnoreDirective {
    /// Line number where the comment is located
    comment_line: usize,
    /// Line number that this directive targets
    target_line: usize,
    /// Rule name to ignore
    rule_name: String,
    /// Whether this directive was used to ignore an error
    used: bool,
    /// Whether this is an inline comment (vs comment-only line)
    is_inline: bool,
    /// For inline comments, the content before the comment (for fix)
    content_before_comment: Option<String>,
}

/// Tracks ignored rules per line
#[derive(Debug, Default)]
pub struct IgnoreTracker {
    /// Map from target line number to set of ignored rule names
    ignored_lines: HashMap<usize, HashSet<String>>,
    /// All ignore directives for tracking usage
    directives: Vec<IgnoreDirective>,
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
        Self::from_content_with_rules(content, None)
    }

    /// Build an ignore tracker from content with optional rule name validation
    pub fn from_content_with_rules(
        content: &str,
        valid_rules: Option<&HashSet<String>>,
    ) -> (Self, Vec<IgnoreWarning>) {
        let mut tracker = Self::new();
        let mut warnings = Vec::new();

        // First pass: parse all disable comments
        let lines: Vec<&str> = content.lines().collect();
        let mut parsed_comments: Vec<(usize, Result<ParsedDisableComment, IgnoreWarning>)> =
            Vec::new();

        for (line_idx, line) in lines.iter().enumerate() {
            let line_number = line_idx + 1; // Convert to 1-indexed
            if let Some(result) = parse_disable_comment(line, line_number) {
                parsed_comments.push((line_idx, result));
            }
        }

        // Second pass: adjust target lines for consecutive comment-only disables
        // They should all target the first non-disable-comment line
        for i in 0..parsed_comments.len() {
            let (line_idx, ref result) = parsed_comments[i];

            match result {
                Ok(parsed) if !parsed.is_inline => {
                    // Find the first non-disable-comment line after this one
                    let mut target_idx = line_idx + 1;
                    while target_idx < lines.len() {
                        // Check if this line is also a disable comment
                        let is_disable_comment = parsed_comments
                            .iter()
                            .any(|(idx, r)| *idx == target_idx && matches!(r, Ok(p) if !p.is_inline));
                        if !is_disable_comment {
                            break;
                        }
                        target_idx += 1;
                    }
                    let actual_target_line = target_idx + 1; // Convert to 1-indexed

                    // Check if rule name is valid
                    if let Some(valid) = valid_rules {
                        if !valid.contains(&parsed.rule_name) {
                            warnings.push(IgnoreWarning {
                                line: parsed.comment_line,
                                message: format!(
                                    "unknown rule '{}' in nginx-lint:disable comment",
                                    parsed.rule_name
                                ),
                                fix: None,
                            });
                        }
                    }

                    tracker
                        .ignored_lines
                        .entry(actual_target_line)
                        .or_default()
                        .insert(parsed.rule_name.clone());

                    tracker.directives.push(IgnoreDirective {
                        comment_line: parsed.comment_line,
                        target_line: actual_target_line,
                        rule_name: parsed.rule_name.clone(),
                        used: false,
                        is_inline: false,
                        content_before_comment: None,
                    });
                }
                Ok(parsed) => {
                    // Inline comment - targets current line
                    if let Some(valid) = valid_rules {
                        if !valid.contains(&parsed.rule_name) {
                            warnings.push(IgnoreWarning {
                                line: parsed.comment_line,
                                message: format!(
                                    "unknown rule '{}' in nginx-lint:disable comment",
                                    parsed.rule_name
                                ),
                                fix: None,
                            });
                        }
                    }

                    tracker
                        .ignored_lines
                        .entry(parsed.target_line)
                        .or_default()
                        .insert(parsed.rule_name.clone());

                    tracker.directives.push(IgnoreDirective {
                        comment_line: parsed.comment_line,
                        target_line: parsed.target_line,
                        rule_name: parsed.rule_name.clone(),
                        used: false,
                        is_inline: true,
                        content_before_comment: parsed.content_before_comment.clone(),
                    });
                }
                Err(warning) => {
                    warnings.push(warning.clone());
                }
            }
        }

        (tracker, warnings)
    }

    /// Mark a rule as used on a specific line
    fn mark_used(&mut self, rule: &str, line: usize) {
        for directive in &mut self.directives {
            if directive.target_line == line && directive.rule_name == rule {
                directive.used = true;
            }
        }
    }

    /// Get warnings for unused ignore directives
    pub fn unused_warnings(&self) -> Vec<IgnoreWarning> {
        self.directives
            .iter()
            .filter(|d| !d.used)
            .map(|d| {
                // Provide fix for both comment-only lines and inline comments
                let fix = if d.is_inline {
                    // For inline comments, replace line with just the content before the comment
                    d.content_before_comment
                        .as_ref()
                        .map(|content| Fix::replace_line(d.comment_line, content))
                } else {
                    // For comment-only lines, delete the entire line
                    Some(Fix::delete(d.comment_line))
                };

                IgnoreWarning {
                    line: d.comment_line,
                    message: format!(
                        "unused nginx-lint:disable comment for rule '{}'",
                        d.rule_name
                    ),
                    fix,
                }
            })
            .collect()
    }

    /// Add an ignore rule for a specific line
    #[cfg(test)]
    pub fn add_ignore(&mut self, rule: &str, line: usize) {
        self.ignored_lines
            .entry(line)
            .or_default()
            .insert(rule.to_string());
        self.directives.push(IgnoreDirective {
            comment_line: line.saturating_sub(1).max(1),
            target_line: line,
            rule_name: rule.to_string(),
            used: false,
            is_inline: false,
            content_before_comment: None,
        });
    }
}

/// Parsed result of a disable comment
#[derive(Debug)]
struct ParsedDisableComment {
    /// Rule name to ignore
    rule_name: String,
    /// Target line number (the line to ignore errors on)
    target_line: usize,
    /// Comment line number (where the comment is located)
    comment_line: usize,
    /// Whether this is an inline comment
    is_inline: bool,
    /// For inline comments, the content before the comment (trimmed)
    content_before_comment: Option<String>,
}

/// Parse a disable comment from a line
///
/// Supports two formats:
/// 1. Comment-only line: `# nginx-lint:disable rule-name reason` → targets next line
/// 2. Inline comment: `directive; # nginx-lint:disable rule-name reason` → targets current line
///
/// Returns:
/// - `None` if the line does not contain a disable comment
/// - `Some(Ok(ParsedDisableComment))` if valid
/// - `Some(Err(warning))` if the comment is malformed
fn parse_disable_comment(
    line: &str,
    line_number: usize,
) -> Option<Result<ParsedDisableComment, IgnoreWarning>> {
    const DISABLE_PREFIX: &str = "nginx-lint:disable";

    // Find the comment marker
    let comment_start = line.find('#')?;
    let comment_part = &line[comment_start..];
    let comment = comment_part.trim_start_matches('#').trim();

    // Check for nginx-lint:disable prefix
    let rest = comment.strip_prefix(DISABLE_PREFIX)?;
    let rest = rest.trim();

    // Determine if this is a comment-only line or inline comment
    let before_comment_trimmed = line[..comment_start].trim();
    let is_inline = !before_comment_trimmed.is_empty();

    // Parse rule name and reason
    let parts: Vec<&str> = rest.splitn(2, |c: char| c.is_whitespace()).collect();

    // Check for missing rule name
    if parts.is_empty() || parts[0].is_empty() {
        return Some(Err(IgnoreWarning {
            line: line_number,
            message: "nginx-lint:disable requires a rule name".to_string(),
            fix: None,
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
            fix: None,
        }));
    }

    // Inline comment targets current line, comment-only line targets next line
    let target_line = if is_inline {
        line_number
    } else {
        line_number + 1
    };

    // Content before comment for inline fixes (preserve leading whitespace, trim trailing)
    let content_before = if is_inline {
        Some(line[..comment_start].trim_end().to_string())
    } else {
        None
    };

    Some(Ok(ParsedDisableComment {
        rule_name,
        target_line,
        comment_line: line_number,
        is_inline,
        content_before_comment: content_before,
    }))
}

/// Result of filtering errors with ignore tracker
#[derive(Debug)]
pub struct FilterResult {
    /// Errors that were not ignored
    pub errors: Vec<LintError>,
    /// Number of errors that were ignored
    pub ignored_count: usize,
    /// Warnings for unused ignore directives
    pub unused_warnings: Vec<IgnoreWarning>,
}

/// Filter errors using an ignore tracker, returning remaining errors and ignored count
pub fn filter_errors(errors: Vec<LintError>, tracker: &mut IgnoreTracker) -> FilterResult {
    let mut remaining = Vec::new();
    let mut ignored_count = 0;

    for error in errors {
        if let Some(line) = error.line
            && tracker.is_ignored(&error.rule, line)
        {
            tracker.mark_used(&error.rule, line);
            ignored_count += 1;
            continue;
        }
        remaining.push(error);
    }

    let unused_warnings = tracker.unused_warnings();

    FilterResult {
        errors: remaining,
        ignored_count,
        unused_warnings,
    }
}

/// Convert ignore warnings to lint errors
pub fn warnings_to_errors(warnings: Vec<IgnoreWarning>) -> Vec<LintError> {
    warnings
        .into_iter()
        .map(|warning| {
            let mut error = LintError::new(
                "invalid-nginx-lint-disable",
                "ignore",
                &warning.message,
                Severity::Warning,
            )
            .with_location(warning.line, 1);

            if let Some(fix) = warning.fix {
                error = error.with_fix(fix);
            }

            error
        })
        .collect()
}

/// Get a set of all known rule names
pub fn known_rule_names() -> HashSet<String> {
    // All rule names that can be used with nginx-lint:disable
    [
        "duplicate-directive",
        "unmatched-braces",
        "unclosed-quote",
        "missing-semicolon",
        "invalid-directive-context",
        "deprecated-ssl-protocol",
        "server-tokens-enabled",
        "autoindex-enabled",
        "weak-ssl-ciphers",
        "indent",
        "trailing-whitespace",
        "space-before-semicolon",
        "gzip-not-enabled",
        "missing-error-log",
        "proxy-pass-domain",
        "upstream-server-no-resolve",
        "proxy-set-header-inheritance",
        "root-in-location",
        "alias-location-slash-mismatch",
        "proxy-pass-with-uri",
        "add-header-inheritance",
        "proxy-keepalive",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Prefix for context comments
const CONTEXT_PREFIX: &str = "nginx-lint:context";

/// Parse context comment from file content
///
/// Looks for `# nginx-lint:context http,server` in the first few lines of the file.
/// Returns the context as a vector of block names, or None if no context comment found.
///
/// # Example
/// ```
/// use nginx_lint::ignore::parse_context_comment;
///
/// let content = "# nginx-lint:context http,server\nserver { listen 80; }";
/// let context = parse_context_comment(content);
/// assert_eq!(context, Some(vec!["http".to_string(), "server".to_string()]));
/// ```
pub fn parse_context_comment(content: &str) -> Option<Vec<String>> {
    // Only check first 10 lines for context comment
    for line in content.lines().take(10) {
        let trimmed = line.trim();

        // Skip empty lines
        if trimmed.is_empty() {
            continue;
        }

        // Must be a comment
        if !trimmed.starts_with('#') {
            // If we hit a non-comment, non-empty line, stop looking
            break;
        }

        let comment = trimmed.trim_start_matches('#').trim();

        // Check for nginx-lint:context prefix
        if let Some(rest) = comment.strip_prefix(CONTEXT_PREFIX) {
            let context_str = rest.trim();
            if context_str.is_empty() {
                return None;
            }

            let context: Vec<String> = context_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            if context.is_empty() {
                return None;
            }

            return Some(context);
        }
    }

    None
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
        let parsed = result.unwrap().unwrap();
        assert_eq!(parsed.rule_name, "server-tokens-enabled");
        assert_eq!(parsed.target_line, 6); // Next line
        assert_eq!(parsed.comment_line, 5);
        assert!(!parsed.is_inline);
        assert!(parsed.content_before_comment.is_none());
    }

    #[test]
    fn test_parse_disable_comment_with_japanese_reason() {
        let result = parse_disable_comment(
            "# nginx-lint:disable server-tokens-enabled 開発環境用",
            5,
        );
        assert!(result.is_some());
        let parsed = result.unwrap().unwrap();
        assert_eq!(parsed.rule_name, "server-tokens-enabled");
        assert_eq!(parsed.target_line, 6);
        assert_eq!(parsed.comment_line, 5);
        assert!(!parsed.is_inline);
        assert!(parsed.content_before_comment.is_none());
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

        let result = filter_errors(errors, &mut tracker);
        assert_eq!(result.errors.len(), 2);
        assert_eq!(result.ignored_count, 1);
        // Line 5 server-tokens-enabled should be filtered out
        assert!(result
            .errors
            .iter()
            .all(|e| !(e.rule == "server-tokens-enabled" && e.line == Some(5))));
        // The used directive should have no unused warnings for that rule
        assert!(result.unused_warnings.is_empty());
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

        let result = filter_errors(errors, &mut tracker);
        assert_eq!(result.errors.len(), 1); // Should not be filtered
        assert_eq!(result.ignored_count, 0);
        // The directive was not used, so there should be an unused warning
        assert_eq!(result.unused_warnings.len(), 1);
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
    fn test_consecutive_disable_comments() {
        // Multiple consecutive disable comments should all target the same line
        let content = r#"
# nginx-lint:disable server-tokens-enabled reason1
# nginx-lint:disable autoindex-enabled reason2
server_tokens on;
"#;
        let (tracker, warnings) = IgnoreTracker::from_content(content);
        assert!(warnings.is_empty());
        // Both rules should be ignored on line 4 (the directive line)
        assert!(tracker.is_ignored("server-tokens-enabled", 4));
        assert!(tracker.is_ignored("autoindex-enabled", 4));
        // Should not be ignored on the comment lines themselves
        assert!(!tracker.is_ignored("server-tokens-enabled", 2));
        assert!(!tracker.is_ignored("autoindex-enabled", 3));
    }

    #[test]
    fn test_three_consecutive_disable_comments() {
        let content = r#"
# nginx-lint:disable server-tokens-enabled reason1
# nginx-lint:disable autoindex-enabled reason2
# nginx-lint:disable gzip-not-enabled reason3
server_tokens on;
"#;
        let (tracker, warnings) = IgnoreTracker::from_content(content);
        assert!(warnings.is_empty());
        // All three rules should be ignored on line 5
        assert!(tracker.is_ignored("server-tokens-enabled", 5));
        assert!(tracker.is_ignored("autoindex-enabled", 5));
        assert!(tracker.is_ignored("gzip-not-enabled", 5));
    }

    #[test]
    fn test_warnings_to_errors() {
        let warnings = vec![IgnoreWarning {
            line: 5,
            message: "test warning".to_string(),
            fix: None,
        }];

        let errors = warnings_to_errors(warnings);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].rule, "invalid-nginx-lint-disable");
        assert_eq!(errors[0].category, "ignore");
        assert_eq!(errors[0].message, "test warning");
        assert_eq!(errors[0].severity, Severity::Warning);
        assert_eq!(errors[0].line, Some(5));
        assert!(errors[0].fix.is_none());
    }

    #[test]
    fn test_warnings_to_errors_with_fix() {
        let warnings = vec![IgnoreWarning {
            line: 5,
            message: "test warning".to_string(),
            fix: Some(Fix::delete(5)),
        }];

        let errors = warnings_to_errors(warnings);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].fix.is_some());
        let fix = errors[0].fix.as_ref().unwrap();
        assert_eq!(fix.line, 5);
        assert!(fix.delete_line);
    }

    #[test]
    fn test_parse_inline_comment() {
        let result = parse_disable_comment(
            "server_tokens on; # nginx-lint:disable server-tokens-enabled dev environment",
            5,
        );
        assert!(result.is_some());
        let parsed = result.unwrap().unwrap();
        assert_eq!(parsed.rule_name, "server-tokens-enabled");
        assert_eq!(parsed.target_line, 5); // Same line (inline)
        assert_eq!(parsed.comment_line, 5);
        assert!(parsed.is_inline);
        assert_eq!(parsed.content_before_comment, Some("server_tokens on;".to_string()));
    }

    #[test]
    fn test_parse_inline_comment_with_japanese_reason() {
        let result = parse_disable_comment(
            "server_tokens on; # nginx-lint:disable server-tokens-enabled 開発環境用",
            5,
        );
        assert!(result.is_some());
        let parsed = result.unwrap().unwrap();
        assert_eq!(parsed.rule_name, "server-tokens-enabled");
        assert_eq!(parsed.target_line, 5); // Same line (inline)
        assert_eq!(parsed.comment_line, 5);
        assert!(parsed.is_inline);
        assert_eq!(parsed.content_before_comment, Some("server_tokens on;".to_string()));
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

    #[test]
    fn test_unknown_rule_name() {
        let content = r#"
# nginx-lint:disable unknown-rule-name some reason
server_tokens on;
"#;
        let valid_rules = known_rule_names();
        let (_, warnings) = IgnoreTracker::from_content_with_rules(content, Some(&valid_rules));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("unknown rule 'unknown-rule-name'"));
    }

    #[test]
    fn test_unused_ignore_directive() {
        let content = r#"
# nginx-lint:disable server-tokens-enabled reason
server_tokens off;
"#;
        let (mut tracker, _) = IgnoreTracker::from_content(content);

        // No errors to filter
        let errors: Vec<LintError> = vec![];
        let result = filter_errors(errors, &mut tracker);

        // Should have unused warning
        assert_eq!(result.unused_warnings.len(), 1);
        assert!(result.unused_warnings[0].message.contains("unused nginx-lint:disable"));
        assert!(result.unused_warnings[0].message.contains("server-tokens-enabled"));
    }

    #[test]
    fn test_used_ignore_directive_no_warning() {
        let content = r#"
# nginx-lint:disable server-tokens-enabled reason
server_tokens on;
"#;
        let (mut tracker, _) = IgnoreTracker::from_content(content);

        // Create an error that will be filtered
        let errors = vec![LintError::new(
            "server-tokens-enabled",
            "security",
            "test error",
            Severity::Warning,
        )
        .with_location(3, 1)];

        let result = filter_errors(errors, &mut tracker);

        // Should have no unused warnings
        assert!(result.unused_warnings.is_empty());
        assert_eq!(result.ignored_count, 1);
    }

    #[test]
    fn test_unused_comment_only_line_fix() {
        let content = r#"
# nginx-lint:disable server-tokens-enabled reason
server_tokens off;
"#;
        let (mut tracker, _) = IgnoreTracker::from_content(content);

        let errors: Vec<LintError> = vec![];
        let result = filter_errors(errors, &mut tracker);

        // Should have unused warning with delete fix
        assert_eq!(result.unused_warnings.len(), 1);
        let fix = result.unused_warnings[0].fix.as_ref().unwrap();
        assert_eq!(fix.line, 2); // Line of comment
        assert!(fix.delete_line);
    }

    #[test]
    fn test_unused_inline_comment_fix() {
        let content = r#"
server_tokens off; # nginx-lint:disable server-tokens-enabled reason
"#;
        let (mut tracker, _) = IgnoreTracker::from_content(content);

        let errors: Vec<LintError> = vec![];
        let result = filter_errors(errors, &mut tracker);

        // Should have unused warning with replace_line fix
        assert_eq!(result.unused_warnings.len(), 1);
        let fix = result.unused_warnings[0].fix.as_ref().unwrap();
        assert_eq!(fix.line, 2); // Line of inline comment
        assert!(!fix.delete_line); // Not deleting entire line
        assert!(fix.old_text.is_none()); // replace_line uses None for old_text
        assert_eq!(fix.new_text, "server_tokens off;");
    }

    // Context comment tests

    #[test]
    fn test_parse_context_comment_simple() {
        let content = "# nginx-lint:context http\nserver { listen 80; }";
        let context = parse_context_comment(content);
        assert_eq!(context, Some(vec!["http".to_string()]));
    }

    #[test]
    fn test_parse_context_comment_multiple() {
        let content = "# nginx-lint:context http,server\nlocation / { }";
        let context = parse_context_comment(content);
        assert_eq!(context, Some(vec!["http".to_string(), "server".to_string()]));
    }

    #[test]
    fn test_parse_context_comment_with_spaces() {
        let content = "# nginx-lint:context http, server\nlocation / { }";
        let context = parse_context_comment(content);
        assert_eq!(context, Some(vec!["http".to_string(), "server".to_string()]));
    }

    #[test]
    fn test_parse_context_comment_after_empty_lines() {
        let content = "\n\n# nginx-lint:context http\nserver { }";
        let context = parse_context_comment(content);
        assert_eq!(context, Some(vec!["http".to_string()]));
    }

    #[test]
    fn test_parse_context_comment_after_other_comments() {
        let content = "# Some description\n# nginx-lint:context http\nserver { }";
        let context = parse_context_comment(content);
        assert_eq!(context, Some(vec!["http".to_string()]));
    }

    #[test]
    fn test_parse_context_comment_not_found() {
        let content = "server { listen 80; }";
        let context = parse_context_comment(content);
        assert_eq!(context, None);
    }

    #[test]
    fn test_parse_context_comment_after_directive() {
        // Context comment after a directive should not be found
        let content = "server { }\n# nginx-lint:context http";
        let context = parse_context_comment(content);
        assert_eq!(context, None);
    }

    #[test]
    fn test_parse_context_comment_empty_value() {
        let content = "# nginx-lint:context\nserver { }";
        let context = parse_context_comment(content);
        assert_eq!(context, None);
    }
}
