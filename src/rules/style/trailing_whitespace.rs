use crate::docs::RuleDoc;
use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::fs;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "trailing-whitespace",
    category: "style",
    description: "Detects trailing whitespace at the end of lines",
    severity: "warning",
    why: r#"Trailing whitespace is invisible and can cause unnecessary diffs
in version control and hinder code reviews.

Removing trailing whitespace keeps configuration files clean."#,
    bad_example: include_str!("trailing_whitespace/bad.conf"),
    good_example: include_str!("trailing_whitespace/good.conf"),
    references: &[],
};

/// Check for trailing whitespace at the end of lines
pub struct TrailingWhitespace;

impl TrailingWhitespace {
    /// Check trailing whitespace on content string directly (used by WASM)
    pub fn check_content(&self, content: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            let line_number = line_num + 1;

            // Check if line has trailing whitespace
            if let Some(last_char) = line.chars().last() {
                if last_char == ' ' || last_char == '\t' {
                    // Find the position where trailing whitespace starts
                    let trimmed_end = line.trim_end();
                    let trailing_start = trimmed_end.len();

                    errors.push(
                        LintError::new(
                            self.name(),
                            self.category(),
                            "trailing whitespace at end of line",
                            Severity::Warning,
                        )
                        .with_location(line_number, trailing_start + 1)
                        .with_fix(Fix::replace_line(line_number, trimmed_end)),
                    );
                }
            }
        }

        errors
    }
}

impl LintRule for TrailingWhitespace {
    fn name(&self) -> &'static str {
        "trailing-whitespace"
    }

    fn description(&self) -> &'static str {
        "Check for trailing whitespace at the end of lines"
    }

    fn category(&self) -> &'static str {
        "style"
    }

    fn check(&self, _config: &Config, path: &Path) -> Vec<LintError> {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        self.check_content(&content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_trailing_whitespace() {
        let content = r#"http {
    server {
        listen 80;
    }
}"#;
        let rule = TrailingWhitespace;
        let errors = rule.check_content(content);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_trailing_spaces() {
        let content = "http {  \n    server {\n    }\n}";
        let rule = TrailingWhitespace;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, Some(1));
        assert_eq!(errors[0].column, Some(7)); // Position after "http {"
    }

    #[test]
    fn test_trailing_tabs() {
        let content = "http {\t\n    server {\n    }\n}";
        let rule = TrailingWhitespace;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, Some(1));
    }

    #[test]
    fn test_multiple_lines_with_trailing_whitespace() {
        let content = "http {  \n    server {  \n    }\n}";
        let rule = TrailingWhitespace;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].line, Some(1));
        assert_eq!(errors[1].line, Some(2));
    }

    #[test]
    fn test_fix_removes_trailing_whitespace() {
        let content = "http {  ";
        let rule = TrailingWhitespace;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1);
        let fix = errors[0].fix.as_ref().unwrap();
        assert_eq!(fix.new_text, "http {");
    }

    #[test]
    fn test_empty_line_no_error() {
        let content = "http {\n\n    server {\n    }\n}";
        let rule = TrailingWhitespace;
        let errors = rule.check_content(content);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_line_with_only_whitespace() {
        let content = "http {\n   \n    server {\n    }\n}";
        let rule = TrailingWhitespace;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, Some(2));
        let fix = errors[0].fix.as_ref().unwrap();
        assert_eq!(fix.new_text, ""); // Empty line after fix
    }
}
