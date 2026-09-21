//! What a component returns, converted for the host: its specs and its
//! findings, with every string it hands over sanitized.

use super::bindings;
use crate::linter::{LintError, Severity};

/// Plugin spec returned by the plugin
#[derive(Debug, Clone)]
pub struct PluginSpec {
    pub name: String,
    pub category: String,
    pub description: String,
    #[allow(dead_code)]
    pub api_version: String,
    #[allow(dead_code)]
    pub severity: Option<String>,
    pub why: Option<String>,
    pub bad_example: Option<String>,
    pub good_example: Option<String>,
    pub references: Option<Vec<String>>,
    pub min_nginx_version: Option<String>,
    pub max_nginx_version: Option<String>,
}

/// Replace control characters (except `\n` and `\t`) and Unicode
/// Bidi_Control characters in plugin-provided text with U+FFFD (�).
///
/// Plugin output is untrusted and is printed to the user's terminal or
/// embedded in generated documentation; raw control characters such as ESC
/// would allow ANSI escape sequence injection (e.g. spoofing or hiding parts
/// of the lint report), and bidi overrides (Trojan Source) can visually
/// reorder displayed text. Characters are replaced rather than removed so
/// that tampering stays visible to the user.
pub(crate) fn sanitize_text(text: &str) -> String {
    text.chars()
        .map(|c| {
            let is_disallowed_control = c.is_control() && c != '\n' && c != '\t';
            // All characters with the Unicode Bidi_Control property
            let is_bidi_control = matches!(
                c,
                '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
            );
            if is_disallowed_control || is_bidi_control {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

/// Sanitize an optional plugin-provided string.
fn sanitize_opt(text: &Option<String>) -> Option<String> {
    text.as_deref().map(sanitize_text)
}

/// Convert WIT Severity to crate Severity
fn convert_severity(severity: &bindings::nginx_lint::plugin::types::Severity) -> Severity {
    match severity {
        bindings::nginx_lint::plugin::types::Severity::Error => Severity::Error,
        bindings::nginx_lint::plugin::types::Severity::Warning => Severity::Warning,
    }
}

/// Convert WIT Fix to crate Fix
fn convert_fix(fix: &bindings::nginx_lint::plugin::types::Fix) -> crate::linter::Fix {
    crate::linter::Fix {
        line: fix.line as usize,
        old_text: fix.old_text.clone(),
        new_text: fix.new_text.clone(),
        delete_line: fix.delete_line,
        insert_after: fix.insert_after,
        start_offset: fix.start_offset.map(|v| v as usize),
        end_offset: fix.end_offset.map(|v| v as usize),
    }
}

/// Convert WIT LintError to crate LintError
pub(crate) fn convert_lint_error(
    error: &bindings::nginx_lint::plugin::types::LintError,
) -> LintError {
    let severity = convert_severity(&error.severity);
    let mut lint_error = LintError::new(
        &sanitize_text(&error.rule),
        &sanitize_text(&error.category),
        &sanitize_text(&error.message),
        severity,
    );

    if let (Some(line), Some(column)) = (error.line, error.column) {
        lint_error = lint_error.with_location(line as usize, column as usize);
    } else if let Some(line) = error.line {
        lint_error = lint_error.with_location(line as usize, 1);
    }

    for fix in &error.fixes {
        lint_error = lint_error.with_fix(convert_fix(fix));
    }

    lint_error
}

/// Convert WIT PluginSpec to our PluginSpec format
pub(crate) fn convert_plugin_spec(
    spec: &bindings::nginx_lint::plugin::types::PluginSpec,
) -> PluginSpec {
    PluginSpec {
        name: sanitize_text(&spec.name),
        category: sanitize_text(&spec.category),
        description: sanitize_text(&spec.description),
        api_version: sanitize_text(&spec.api_version),
        severity: sanitize_opt(&spec.severity),
        why: sanitize_opt(&spec.why),
        bad_example: sanitize_opt(&spec.bad_example),
        good_example: sanitize_opt(&spec.good_example),
        references: spec
            .references
            .as_ref()
            .map(|refs| refs.iter().map(|r| sanitize_text(r)).collect()),
        min_nginx_version: sanitize_opt(&spec.min_nginx_version),
        max_nginx_version: sanitize_opt(&spec.max_nginx_version),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_text_replaces_ansi_escape() {
        // ESC [ 31 m (red) + ESC [ 2 K (erase line) — typical injection payloads.
        // Replaced with U+FFFD so the tampering stays visible.
        let input = "\x1b[31mfake error\x1b[2K";
        assert_eq!(sanitize_text(input), "\u{FFFD}[31mfake error\u{FFFD}[2K");
    }

    #[test]
    fn test_sanitize_text_keeps_newline_and_tab() {
        let input = "line1\n\tline2";
        assert_eq!(sanitize_text(input), "line1\n\tline2");
    }

    #[test]
    fn test_sanitize_text_replaces_other_control_chars() {
        // \r (CR), \x07 (BEL), \x08 (BS) are replaced; unicode text is kept
        let input = "警告\r\x07\x08です";
        assert_eq!(sanitize_text(input), "警告\u{FFFD}\u{FFFD}\u{FFFD}です");
    }

    #[test]
    fn test_sanitize_text_replaces_c1_control_chars() {
        // 0x9B is a C1 control char acting as a standalone CSI
        let input = "a\u{9B}31mb";
        assert_eq!(sanitize_text(input), "a\u{FFFD}31mb");
    }

    #[test]
    fn test_sanitize_text_replaces_bidi_controls() {
        // U+202E (RTL override) and U+2066 (LTR isolate) enable Trojan
        // Source style display reordering
        let input = "safe\u{202E}gnirts live\u{2066}x";
        assert_eq!(sanitize_text(input), "safe\u{FFFD}gnirts live\u{FFFD}x");
    }

    #[test]
    fn test_sanitize_text_replaces_implicit_bidi_marks() {
        // U+200E (LRM), U+200F (RLM), U+061C (ALM) — the remaining
        // Bidi_Control characters
        let input = "a\u{200E}b\u{200F}c\u{061C}d";
        assert_eq!(sanitize_text(input), "a\u{FFFD}b\u{FFFD}c\u{FFFD}d");
    }

    #[test]
    fn test_convert_lint_error_sanitizes_strings() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "evil\x1brule".to_string(),
            category: "cat\x1begory".to_string(),
            message: "msg\x1b[31m with escape".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Warning,
            line: Some(1),
            column: Some(1),
            fixes: vec![],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.rule, "evil\u{FFFD}rule");
        assert_eq!(error.category, "cat\u{FFFD}egory");
        assert_eq!(error.message, "msg\u{FFFD}[31m with escape");
    }

    #[test]
    fn test_convert_plugin_spec_sanitizes_strings() {
        let wit_spec = bindings::nginx_lint::plugin::types::PluginSpec {
            name: "na\x1bme".to_string(),
            category: "cat\x1begory".to_string(),
            description: "desc\x1b[0m".to_string(),
            api_version: "1.0".to_string(),
            severity: Some("warn\x1bing".to_string()),
            why: Some("why\x07".to_string()),
            bad_example: Some("bad\nexample\x1b".to_string()),
            good_example: Some("good\texample\x08".to_string()),
            references: Some(vec!["https://example.com/\x1b[31m".to_string()]),
            min_nginx_version: Some("1.0\x1b".to_string()),
            max_nginx_version: None,
        };
        let spec = convert_plugin_spec(&wit_spec);
        assert_eq!(spec.name, "na\u{FFFD}me");
        assert_eq!(spec.category, "cat\u{FFFD}egory");
        assert_eq!(spec.description, "desc\u{FFFD}[0m");
        assert_eq!(spec.severity.as_deref(), Some("warn\u{FFFD}ing"));
        assert_eq!(spec.why.as_deref(), Some("why\u{FFFD}"));
        // Newlines and tabs in examples are preserved
        assert_eq!(spec.bad_example.as_deref(), Some("bad\nexample\u{FFFD}"));
        assert_eq!(spec.good_example.as_deref(), Some("good\texample\u{FFFD}"));
        assert_eq!(
            spec.references,
            Some(vec!["https://example.com/\u{FFFD}[31m".to_string()])
        );
        assert_eq!(spec.min_nginx_version.as_deref(), Some("1.0\u{FFFD}"));
    }

    #[test]
    fn test_convert_severity_error() {
        let wit_severity = bindings::nginx_lint::plugin::types::Severity::Error;
        assert!(matches!(convert_severity(&wit_severity), Severity::Error));
    }

    #[test]
    fn test_convert_severity_warning() {
        let wit_severity = bindings::nginx_lint::plugin::types::Severity::Warning;
        assert!(matches!(convert_severity(&wit_severity), Severity::Warning));
    }

    #[test]
    fn test_convert_fix_basic() {
        let wit_fix = bindings::nginx_lint::plugin::types::Fix {
            line: 10,
            old_text: Some("old".to_string()),
            new_text: "new".to_string(),
            delete_line: false,
            insert_after: true,
            start_offset: Some(5),
            end_offset: Some(8),
        };
        let fix = convert_fix(&wit_fix);
        assert_eq!(fix.line, 10);
        assert_eq!(fix.old_text.as_deref(), Some("old"));
        assert_eq!(fix.new_text, "new");
        assert!(!fix.delete_line);
        assert!(fix.insert_after);
        assert_eq!(fix.start_offset, Some(5));
        assert_eq!(fix.end_offset, Some(8));
    }

    #[test]
    fn test_convert_fix_optional_fields_none() {
        let wit_fix = bindings::nginx_lint::plugin::types::Fix {
            line: 1,
            old_text: None,
            new_text: "text".to_string(),
            delete_line: true,
            insert_after: false,
            start_offset: None,
            end_offset: None,
        };
        let fix = convert_fix(&wit_fix);
        assert!(fix.old_text.is_none());
        assert!(fix.delete_line);
        assert!(fix.start_offset.is_none());
        assert!(fix.end_offset.is_none());
    }

    #[test]
    fn test_convert_lint_error_with_location() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "test-rule".to_string(),
            category: "test-cat".to_string(),
            message: "test message".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Warning,
            line: Some(42),
            column: Some(10),
            fixes: vec![],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.rule, "test-rule");
        assert_eq!(error.category, "test-cat");
        assert_eq!(error.message, "test message");
        assert!(matches!(error.severity, Severity::Warning));
        assert_eq!(error.line, Some(42));
        assert_eq!(error.column, Some(10));
    }

    #[test]
    fn test_convert_lint_error_line_only() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "rule".to_string(),
            category: "cat".to_string(),
            message: "msg".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Error,
            line: Some(5),
            column: None,
            fixes: vec![],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.line, Some(5));
        assert_eq!(error.column, Some(1)); // defaults to column 1
    }

    #[test]
    fn test_convert_lint_error_no_location() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "rule".to_string(),
            category: "cat".to_string(),
            message: "msg".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Error,
            line: None,
            column: None,
            fixes: vec![],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.line, None);
        assert_eq!(error.column, None);
    }

    #[test]
    fn test_convert_lint_error_with_fixes() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "rule".to_string(),
            category: "cat".to_string(),
            message: "msg".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Warning,
            line: Some(1),
            column: Some(1),
            fixes: vec![bindings::nginx_lint::plugin::types::Fix {
                line: 1,
                old_text: Some("bad".to_string()),
                new_text: "good".to_string(),
                delete_line: false,
                insert_after: false,
                start_offset: None,
                end_offset: None,
            }],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.fixes.len(), 1);
        assert_eq!(error.fixes[0].new_text, "good");
    }

    #[test]
    fn test_convert_plugin_spec() {
        let wit_spec = bindings::nginx_lint::plugin::types::PluginSpec {
            name: "test-plugin".to_string(),
            category: "security".to_string(),
            description: "A test plugin".to_string(),
            api_version: "1.0".to_string(),
            severity: Some("warning".to_string()),
            why: Some("because".to_string()),
            bad_example: Some("bad".to_string()),
            good_example: Some("good".to_string()),
            references: Some(vec!["https://example.com".to_string()]),
            min_nginx_version: Some("0.6.27".to_string()),
            max_nginx_version: Some("1.30.0".to_string()),
        };
        let spec = convert_plugin_spec(&wit_spec);
        assert_eq!(spec.name, "test-plugin");
        assert_eq!(spec.category, "security");
        assert_eq!(spec.description, "A test plugin");
        assert_eq!(spec.api_version, "1.0");
        assert_eq!(spec.severity.as_deref(), Some("warning"));
        assert_eq!(spec.why.as_deref(), Some("because"));
        assert_eq!(spec.bad_example.as_deref(), Some("bad"));
        assert_eq!(spec.good_example.as_deref(), Some("good"));
        assert_eq!(
            spec.references,
            Some(vec!["https://example.com".to_string()])
        );
        assert_eq!(spec.min_nginx_version.as_deref(), Some("0.6.27"));
        assert_eq!(spec.max_nginx_version.as_deref(), Some("1.30.0"));
    }

    #[test]
    fn test_convert_plugin_spec_optional_none() {
        let wit_spec = bindings::nginx_lint::plugin::types::PluginSpec {
            name: "minimal".to_string(),
            category: "test".to_string(),
            description: "Minimal".to_string(),
            api_version: "1.0".to_string(),
            severity: None,
            why: None,
            bad_example: None,
            good_example: None,
            references: None,
            min_nginx_version: None,
            max_nginx_version: None,
        };
        let spec = convert_plugin_spec(&wit_spec);
        assert_eq!(spec.name, "minimal");
        assert!(spec.severity.is_none());
        assert!(spec.why.is_none());
        assert!(spec.references.is_none());
        assert!(spec.min_nginx_version.is_none());
        assert!(spec.max_nginx_version.is_none());
    }
}
