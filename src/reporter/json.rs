use crate::LintError;
use crate::Severity;
use std::path::Path;

#[derive(serde::Serialize)]
struct JsonReport {
    file: String,
    errors: Vec<LintError>,
    summary: Summary,
}

#[derive(serde::Serialize)]
struct Summary {
    errors: usize,
    warnings: usize,
    ignored: usize,
}

pub(crate) fn report(errors: &[LintError], path: &Path, ignored_count: usize) {
    println!("{}", format(errors, path, ignored_count));
}

pub(crate) fn format(errors: &[LintError], path: &Path, ignored_count: usize) -> String {
    // Sort errors by line number, then by column number
    let mut sorted_errors: Vec<_> = errors.to_vec();
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

    let report = JsonReport {
        file: path.display().to_string(),
        errors: sorted_errors,
        summary: Summary {
            errors: errors
                .iter()
                .filter(|e| e.severity == Severity::Error)
                .count(),
            warnings: errors
                .iter()
                .filter(|e| e.severity == Severity::Warning)
                .count(),
            ignored: ignored_count,
        },
    };

    serde_json::to_string_pretty(&report).unwrap()
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
    fn test_json_structure() {
        let errors = vec![make_error(
            "missing-semicolon",
            "syntax",
            "Missing semicolon",
            Severity::Error,
            Some(10),
            Some(5),
        )];
        let path = Path::new("nginx.conf");
        let output = format(&errors, path, 0);
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(json["file"], "nginx.conf");
        assert_eq!(json["errors"].as_array().unwrap().len(), 1);
        assert_eq!(json["errors"][0]["rule"], "missing-semicolon");
        assert_eq!(json["errors"][0]["category"], "syntax");
        assert_eq!(json["errors"][0]["message"], "Missing semicolon");
        assert_eq!(json["errors"][0]["severity"], "Error");
        assert_eq!(json["errors"][0]["line"], 10);
        assert_eq!(json["errors"][0]["column"], 5);
        assert_eq!(json["summary"]["errors"], 1);
        assert_eq!(json["summary"]["warnings"], 0);
        assert_eq!(json["summary"]["ignored"], 0);
    }

    #[test]
    fn test_json_summary_counts() {
        let errors = vec![
            make_error("r1", "cat", "err", Severity::Error, Some(1), None),
            make_error("r2", "cat", "warn1", Severity::Warning, Some(2), None),
            make_error("r3", "cat", "warn2", Severity::Warning, Some(3), None),
        ];
        let path = Path::new("nginx.conf");
        let output = format(&errors, path, 2);
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(json["summary"]["errors"], 1);
        assert_eq!(json["summary"]["warnings"], 2);
        assert_eq!(json["summary"]["ignored"], 2);
    }

    #[test]
    fn test_json_sorted_by_line_and_column() {
        let errors = vec![
            make_error("r1", "cat", "third", Severity::Error, Some(10), Some(1)),
            make_error("r2", "cat", "first", Severity::Warning, Some(1), Some(5)),
            make_error("r3", "cat", "second", Severity::Error, Some(1), Some(10)),
        ];
        let path = Path::new("nginx.conf");
        let output = format(&errors, path, 0);
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();

        let errs = json["errors"].as_array().unwrap();
        assert_eq!(errs[0]["message"], "first");
        assert_eq!(errs[1]["message"], "second");
        assert_eq!(errs[2]["message"], "third");
    }

    #[test]
    fn test_json_empty_errors() {
        let errors: Vec<LintError> = vec![];
        let path = Path::new("nginx.conf");
        let output = format(&errors, path, 0);
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(json["file"], "nginx.conf");
        assert!(json["errors"].as_array().unwrap().is_empty());
        assert_eq!(json["summary"]["errors"], 0);
        assert_eq!(json["summary"]["warnings"], 0);
    }
}
