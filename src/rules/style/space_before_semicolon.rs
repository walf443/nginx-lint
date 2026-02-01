use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use crate::parser::is_raw_block_directive;
use std::fs;
use std::path::Path;

/// Check for spaces or tabs before semicolons
pub struct SpaceBeforeSemicolon;

impl SpaceBeforeSemicolon {
    /// Check space before semicolon on content string directly (used by WASM)
    pub fn check_content(&self, content: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let mut raw_block_depth = 0; // Track nested braces inside raw blocks
        let mut in_raw_block = false;

        for (line_num, line) in content.lines().enumerate() {
            let line_number = line_num + 1;

            // Skip comment-only lines
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }

            // Check if this line starts a raw block (like content_by_lua_block {)
            if !in_raw_block {
                if let Some(first_word) = trimmed.split_whitespace().next() {
                    if is_raw_block_directive(first_word) && trimmed.contains('{') {
                        in_raw_block = true;
                        raw_block_depth = 1;
                        // Count additional braces on the same line
                        for ch in trimmed.chars() {
                            if ch == '{' {
                                // Already counted the first one
                            } else if ch == '}' {
                                raw_block_depth -= 1;
                                if raw_block_depth == 0 {
                                    in_raw_block = false;
                                    break;
                                }
                            }
                        }
                        // Skip the opening brace
                        raw_block_depth = trimmed.chars().filter(|&c| c == '{').count() as i32
                            - trimmed.chars().filter(|&c| c == '}').count() as i32;
                        if raw_block_depth <= 0 {
                            in_raw_block = false;
                            raw_block_depth = 0;
                        }
                        continue;
                    }
                }
            }

            // If we're inside a raw block, track braces and skip semicolon checks
            if in_raw_block {
                for ch in trimmed.chars() {
                    if ch == '{' {
                        raw_block_depth += 1;
                    } else if ch == '}' {
                        raw_block_depth -= 1;
                        if raw_block_depth <= 0 {
                            in_raw_block = false;
                            raw_block_depth = 0;
                            break;
                        }
                    }
                }
                continue;
            }

            // Find the statement-ending semicolon (not in quotes or comments)
            let chars: Vec<char> = line.chars().collect();
            let mut in_single_quote = false;
            let mut in_double_quote = false;
            let mut prev_char = '\0';

            for (i, &ch) in chars.iter().enumerate() {
                // Handle escape sequences
                if prev_char == '\\' {
                    prev_char = ch;
                    continue;
                }

                // Track quote state
                if ch == '\'' && !in_double_quote {
                    in_single_quote = !in_single_quote;
                } else if ch == '"' && !in_single_quote {
                    in_double_quote = !in_double_quote;
                }
                // Stop at comment start (outside quotes)
                else if ch == '#' && !in_single_quote && !in_double_quote {
                    break;
                }
                // Check semicolon outside quotes
                else if ch == ';' && !in_single_quote && !in_double_quote && i > 0 {
                    let prev = chars[i - 1];
                    if prev == ' ' || prev == '\t' {
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

                        // Only report once per line (first occurrence outside quotes)
                        break;
                    }
                }

                prev_char = ch;
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
    fn test_semicolon_in_quoted_string_ignored() {
        // Semicolon inside quoted string should be ignored
        let content = r#"return 200 "hello ; world";"#;
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_space_before_semicolon_with_quoted_string() {
        // Space before statement-ending semicolon should be detected
        let content = r#"return 200 "hello" ;"#;
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1);
        let fix = errors[0].fix.as_ref().unwrap();
        assert_eq!(fix.new_text, r#"return 200 "hello";"#);
    }

    #[test]
    fn test_inline_comment_with_semicolon() {
        // Semicolon in inline comment should be ignored
        let content = "listen 80; # comment with ; here";
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_single_quoted_string() {
        // Semicolon inside single-quoted string should be ignored
        let content = "set $var 'value ; test';";
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert!(errors.is_empty());
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

    #[test]
    fn test_lua_block_ignored() {
        // Semicolons inside Lua blocks should be ignored
        let content = r#"location / {
    content_by_lua_block {
        local a = 1 ;
        ngx.say("hello ; world")
    }
}"#;
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_lua_block_with_error_after() {
        // Error after Lua block should be detected
        let content = r#"location / {
    content_by_lua_block {
        local a = 1;
    }
    return 200 ;
}"#;
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, Some(5));
    }

    #[test]
    fn test_access_by_lua_block_ignored() {
        let content = r#"access_by_lua_block {
    local x = 1 ;
}"#;
        let rule = SpaceBeforeSemicolon;
        let errors = rule.check_content(content);
        assert!(errors.is_empty());
    }
}
