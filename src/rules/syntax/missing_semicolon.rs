use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use crate::parser::is_raw_block_directive;
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

    fn check(&self, _config: &Config, path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return errors,
        };

        let mut in_string = false;
        let mut string_char: Option<char> = None;
        let mut in_lua_block = false;
        let mut lua_brace_depth = 0;

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

            // Check if we're entering a raw block (like lua_block)
            if !in_lua_block && is_raw_block_line(trimmed) {
                in_lua_block = true;
                lua_brace_depth = 1;
                continue;
            }

            // Track brace depth inside lua_block
            if in_lua_block {
                for ch in trimmed.chars() {
                    if ch == '{' {
                        lua_brace_depth += 1;
                    } else if ch == '}' {
                        lua_brace_depth -= 1;
                        if lua_brace_depth == 0 {
                            in_lua_block = false;
                        }
                    }
                }
                // Skip all lines inside lua_block
                if in_lua_block || trimmed == "}" {
                    continue;
                }
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
                } else if let Some(quote) = string_char
                    && ch == quote
                {
                    string_char = None;
                    in_string = false;
                }
            }

            // Skip if we're in a multi-line string
            if in_string {
                continue;
            }

            // Strip inline comments before checking for semicolon
            let code_part = strip_inline_comment(trimmed);
            let code_part = code_part.trim();

            // Skip if the line is empty after stripping comments
            if code_part.is_empty() {
                continue;
            }

            // Check if line ends with semicolon
            if !code_part.ends_with(';') {
                // This line looks like a directive but doesn't end with semicolon
                // Make sure it's not just a value continuation or include pattern
                if looks_like_directive(code_part) {
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

/// Check if a line starts a raw block directive (like lua_block)
fn is_raw_block_line(line: &str) -> bool {
    // Extract the first word (directive name) from the line
    let directive_name = line.split_whitespace().next().unwrap_or("");

    // Check if it's a raw block directive and the line contains an opening brace
    is_raw_block_directive(directive_name) && line.contains('{')
}

/// Strip inline comments from a line, respecting string literals
fn strip_inline_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut string_char: Option<char> = None;
    let mut prev_char: Option<char> = None;

    for (i, ch) in line.char_indices() {
        // Handle string quotes (skip if escaped)
        if (ch == '"' || ch == '\'') && prev_char != Some('\\') {
            if string_char.is_none() {
                string_char = Some(ch);
                in_string = true;
            } else if string_char == Some(ch) {
                string_char = None;
                in_string = false;
            }
        }

        // Check for comment start (only if not in a string)
        if ch == '#' && !in_string {
            return &line[..i];
        }

        prev_char = Some(ch);
    }

    line
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
    if !first_word
        .chars()
        .next()
        .map(|c| c.is_alphabetic() || c == '_')
        .unwrap_or(false)
    {
        return false;
    }

    // Must have content (not just a single word that could be something else)
    // Single word directives like "internal" still need semicolons
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::Config;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn check_content(content: &str) -> Vec<LintError> {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        let path = file.path().to_path_buf();

        let rule = MissingSemicolon;
        let config = Config::new();
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

    #[test]
    fn test_semicolon_in_string() {
        // Semicolon inside string, but line ends with proper semicolon
        let content = r#"http {
    server {
        return 200 "hello; world";
    }
}
"#;
        let errors = check_content(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_semicolon_in_string_no_trailing() {
        // Semicolon inside string but no trailing semicolon - should error
        let content = r#"http {
    server {
        return 200 "hello; world"
    }
}
"#;
        let errors = check_content(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_string_ending_with_semicolon() {
        // String content ends with semicolon but line doesn't
        let content = r#"http {
    server {
        return 200 "test;"
    }
}
"#;
        let errors = check_content(content);
        assert_eq!(
            errors.len(),
            1,
            "Expected 1 error - string ending with ; is not a real semicolon"
        );
    }

    #[test]
    fn test_comment_ending_with_semicolon() {
        // Comment ends with semicolon but directive doesn't have one
        let content = r#"http {
    server {
        listen 80  # port number;
    }
}
"#;
        let errors = check_content(content);
        assert_eq!(
            errors.len(),
            1,
            "Expected 1 error - comment ending with ; should not count"
        );
    }

    #[test]
    fn test_lua_block_ignored() {
        // Lua code inside lua_block should not trigger missing semicolon errors
        let content = r#"http {
    server {
        listen 80;
        content_by_lua_block {
            local cjson = require "cjson"
            ngx.say(cjson.encode({status = "ok"}))
        }
    }
}
"#;
        let errors = check_content(content);
        assert!(
            errors.is_empty(),
            "Expected no errors in lua_block, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_multiple_lua_blocks_ignored() {
        let content = r#"http {
    init_by_lua_block {
        require "resty.core"
        cjson = require "cjson"
    }

    server {
        listen 80;
        access_by_lua_block {
            local token = ngx.var.http_authorization
            if not token then
                ngx.exit(ngx.HTTP_UNAUTHORIZED)
            end
        }
    }
}
"#;
        let errors = check_content(content);
        assert!(
            errors.is_empty(),
            "Expected no errors in lua_blocks, got: {:?}",
            errors
        );
    }
}
