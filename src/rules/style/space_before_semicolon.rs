use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::fs;
use std::path::Path;

/// Check for spaces or tabs before semicolons
pub struct SpaceBeforeSemicolon;

impl SpaceBeforeSemicolon {
    /// Check space before semicolon on content string directly (used by WASM)
    pub fn check_content(&self, content: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            let line_number = line_num + 1;

            // Skip comment-only lines
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }

            // Find semicolons and check for preceding whitespace
            let chars: Vec<char> = line.chars().collect();
            for (i, &ch) in chars.iter().enumerate() {
                if ch == ';' && i > 0 {
                    // Check if there's whitespace before the semicolon
                    let prev_char = chars[i - 1];
                    if prev_char == ' ' || prev_char == '\t' {
                        // Find the start of the whitespace sequence
                        let mut ws_start = i - 1;
                        while ws_start > 0 {
                            let c = chars[ws_start - 1];
                            if c == ' ' || c == '\t' {
                                ws_start -= 1;
                            } else {
                                break;
                            }
                        }

                        // Create fixed line by removing whitespace before semicolon
                        let before_ws: String = chars[..ws_start].iter().collect();
                        let from_semicolon: String = chars[i..].iter().collect();
                        let fixed_line = format!("{}{}", before_ws, from_semicolon);

                        errors.push(
                            LintError::new(
                                self.name(),
                                self.category(),
                                "space before semicolon",
                                Severity::Warning,
                            )
                            .with_location(line_number, ws_start + 1)
                            .with_fix(Fix::replace_line(line_number, &fixed_line)),
                        );

                        // Only report once per line (first occurrence)
                        break;
                    }
                }
            }
        }

        errors
    }
}

impl LintRule for SpaceBeforeSemicolon {
    fn name(&self) -> &'static str {
        "space-before-semicolon"
    }

    fn description(&self) -> &'static str {
        "Check for spaces or tabs before semicolons"
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
    fn test_no_space_before_semicolon() {
        let content = r#"http {
    server {
        listen 80;
        server_name example.com;
    }
}"#;
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_single_space_before_semicolon() {
        let content = "listen 80 ;";
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, Some(1));
        assert_eq!(errors[0].column, Some(10)); // Position of the space
        let fix = errors[0].fix.as_ref().unwrap();
        assert_eq!(fix.new_text, "listen 80;");
    }

    #[test]
    fn test_multiple_spaces_before_semicolon() {
        let content = "listen 80   ;";
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1);
        let fix = errors[0].fix.as_ref().unwrap();
        assert_eq!(fix.new_text, "listen 80;");
    }

    #[test]
    fn test_tab_before_semicolon() {
        let content = "listen 80\t;";
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1);
        let fix = errors[0].fix.as_ref().unwrap();
        assert_eq!(fix.new_text, "listen 80;");
    }

    #[test]
    fn test_mixed_whitespace_before_semicolon() {
        let content = "listen 80 \t ;";
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1);
        let fix = errors[0].fix.as_ref().unwrap();
        assert_eq!(fix.new_text, "listen 80;");
    }

    #[test]
    fn test_multiple_lines_with_space_before_semicolon() {
        let content = "listen 80 ;\nserver_name example.com ;";
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].line, Some(1));
        assert_eq!(errors[1].line, Some(2));
    }

    #[test]
    fn test_comment_line_ignored() {
        let content = "# comment with space before semicolon ;";
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_semicolon_in_quoted_string() {
        // Semicolon in quoted string should still be checked if space before it
        // This is technically valid in nginx for regex patterns
        let content = r#"rewrite "^/test" /new ;"#;
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn test_preserves_indentation() {
        let content = "    listen 80 ;";
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1);
        let fix = errors[0].fix.as_ref().unwrap();
        assert_eq!(fix.new_text, "    listen 80;");
    }
}
