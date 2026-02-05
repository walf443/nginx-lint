use crate::config::IndentSize;
use crate::docs::RuleDoc;
use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use crate::parser::is_raw_block_directive;
use std::fs;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "indent",
    category: "style",
    description: "Detects inconsistent indentation",
    severity: "warning",
    why: r#"Consistent indentation improves readability of configuration files.
Properly indented nested blocks make the structure visually clear
and easier to understand.

Using spaces instead of tabs ensures consistent appearance
across different environments."#,
    bad_example: include_str!("indent/bad.conf"),
    good_example: include_str!("indent/good.conf"),
    references: &[],
};

/// Check for inconsistent indentation
pub struct Indent {
    /// Indent size configuration: fixed number or auto-detect
    pub indent_size: IndentSize,
}

impl Default for Indent {
    fn default() -> Self {
        Self {
            indent_size: IndentSize::Auto,
        }
    }
}

impl Indent {
    /// Create with a specific indent size
    pub fn with_size(size: usize) -> Self {
        Self {
            indent_size: IndentSize::Fixed(size),
        }
    }

    /// Create with auto-detection
    pub fn auto() -> Self {
        Self {
            indent_size: IndentSize::Auto,
        }
    }
}

impl Indent {
    /// Check indentation on content string directly (used by WASM)
    pub fn check_content(&self, content: &str) -> Vec<LintError> {
        self.check_content_impl(content)
    }

    /// Detect indent size from the first indented line in the content
    fn detect_indent_size(content: &str) -> Option<usize> {
        let mut depth = 0;
        for line in content.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Check indentation BEFORE updating depth
            // We want the first line that is indented at depth 1
            if depth == 1 {
                let leading_spaces = line.len() - line.trim_start().len();
                if leading_spaces > 0 && !line.starts_with('\t') {
                    // Found the first indented line at depth 1
                    return Some(leading_spaces);
                }
            }

            // Update depth based on braces
            if trimmed.ends_with('{') {
                depth += 1;
            }
            if trimmed.starts_with('}') {
                depth -= 1;
            }
        }
        None
    }

    /// Get the effective indent size (auto-detected or fixed)
    fn effective_indent_size(&self, content: &str) -> usize {
        match self.indent_size {
            IndentSize::Fixed(size) => size,
            IndentSize::Auto => Self::detect_indent_size(content).unwrap_or(2),
        }
    }

    fn check_content_impl(&self, content: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let mut expected_depth: i32 = 0;
        let indent_size = self.effective_indent_size(content);
        let mut in_raw_block = false;
        let mut raw_block_brace_depth = 0;
        let mut in_multiline_string = false;
        let mut string_char: Option<char> = None;
        let mut line_start_offset: usize = 0;

        for (line_num, line) in content.lines().enumerate() {
            let line_number = line_num + 1;
            let trimmed = line.trim();
            let current_line_offset = line_start_offset;

            // Update offset for next line (line length + newline character)
            line_start_offset += line.len() + 1; // +1 for '\n'

            // Skip empty lines
            if trimmed.is_empty() {
                continue;
            }

            // Track brace depth inside raw_block (must be before comment check)
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
                // Skip indentation check for lines inside raw_block (including comments)
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
                        line_number,
                        current_line_offset,
                        expected_depth,
                        indent_size,
                    );
                    continue;
                }
            }

            // Check indentation for comments but don't adjust depth
            if trimmed.starts_with('#') {
                check_line_indentation(
                    &mut errors,
                    self.name(),
                    self.category(),
                    line,
                    line_number,
                    current_line_offset,
                    expected_depth,
                    indent_size,
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
                    line_number,
                    current_line_offset,
                    expected_depth,
                    indent_size,
                );
                expected_depth += 1;
                continue;
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
                line_number,
                current_line_offset,
                expected_depth,
                indent_size,
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
fn check_line_indentation(
    errors: &mut Vec<LintError>,
    rule_name: &'static str,
    category: &'static str,
    line: &str,
    line_number: usize,
    line_start_offset: usize,
    expected_depth: i32,
    indent_size: usize,
) {
    // Calculate current indentation
    let leading_spaces = line.len() - line.trim_start().len();

    let expected_spaces = (expected_depth.max(0) as usize) * indent_size;

    // Detect if line uses tabs
    if line.starts_with('\t') {
        let correct_indent = " ".repeat(expected_spaces);
        // Use range-based fix to replace only the leading whitespace
        let fix = Fix::replace_range(line_start_offset, line_start_offset + leading_spaces, &correct_indent);
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
        // Use range-based fix to replace only the leading whitespace
        let fix = Fix::replace_range(line_start_offset, line_start_offset + leading_spaces, &correct_indent);
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
    fn test_lua_block_content_not_checked() {
        // Lua block content should not be checked for indentation
        let content = r#"http {
  server {
    content_by_lua_block {
-- Lua comment with different indentation
local x = 1
        if x > 0 then
            print("hello")
        end
    }
  }
}
"#;
        let errors = check_content(content);
        // Only the opening and closing braces should be checked
        // Content inside lua_block should be skipped
        assert!(
            errors.is_empty(),
            "Expected no errors for Lua block content, got: {:?}",
            errors
        );
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

    #[test]
    fn test_auto_detect_indent_size_4() {
        // File uses 4-space indentation
        let content = r#"http {
    server {
        listen 80;
    }
}
"#;
        let rule = Indent::auto();
        let errors = rule.check_content(content);
        // With auto-detection, this should have no errors (detected as 4-space indent)
        assert!(errors.is_empty(), "Expected no errors with auto-detection, got: {:?}", errors);
    }

    #[test]
    fn test_auto_detect_indent_size_2() {
        // File uses 2-space indentation
        let content = r#"http {
  server {
    listen 80;
  }
}
"#;
        let rule = Indent::auto();
        let errors = rule.check_content(content);
        assert!(errors.is_empty(), "Expected no errors with auto-detection, got: {:?}", errors);
    }

    #[test]
    fn test_detect_indent_size() {
        // Test the detection function directly
        let content_4 = "http {\n    server {\n    }\n}\n";
        assert_eq!(Indent::detect_indent_size(content_4), Some(4));

        let content_2 = "http {\n  server {\n  }\n}\n";
        assert_eq!(Indent::detect_indent_size(content_2), Some(2));

        let content_tab = "http {\n\tserver {\n\t}\n}\n";
        // Tab indentation returns None (not space-based)
        assert_eq!(Indent::detect_indent_size(content_tab), None);
    }

    #[test]
    fn test_fixed_indent_size() {
        // Using fixed 4-space setting on 2-space indented file should produce errors
        let content = r#"http {
  server {
    listen 80;
  }
}
"#;
        let rule = Indent::with_size(4);
        let errors = rule.check_content(content);
        assert!(!errors.is_empty(), "Expected errors with mismatched indent size");
    }
}
