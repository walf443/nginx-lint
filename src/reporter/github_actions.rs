use crate::LintError;
use crate::Severity;
use std::path::Path;

pub(crate) fn report(errors: &[LintError], path: &Path) {
    for line in format(errors, path) {
        println!("{}", line);
    }
}

pub(crate) fn format(errors: &[LintError], path: &Path) -> Vec<String> {
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

    let mut lines = Vec::new();
    for error in sorted_errors {
        let level = match error.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };

        let mut params = format!("file={}", path_str);
        if let Some(line) = error.line {
            params.push_str(&format!(",line={}", line));
        }
        if let Some(col) = error.column {
            params.push_str(&format!(",col={}", col));
        }
        params.push_str(&format!(",title={}/{}", error.category, error.rule));

        lines.push(format!("::{} {}::{}", level, params, error.message));
    }
    lines
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
    fn test_error_format() {
        let errors = vec![make_error(
            "missing-semicolon",
            "syntax",
            "Missing semicolon at end of directive",
            Severity::Error,
            Some(10),
            Some(5),
        )];
        let path = Path::new("nginx.conf");
        let lines = format(&errors, path);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0],
            "::error file=nginx.conf,line=10,col=5,title=syntax/missing-semicolon::Missing semicolon at end of directive"
        );
    }

    #[test]
    fn test_warning_format() {
        let errors = vec![make_error(
            "server-tokens-enabled",
            "security",
            "server_tokens is enabled",
            Severity::Warning,
            Some(3),
            Some(1),
        )];
        let path = Path::new("nginx.conf");
        let lines = format(&errors, path);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0],
            "::warning file=nginx.conf,line=3,col=1,title=security/server-tokens-enabled::server_tokens is enabled"
        );
    }

    #[test]
    fn test_without_column() {
        let errors = vec![make_error(
            "indent",
            "style",
            "Wrong indentation",
            Severity::Warning,
            Some(5),
            None,
        )];
        let path = Path::new("nginx.conf");
        let lines = format(&errors, path);
        assert_eq!(
            lines[0],
            "::warning file=nginx.conf,line=5,title=style/indent::Wrong indentation"
        );
    }

    #[test]
    fn test_without_line_and_column() {
        let errors = vec![make_error(
            "some-rule",
            "best-practices",
            "Something is wrong",
            Severity::Error,
            None,
            None,
        )];
        let path = Path::new("conf/nginx.conf");
        let lines = format(&errors, path);
        assert_eq!(
            lines[0],
            "::error file=conf/nginx.conf,title=best-practices/some-rule::Something is wrong"
        );
    }

    #[test]
    fn test_sorted_by_line_and_column() {
        let errors = vec![
            make_error("r1", "cat", "third", Severity::Error, Some(10), Some(1)),
            make_error("r2", "cat", "first", Severity::Warning, Some(1), Some(5)),
            make_error("r3", "cat", "second", Severity::Error, Some(1), Some(10)),
        ];
        let path = Path::new("nginx.conf");
        let lines = format(&errors, path);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("::first"));
        assert!(lines[1].contains("::second"));
        assert!(lines[2].contains("::third"));
    }

    #[test]
    fn test_empty_errors() {
        let errors: Vec<LintError> = vec![];
        let path = Path::new("nginx.conf");
        let lines = format(&errors, path);
        assert!(lines.is_empty());
    }
}
