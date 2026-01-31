use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::fs;
use std::path::Path;

/// Check for unmatched braces
pub struct UnmatchedBraces;

/// Information about an opening brace
#[derive(Debug, Clone)]
struct BraceInfo {
    line: usize,
    column: usize,
    indent: usize,
}

impl LintRule for UnmatchedBraces {
    fn name(&self) -> &'static str {
        "unmatched-braces"
    }

    fn category(&self) -> &'static str {
        "syntax"
    }

    fn description(&self) -> &'static str {
        "Detects unmatched opening or closing braces"
    }

    fn check(&self, _config: &Config, path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return errors,
        };

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let mut brace_stack: Vec<BraceInfo> = Vec::new();
        let mut string_char: Option<char> = None;
        let mut in_comment = false;
        let mut prev_char = ' ';

        // Track where to insert missing closing braces
        // Maps line number to the indentation needed for closing brace
        let mut missing_close_braces: Vec<(usize, usize, usize)> = Vec::new(); // (insert_after_line, indent, error_line)

        // Track comment lines before which closing braces should be inserted
        // (insert_before_line, indent, error_line)
        let mut comment_insertions: Vec<(usize, usize, usize)> = Vec::new();

        for (line_num, line) in lines.iter().enumerate() {
            let line_number = line_num + 1;
            let chars: Vec<char> = line.chars().collect();
            let line_indent = line.len() - line.trim_start().len();
            let trimmed = line.trim_start();

            // Check if this is a comment-only line
            if trimmed.starts_with('#') {
                // Check if any unclosed brace should be closed at this comment's indent
                while let Some(top) = brace_stack.last() {
                    if top.indent > line_indent {
                        // This block should be closed before this comment
                        let unclosed = brace_stack.pop().unwrap();
                        missing_close_braces.push((
                            line_number - 1,
                            unclosed.indent,
                            unclosed.line,
                        ));
                        errors.push(
                            LintError::new(
                                self.name(),
                                self.category(),
                                "Unclosed brace '{' - missing closing brace '}'",
                                Severity::Error,
                            )
                            .with_location(unclosed.line, unclosed.column),
                        );
                    } else if top.indent == line_indent {
                        // Insert closing brace before this comment line
                        let unclosed = brace_stack.pop().unwrap();
                        comment_insertions.push((line_number - 1, unclosed.indent, unclosed.line));
                        errors.push(
                            LintError::new(
                                self.name(),
                                self.category(),
                                "Unclosed brace '{' - missing closing brace '}'",
                                Severity::Error,
                            )
                            .with_location(unclosed.line, unclosed.column),
                        );
                    } else {
                        break;
                    }
                }
                continue; // Skip character processing for comment lines
            }

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
                    brace_stack.push(BraceInfo {
                        line: line_number,
                        column,
                        indent: line_indent,
                    });
                } else if ch == '}' {
                    // Check if this closing brace matches the expected indent
                    let close_indent = line_indent;
                    let mut found_match = false;

                    // Pop blocks that should have been closed before this one
                    while let Some(top) = brace_stack.last() {
                        if top.indent > close_indent {
                            // This block was never closed - needs a closing brace
                            let unclosed = brace_stack.pop().unwrap();
                            // Insert before current line
                            missing_close_braces.push((
                                line_number - 1,
                                unclosed.indent,
                                unclosed.line,
                            ));
                            // Add error for unclosed brace
                            errors.push(
                                LintError::new(
                                    self.name(),
                                    self.category(),
                                    "Unclosed brace '{' - missing closing brace '}'",
                                    Severity::Error,
                                )
                                .with_location(unclosed.line, unclosed.column),
                            );
                        } else if top.indent == close_indent {
                            // This closing brace matches the top block
                            brace_stack.pop();
                            found_match = true;
                            break;
                        } else {
                            // close_indent > top.indent - unexpected closing brace
                            break;
                        }
                    }

                    if !found_match {
                        // Extra closing brace - no matching opening brace
                        let fix = if line.trim() == "}" {
                            Some(Fix::delete(line_number))
                        } else {
                            None
                        };
                        let mut error = LintError::new(
                            self.name(),
                            self.category(),
                            "Unexpected closing brace '}' without matching opening brace",
                            Severity::Error,
                        )
                        .with_location(line_number, column);
                        if let Some(f) = fix {
                            error = error.with_fix(f);
                        }
                        errors.push(error);
                    }
                }

                prev_char = ch;
            }

            // Reset comment flag at end of line
            in_comment = false;
            prev_char = ' ';
        }

        // Remaining unclosed braces - add closing braces at end of file
        while let Some(unclosed) = brace_stack.pop() {
            missing_close_braces.push((total_lines, unclosed.indent, unclosed.line));
            errors.push(
                LintError::new(
                    self.name(),
                    self.category(),
                    "Unclosed brace '{' - missing closing brace '}'",
                    Severity::Error,
                )
                .with_location(unclosed.line, unclosed.column),
            );
        }

        // Merge comment insertions into missing_close_braces (both use insert_after)
        missing_close_braces.extend(comment_insertions);

        // Create fixes for missing closing braces (insert after)
        // Sort by insert line descending, then by indent ascending
        // (outer blocks should be inserted first so they end up at the bottom)
        missing_close_braces.sort_by(|a, b| match b.0.cmp(&a.0) {
            std::cmp::Ordering::Equal => a.1.cmp(&b.1),
            other => other,
        });

        for (insert_after_line, indent, error_line) in missing_close_braces {
            let closing_brace = format!("{}}}", " ".repeat(indent));
            // Find the corresponding error by matching the error's line number
            for error in &mut errors {
                if error.fix.is_none()
                    && error.message.contains("Unclosed brace")
                    && error.line == Some(error_line)
                {
                    error.fix = Some(Fix::insert_after(insert_after_line, &closing_brace));
                    break;
                }
            }
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::Config;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn check_braces(content: &str) -> Vec<LintError> {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        let path = file.path().to_path_buf();

        let rule = UnmatchedBraces;
        let config = Config::new();
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
