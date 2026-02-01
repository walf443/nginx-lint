use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use crate::parser::is_raw_block_directive;
use std::fs;
use std::path::Path;

/// Check for inconsistent indentation
pub struct Indent {
    /// Expected spaces per indent level (default: 2)
    pub indent_size: usize,
}

impl Default for Indent {
    fn default() -> Self {
        Self { indent_size: 2 }
    }
}

impl Indent {
    /// Check indentation on content string directly (used by WASM)
    pub fn check_content(&self, content: &str) -> Vec<LintError> {
        self.check_content_impl(content)
    }

    fn check_content_impl(&self, content: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let mut expected_depth: i32 = 0;
        let mut detected_indent_size: Option<usize> = None;
        let mut in_raw_block = false;
        let mut raw_block_brace_depth = 0;
        let mut in_multiline_string = false;
        let mut string_char: Option<char> = None;

        for (line_num, line) in content.lines().enumerate() {
            let line_number = line_num + 1;
            let trimmed = line.trim();

            // Skip empty lines
            if trimmed.is_empty() {
                continue;
            }

            // Check indentation for comments but don't adjust depth
            if trimmed.starts_with('#') {
                check_line_indentation(
                    &mut errors,
                    self.name(),
                    self.category(),
                    line,
                    trimmed,
                    line_number,
                    expected_depth,
                    &mut detected_indent_size,
                    self.indent_size,
                );
                continue;
            }

            // Check if we're entering a raw block (like lua_block)
            if !in_raw_block && !in_multiline_string && is_raw_block_line(trimmed) {
                in_raw_block = true;
                raw_block_brace_depth = 1;
                // Still check indentation for the opening line
                check_line_indentation(
                    &mut errors,
                    self.name(),
                    self.category(),
                    line,
                    trimmed,
                    line_number,
                    expected_depth,
                    &mut detected_indent_size,
                    self.indent_size,
                );
                expected_depth += 1;
                continue;
            }

            // Track brace depth inside raw_block
            if in_raw_block {
                for ch in trimmed.chars() {
                    if ch == '{' {
                        raw_block_brace_depth += 1;
                    } else if ch == '}' {
                        raw_block_brace_depth -= 1;
                        if raw_block_brace_depth == 0 {
                            in_raw_block = false;
                        }
                    }
                }
                // Skip indentation check for lines inside raw_block
                if in_raw_block {
                    continue;
                }
                // Handle the closing brace line of raw_block
                if trimmed == "}" {
                    expected_depth -= 1;
                    check_line_indentation(
                        &mut errors,
                        self.name(),
                        self.category(),
                        line,
                        trimmed,
                        line_number,
                        expected_depth,
                        &mut detected_indent_size,
                        self.indent_size,
                    );
                    continue;
                }
            }

            // Track multiline strings (for mruby inline code etc.)
            if !in_raw_block {
                let (still_in_string, new_string_char) =
                    track_multiline_string(trimmed, in_multiline_string, string_char);
                if in_multiline_string && still_in_string {
                    // Skip lines inside multiline strings
                    in_multiline_string = still_in_string;
                    string_char = new_string_char;
                    continue;
                }
                in_multiline_string = still_in_string;
                string_char = new_string_char;
            }

            // Handle closing brace - adjust depth before checking
            let closes_block = trimmed.starts_with('}');
            if closes_block {
                expected_depth -= 1;
            }

            check_line_indentation(
                &mut errors,
                self.name(),
                self.category(),
                line,
                trimmed,
                line_number,
                expected_depth,
                &mut detected_indent_size,
                self.indent_size,
            );

            // Adjust expected depth after checking if line ends with {
            if trimmed.ends_with('{') {
                expected_depth += 1;
            }
        }

        errors
    }

    fn name(&self) -> &'static str {
        "indent"
    }

    fn category(&self) -> &'static str {
        "style"
    }
}

impl LintRule for Indent {
    fn name(&self) -> &'static str {
        "indent"
    }

    fn category(&self) -> &'static str {
        "style"
    }

    fn description(&self) -> &'static str {
        "Detects inconsistent indentation in nginx configuration"
    }

    fn check(&self, _config: &Config, path: &Path) -> Vec<LintError> {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        self.check_content_impl(&content)
    }
}

/// Check if a line starts a raw block directive (like lua_block)
fn is_raw_block_line(line: &str) -> bool {
    let directive_name = line.split_whitespace().next().unwrap_or("");
    is_raw_block_directive(directive_name) && line.contains('{')
}

/// Track multiline string state
/// Returns (still_in_string, string_char)
fn track_multiline_string(
    line: &str,
    was_in_string: bool,
    prev_string_char: Option<char>,
) -> (bool, Option<char>) {
    let mut in_string = was_in_string;
    let mut current_char = prev_string_char;
    let mut prev_ch = None;

    for ch in line.chars() {
        if (ch == '\'' || ch == '"') && prev_ch != Some('\\') {
            if !in_string {
                in_string = true;
                current_char = Some(ch);
            } else if current_char == Some(ch) {
                in_string = false;
                current_char = None;
            }
        }
        prev_ch = Some(ch);
    }

    (in_string, current_char)
}

/// Check indentation for a single line
#[allow(clippy::too_many_arguments)]
fn check_line_indentation(
    errors: &mut Vec<LintError>,
    rule_name: &'static str,
    category: &'static str,
    line: &str,
    trimmed: &str,
    line_number: usize,
    expected_depth: i32,
    _detected_indent_size: &mut Option<usize>,
    default_indent_size: usize,
) {
    // Calculate current indentation
    let leading_spaces = line.len() - line.trim_start().len();

    // Always use default indent size for consistent formatting
    let expected_spaces = (expected_depth.max(0) as usize) * default_indent_size;

    // Detect if line uses tabs
    if line.starts_with('\t') {
        let correct_indent = " ".repeat(expected_spaces);
        let fixed_line = format!("{}{}", correct_indent, trimmed);
        let fix = Fix::replace_line(line_number, &fixed_line);
        errors.push(
            LintError::new(
                rule_name,
                category,
                "Use spaces instead of tabs for indentation",
                Severity::Warning,
            )
            .with_location(line_number, 1)
            .with_fix(fix),
        );
        return;
    }

    // Check indentation
    if leading_spaces != expected_spaces {
        let message = format!(
            "Expected {} spaces of indentation, found {}",
            expected_spaces, leading_spaces
        );
        let correct_indent = " ".repeat(expected_spaces);
        let fixed_line = format!("{}{}", correct_indent, trimmed);
        let fix = Fix::replace_line(line_number, &fixed_line);
        errors.push(
            LintError::new(rule_name, category, &message, Severity::Warning)
                .with_location(line_number, 1)
                .with_fix(fix),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::Config;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn check_content(content: &str) -> Vec<LintError> {
        let rule = Indent::default();
        rule.check_content(content)
    }

    fn check_content_with_file(content: &str) -> Vec<LintError> {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        let path = file.path().to_path_buf();

        let rule = Indent::default();
        let config = Config::new();
        rule.check(&config, &path)
    }

    #[test]
    fn test_correct_indentation() {
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
    fn test_wrong_indentation() {
        // Mixed indentation: first level is 2 spaces, but inner content uses 4
        let content = r#"http {
  server {
      listen 80;
  }
}
"#;
        let errors = check_content(content);
        assert!(!errors.is_empty(), "Expected indentation errors");
    }

    #[test]
    fn test_tab_indentation() {
        let content = "http {\n\tserver {\n\t}\n}\n";
        let errors = check_content(content);
        let tab_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("tabs"))
            .collect();
        assert!(!tab_errors.is_empty(), "Expected tab warning");
    }

    #[test]
    fn test_comment_indentation() {
        // Comment with wrong indentation
        let content = r#"http {
  server {
# This comment has wrong indentation
    listen 80;
  }
}
"#;
        let errors = check_content(content);
        assert!(!errors.is_empty(), "Expected indentation error for comment");
        assert!(
            errors.iter().any(|e| e.line == Some(3)),
            "Expected error on line 3 (comment line)"
        );
    }

    #[test]
    fn test_comment_correct_indentation() {
        let content = r#"http {
  server {
    # This comment has correct indentation
    listen 80;
  }
}
"#;
        let errors = check_content(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_file_check_matches_content_check() {
        let content = r#"http {
    server {
        listen 80;
    }
}
"#;
        let content_errors = check_content(content);
        let file_errors = check_content_with_file(content);
        assert_eq!(content_errors.len(), file_errors.len());
    }
}
