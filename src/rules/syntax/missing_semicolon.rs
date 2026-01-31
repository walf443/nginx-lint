use crate::linter::{LintError, LintRule, Severity};
use nginx_config::ast::Main;
use std::fs;
use std::path::Path;

/// Check for missing semicolons at the end of directives
pub struct MissingSemicolon;

impl LintRule for MissingSemicolon {
    fn name(&self) -> &'static str {
        "missing-semicolon"
    }

    fn description(&self) -> &'static str {
        "Detects missing semicolons at the end of directives"
    }

    fn check(&self, _config: &Main, path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return errors,
        };

        let mut in_string = false;
        let mut string_char: Option<char> = None;

        for (line_num, line) in content.lines().enumerate() {
            let line_number = line_num + 1;
            let trimmed = line.trim();

            // Skip empty lines
            if trimmed.is_empty() {
                continue;
            }

            // Skip comment lines
            if trimmed.starts_with('#') {
                continue;
            }

            // Skip lines that are just closing braces
            if trimmed == "}" {
                continue;
            }

            // Skip lines that end with opening brace (block directives)
            if trimmed.ends_with('{') {
                continue;
            }

            // Skip lines that end with closing brace
            if trimmed.ends_with('}') {
                continue;
            }

            // Check for string state to handle multi-line strings
            for ch in trimmed.chars() {
                if (ch == '"' || ch == '\'') && string_char.is_none() {
                    string_char = Some(ch);
                    in_string = true;
                } else if let Some(quote) = string_char {
                    if ch == quote {
                        string_char = None;
                        in_string = false;
                    }
                }
            }

            // Skip if we're in a multi-line string
            if in_string {
                continue;
            }

            // Check if line ends with semicolon
            if !trimmed.ends_with(';') {
                // This line looks like a directive but doesn't end with semicolon
                // Make sure it's not just a value continuation or include pattern
                if looks_like_directive(trimmed) {
                    errors.push(
                        LintError::new(
                            self.name(),
                            "Missing semicolon at end of directive",
                            Severity::Error,
                        )
                        .with_location(line_number, trimmed.len()),
                    );
                }
            }
        }

        errors
    }
}

/// Check if a line looks like a directive (has a name and potentially arguments)
fn looks_like_directive(line: &str) -> bool {
    // A directive typically starts with an identifier
    let trimmed = line.trim();

    // Must have at least one word
    if trimmed.is_empty() {
        return false;
    }

    // Get the first word
    let first_word = trimmed.split_whitespace().next().unwrap_or("");

    // Must start with a letter or underscore (valid directive name)
    if !first_word.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false) {
        return false;
    }

    // Must have content (not just a single word that could be something else)
    // Single word directives like "internal" still need semicolons
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn check_content(content: &str) -> Vec<LintError> {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        let path = file.path().to_path_buf();

        let rule = MissingSemicolon;
        let config = nginx_config::parse_main("http {}").unwrap();
        rule.check(&config, &path)
    }

    #[test]
    fn test_correct_semicolons() {
        let content = r#"worker_processes auto;

http {
    server {
        listen 80;
        server_name example.com;
    }
}
"#;
        let errors = check_content(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_missing_semicolon() {
        let content = r#"worker_processes auto

http {
    server {
        listen 80;
    }
}
"#;
        let errors = check_content(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("Missing semicolon"));
        assert_eq!(errors[0].line, Some(1));
    }

    #[test]
    fn test_missing_semicolon_in_block() {
        let content = r#"http {
    server {
        listen 80
        server_name example.com;
    }
}
"#;
        let errors = check_content(content);
        assert_eq!(errors.len(), 1, "Expected 1 error");
        assert_eq!(errors[0].line, Some(3));
    }

    #[test]
    fn test_block_directive_no_semicolon_needed() {
        let content = r#"http {
    server {
        listen 80;
    }
}
"#;
        let errors = check_content(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_comment_lines_ignored() {
        let content = r#"# This is a comment without semicolon
worker_processes auto;
"#;
        let errors = check_content(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }
}
