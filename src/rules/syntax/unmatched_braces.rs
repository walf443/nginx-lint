use crate::linter::{LintError, LintRule, Severity};
use nginx_config::ast::Main;
use std::fs;
use std::path::Path;

/// Check for unmatched braces
pub struct UnmatchedBraces;

impl LintRule for UnmatchedBraces {
    fn name(&self) -> &'static str {
        "unmatched-braces"
    }

    fn description(&self) -> &'static str {
        "Detects unmatched opening or closing braces"
    }

    fn check(&self, _config: &Main, path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return errors,
        };

        let mut brace_stack: Vec<(usize, usize)> = Vec::new(); // (line, column)
        let mut string_char: Option<char> = None; // Track which quote started the string
        let mut in_comment = false;
        let mut prev_char = ' ';

        for (line_num, line) in content.lines().enumerate() {
            let line_number = line_num + 1;
            let chars: Vec<char> = line.chars().collect();

            for (col, &ch) in chars.iter().enumerate() {
                let column = col + 1;

                // Handle comments (only outside strings)
                if ch == '#' && string_char.is_none() {
                    in_comment = true;
                }

                if in_comment {
                    prev_char = ch;
                    continue;
                }

                // Handle strings - track which quote type started it
                if (ch == '"' || ch == '\'') && string_char.is_none() {
                    string_char = Some(ch);
                    prev_char = ch;
                    continue;
                }

                // End string only with matching quote (and not escaped)
                if let Some(quote) = string_char {
                    if ch == quote && prev_char != '\\' {
                        string_char = None;
                    }
                    prev_char = ch;
                    continue;
                }

                // Track braces
                if ch == '{' {
                    brace_stack.push((line_number, column));
                } else if ch == '}' {
                    if brace_stack.pop().is_none() {
                        errors.push(
                            LintError::new(
                                self.name(),
                                "Unexpected closing brace '}' without matching opening brace",
                                Severity::Error,
                            )
                            .with_location(line_number, column),
                        );
                    }
                }

                prev_char = ch;
            }

            // Reset comment flag at end of line
            in_comment = false;
            prev_char = ' ';
        }

        // Report unclosed braces
        for (line, column) in brace_stack {
            errors.push(
                LintError::new(
                    self.name(),
                    "Unclosed brace '{' - missing closing brace '}'",
                    Severity::Error,
                )
                .with_location(line, column),
            );
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn check_braces(content: &str) -> Vec<LintError> {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        let path = file.path().to_path_buf();

        let rule = UnmatchedBraces;
        let config = nginx_config::parse_main("http {}").unwrap();
        rule.check(&config, &path)
    }

    #[test]
    fn test_matched_braces() {
        let content = r#"http {
    server {
        listen 80;
    }
}
"#;
        let errors = check_braces(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_unclosed_brace() {
        let content = r#"http {
    server {
        listen 80;
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error");
        assert!(errors[0].message.contains("Unclosed brace"));
    }

    #[test]
    fn test_extra_closing_brace() {
        let content = r#"http {
    server {
        listen 80;
    }
}
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error");
        assert!(errors[0].message.contains("Unexpected closing brace"));
    }

    #[test]
    fn test_braces_in_comment() {
        let content = r#"http {
    # this { should be ignored }
    server {
        listen 80;
    }
}
"#;
        let errors = check_braces(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_braces_in_string() {
        let content = r#"http {
    server {
        return 200 "{ json }";
    }
}
"#;
        let errors = check_braces(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_braces_in_single_quote_string() {
        let content = r#"http {
    server {
        return 200 '{ json }';
    }
}
"#;
        let errors = check_braces(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_mixed_quotes_with_braces() {
        // Double quote containing single quote and braces
        let content = r#"http {
    server {
        return 200 "it's { working }";
    }
}
"#;
        let errors = check_braces(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_escaped_quote_in_string() {
        // Escaped quote should not end the string
        let content = r#"http {
    server {
        return 200 "hello \"{ world }\"";
    }
}
"#;
        let errors = check_braces(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }
}
