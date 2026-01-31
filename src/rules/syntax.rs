use crate::linter::{LintError, LintRule, Severity};
use nginx_config::ast::Main;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Check for duplicate directives that should only appear once
pub struct DuplicateDirective;

impl LintRule for DuplicateDirective {
    fn name(&self) -> &'static str {
        "duplicate-directive"
    }

    fn description(&self) -> &'static str {
        "Detects duplicate directives that should only appear once in a context"
    }

    fn check(&self, config: &Main, _path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        // Directives that should only appear once in main context
        let unique_directives = [
            "worker_processes",
            "pid",
            "error_log",
        ];

        // Check main context
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for directive in &config.directives {
            let name = directive.item.directive_name();
            if unique_directives.contains(&name) {
                let count = seen.entry(name).or_insert(0);
                *count += 1;
                if *count > 1 {
                    errors.push(
                        LintError::new(
                            self.name(),
                            &format!("Duplicate directive '{}' in main context", name),
                            Severity::Warning,
                        )
                        .with_location(directive.position.line, directive.position.column),
                    );
                }
            }
        }

        errors
    }
}

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
        let mut in_string = false;
        let mut in_comment = false;

        for (line_num, line) in content.lines().enumerate() {
            let line_number = line_num + 1;
            let mut chars = line.chars().enumerate().peekable();

            while let Some((col, ch)) = chars.next() {
                let column = col + 1;

                // Handle comments
                if ch == '#' && !in_string {
                    in_comment = true;
                }

                if in_comment {
                    continue;
                }

                // Handle strings (simple handling for single and double quotes)
                if (ch == '"' || ch == '\'') && !in_string {
                    in_string = true;
                    continue;
                }
                if (ch == '"' || ch == '\'') && in_string {
                    in_string = false;
                    continue;
                }

                if in_string {
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
            }

            // Reset comment flag at end of line
            in_comment = false;
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
}
