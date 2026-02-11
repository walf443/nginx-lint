use crate::LintError;
use crate::Severity;
use crate::config::{Color, ColorConfig};
use colored::{ColoredString, Colorize};
use std::path::Path;

pub(crate) fn report(
    errors: &[LintError],
    path: &Path,
    colors: &ColorConfig,
    ignored_count: usize,
) {
    let path_str = path.display();

    let mut sorted_errors: Vec<_> = errors.iter().collect();
    sorted_errors.sort_by(|a, b| match (a.line, b.line) {
        (Some(line_a), Some(line_b)) => {
            line_a
                .cmp(&line_b)
                .then_with(|| match (a.column, b.column) {
                    (Some(col_a), Some(col_b)) => col_a.cmp(&col_b),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                })
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    for error in sorted_errors {
        let location = match (error.line, error.column) {
            (Some(line), Some(col)) => format!("{}:{}:{}", path_str, line, col),
            (Some(line), None) => format!("{}:{}", path_str, line),
            _ => format!("{}", path_str),
        };

        let (severity_label, color) = match error.severity {
            Severity::Error => ("error", colors.error),
            Severity::Warning => ("warning", colors.warning),
        };

        let severity_str = apply_color(
            &format!("{}[{}/{}]", severity_label, error.category, error.rule),
            color,
        )
        .bold();

        println!("{}: {}: {}", location, severity_str, error.message);
    }

    if !errors.is_empty() || ignored_count > 0 {
        println!();
        let error_count = errors
            .iter()
            .filter(|e| e.severity == Severity::Error)
            .count();
        let warning_count = errors
            .iter()
            .filter(|e| e.severity == Severity::Warning)
            .count();

        let mut parts = Vec::new();
        if error_count > 0 {
            parts.push(format!("{} error(s)", error_count));
        }
        if warning_count > 0 {
            parts.push(format!("{} warning(s)", warning_count));
        }
        if ignored_count > 0 {
            parts.push(format!("{} ignored", ignored_count));
        }

        if !parts.is_empty() {
            println!("Found {}", parts.join(", "));
        }
    }
}

#[cfg(test)]
fn format_line(error: &LintError, path: &Path) -> String {
    let path_str = path.display();

    let location = match (error.line, error.column) {
        (Some(line), Some(col)) => format!("{}:{}:{}", path_str, line, col),
        (Some(line), None) => format!("{}:{}", path_str, line),
        _ => format!("{}", path_str),
    };

    let severity_label = match error.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };

    format!(
        "{}: {}[{}/{}]: {}",
        location, severity_label, error.category, error.rule, error.message
    )
}

/// Apply a color to a string
fn apply_color(s: &str, color: Color) -> ColoredString {
    match color {
        Color::Black => s.black(),
        Color::Red => s.red(),
        Color::Green => s.green(),
        Color::Yellow => s.yellow(),
        Color::Blue => s.blue(),
        Color::Magenta => s.magenta(),
        Color::Cyan => s.cyan(),
        Color::White => s.white(),
        Color::BrightBlack => s.bright_black(),
        Color::BrightRed => s.bright_red(),
        Color::BrightGreen => s.bright_green(),
        Color::BrightYellow => s.bright_yellow(),
        Color::BrightBlue => s.bright_blue(),
        Color::BrightMagenta => s.bright_magenta(),
        Color::BrightCyan => s.bright_cyan(),
        Color::BrightWhite => s.bright_white(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_error(
        rule: &str,
        category: &str,
        message: &str,
        severity: Severity,
        line: Option<usize>,
        column: Option<usize>,
    ) -> LintError {
        LintError {
            rule: rule.to_string(),
            category: category.to_string(),
            message: message.to_string(),
            severity,
            line,
            column,
            fixes: vec![],
        }
    }

    #[test]
    fn test_error_format_line() {
        let error = make_error(
            "missing-semicolon",
            "syntax",
            "Missing semicolon at end of directive",
            Severity::Error,
            Some(10),
            Some(5),
        );
        let path = Path::new("nginx.conf");
        assert_eq!(
            format_line(&error, path),
            "nginx.conf:10:5: error[syntax/missing-semicolon]: Missing semicolon at end of directive"
        );
    }

    #[test]
    fn test_warning_format_line() {
        let error = make_error(
            "server-tokens-enabled",
            "security",
            "server_tokens is enabled",
            Severity::Warning,
            Some(3),
            Some(1),
        );
        let path = Path::new("nginx.conf");
        assert_eq!(
            format_line(&error, path),
            "nginx.conf:3:1: warning[security/server-tokens-enabled]: server_tokens is enabled"
        );
    }

    #[test]
    fn test_format_line_without_column() {
        let error = make_error(
            "indent",
            "style",
            "Wrong indentation",
            Severity::Warning,
            Some(5),
            None,
        );
        let path = Path::new("nginx.conf");
        assert_eq!(
            format_line(&error, path),
            "nginx.conf:5: warning[style/indent]: Wrong indentation"
        );
    }

    #[test]
    fn test_format_line_without_line_and_column() {
        let error = make_error(
            "some-rule",
            "best-practices",
            "Something is wrong",
            Severity::Error,
            None,
            None,
        );
        let path = Path::new("conf/nginx.conf");
        assert_eq!(
            format_line(&error, path),
            "conf/nginx.conf: error[best-practices/some-rule]: Something is wrong"
        );
    }
}
