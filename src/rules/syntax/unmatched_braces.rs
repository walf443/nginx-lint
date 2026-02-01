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


impl UnmatchedBraces {
    /// Check content directly (used by WASM)
    pub fn check_content(&self, content: &str) -> Vec<LintError> {
        self.check_content_with_extras(content, &[])
    }

    /// Check content with additional block directives
    pub fn check_content_with_extras(
        &self,
        content: &str,
        additional_block_directives: &[String],
    ) -> Vec<LintError> {
        let mut errors = Vec::new();

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let mut brace_stack: Vec<BraceInfo> = Vec::new();
        let mut string_char: Option<char> = None;
        let mut in_comment = false;
        let mut prev_char = ' ';
        let mut in_raw_block = false;
        let mut raw_block_depth = 0;

        // Track where to insert missing closing braces
        // Maps line number to the indentation needed for closing brace
        let mut missing_close_braces: Vec<(usize, usize, usize)> = Vec::new(); // (insert_after_line, indent, error_line)

        // Track comment lines before which closing braces should be inserted
        // (insert_before_line, indent, error_line)
        let mut comment_insertions: Vec<(usize, usize, usize)> = Vec::new();

        // Track lines where we've already detected a block directive missing '{'
        let mut block_directive_error_lines: Vec<usize> = Vec::new();

        for (line_num, line) in lines.iter().enumerate() {
            let line_number = line_num + 1;
            let chars: Vec<char> = line.chars().collect();
            let line_indent = line.len() - line.trim_start().len();
            let trimmed = line.trim_start();

            // Check if we're entering a raw block (like lua_block)
            if !in_raw_block {
                if let Some(first_word) = trimmed.split_whitespace().next() {
                    if crate::parser::is_raw_block_directive(first_word) && trimmed.contains('{') {
                        in_raw_block = true;
                        raw_block_depth = 1;
                    }
                }
            }

            // Track brace depth inside raw blocks
            if in_raw_block {
                for ch in trimmed.chars() {
                    if ch == '{' && raw_block_depth > 0 {
                        raw_block_depth += 1;
                    } else if ch == '}' {
                        raw_block_depth -= 1;
                        if raw_block_depth == 0 {
                            in_raw_block = false;
                        }
                    }
                }
            }

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
                        // Try to find a block directive above that's missing '{'
                        let fix = find_missing_open_brace_fix(&lines, line_number, close_indent);

                        // Skip if we already reported this as a block directive missing '{'
                        let already_reported = fix.as_ref().map_or(false, |f| {
                            block_directive_error_lines.contains(&f.line)
                        });

                        if !already_reported {
                            let (message, fix) = if let Some(f) = fix {
                                (
                                    "Missing opening brace '{' for block directive",
                                    Some(f),
                                )
                            } else if line.trim() == "}" {
                                (
                                    "Unexpected closing brace '}' without matching opening brace",
                                    Some(Fix::delete(line_number)),
                                )
                            } else {
                                (
                                    "Unexpected closing brace '}' without matching opening brace",
                                    None,
                                )
                            };

                            let mut error = LintError::new(
                                self.name(),
                                self.category(),
                                message,
                                Severity::Error,
                            )
                            .with_location(line_number, column);
                            if let Some(f) = fix {
                                error = error.with_fix(f);
                            }
                            errors.push(error);
                        }
                    }
                }

                prev_char = ch;
            }

            // Reset comment flag at end of line
            in_comment = false;
            prev_char = ' ';

            // Check for block directives missing opening brace
            // This catches cases like "location /" without "{"
            // Skip this check inside raw blocks (like lua_block)
            if string_char.is_none() && !in_raw_block {
                let trimmed = line.trim();
                // Skip empty lines, comments, and lines ending with { ; or }
                if !trimmed.is_empty()
                    && !trimmed.starts_with('#')
                    && !trimmed.ends_with('{')
                    && !trimmed.ends_with(';')
                    && !trimmed.ends_with('}')
                {
                    // Get the first word (directive name)
                    if let Some(first_word) = trimmed.split_whitespace().next() {
                        if crate::parser::is_block_directive_with_extras(
                            first_word,
                            additional_block_directives,
                        ) {
                            // This is a block directive missing its opening brace
                            let new_line = format!("{} {{", line.trim_end());
                            let fix = Fix::replace_line(line_number, &new_line);
                            errors.push(
                                LintError::new(
                                    self.name(),
                                    self.category(),
                                    &format!(
                                        "Block directive '{}' is missing opening brace '{{'",
                                        first_word
                                    ),
                                    Severity::Error,
                                )
                                .with_location(line_number, trimmed.len())
                                .with_fix(fix),
                            );
                            block_directive_error_lines.push(line_number);
                        }
                    }
                }
            }
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

    fn name(&self) -> &'static str {
        "unmatched-braces"
    }

    fn category(&self) -> &'static str {
        "syntax"
    }
}

/// Find a block directive above that's missing an opening brace
/// Returns a Fix to add ' {' to that line if found
fn find_missing_open_brace_fix(lines: &[&str], close_line: usize, close_indent: usize) -> Option<Fix> {
    // Look backwards for a line at the same indent that looks like a block directive
    // Block directives typically: don't end with ';' or '{', and have content
    for i in (0..close_line - 1).rev() {
        let line = lines[i];
        let trimmed = line.trim();
        let line_indent = line.len() - line.trim_start().len();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // If we find a line with less indent, stop searching
        if line_indent < close_indent {
            // Check if this line looks like a block directive missing '{'
            if !trimmed.ends_with('{') && !trimmed.ends_with(';') && !trimmed.ends_with('}') {
                // This could be the block directive missing '{'
                let line_number = i + 1;
                let new_line = format!("{} {{", line.trim_end());
                return Some(Fix::replace_line(line_number, &new_line));
            }
            break;
        }

        // If same indent and doesn't end with '{', ';', or '}', it might be the missing block
        if line_indent == close_indent
            && !trimmed.ends_with('{')
            && !trimmed.ends_with(';')
            && !trimmed.ends_with('}')
        {
            let line_number = i + 1;
            let new_line = format!("{} {{", line.trim_end());
            return Some(Fix::replace_line(line_number, &new_line));
        }
    }

    None
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
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        self.check_content_with_extras(&content, &[])
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

    #[test]
    fn test_missing_opening_brace() {
        // Missing opening brace for 'server' block
        let content = r#"http {
    server
        listen 80;
    }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0].message.contains("missing opening brace"),
            "Expected missing opening brace error, got: {}",
            errors[0].message
        );
        assert!(errors[0].fix.is_some(), "Expected fix to be provided");
    }

    #[test]
    fn test_block_directive_missing_brace() {
        // Block directive 'location' missing opening brace
        let content = r#"http {
  server {
    listen 80;

    location /
      root /var/www/html;
    }
  }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0].message.contains("location"),
            "Expected location in error message, got: {}",
            errors[0].message
        );
        assert!(errors[0].fix.is_some(), "Expected fix to be provided");
    }

    // =========================================================================
    // Tests with custom block directives
    // =========================================================================

    fn check_braces_with_extras(content: &str, extras: &[String]) -> Vec<LintError> {
        let rule = UnmatchedBraces;
        rule.check_content_with_extras(content, extras)
    }

    #[test]
    fn test_custom_block_directive_missing_brace() {
        // Custom block directive from extension module
        let content = r#"http {
    my_custom_block
        some_directive value;
    }
}
"#;
        // Without custom directives, no block directive error
        let errors = check_braces_with_extras(content, &[]);
        assert!(
            !errors.iter().any(|e| e.message.contains("my_custom_block")),
            "Should not detect custom directive without config"
        );

        // With custom directives, should detect missing brace
        let extras = vec!["my_custom_block".to_string()];
        let errors = check_braces_with_extras(content, &extras);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0].message.contains("my_custom_block"),
            "Expected custom block directive in error, got: {}",
            errors[0].message
        );
    }

    #[test]
    fn test_multiple_custom_block_directives() {
        let content = r#"http {
    custom_auth
        auth_type basic;
    }

    custom_cache
        cache_size 100m;
    }
}
"#;
        let extras = vec!["custom_auth".to_string(), "custom_cache".to_string()];
        let errors = check_braces_with_extras(content, &extras);
        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
    }

    // =========================================================================
    // Tests for various built-in block directives
    // =========================================================================

    #[test]
    fn test_if_block_missing_brace() {
        let content = r#"server {
    if ($request_uri ~* "\.php$")
        return 403;
    }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("if"));
    }

    #[test]
    fn test_upstream_block_missing_brace() {
        let content = r#"upstream backend
    server 127.0.0.1:8080;
    server 127.0.0.1:8081;
}

http {
    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("upstream"));
    }

    #[test]
    fn test_map_block_missing_brace() {
        let content = r#"map $uri $new_uri
    /old /new;
    /legacy /current;
}

server {
    listen 80;
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("map"));
    }

    #[test]
    fn test_geo_block_missing_brace() {
        let content = r#"geo $country
    default unknown;
    127.0.0.1 local;
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("geo"));
    }

    #[test]
    fn test_limit_except_block_missing_brace() {
        let content = r#"server {
    location / {
        limit_except GET POST
            deny all;
        }
    }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("limit_except"));
    }

    #[test]
    fn test_events_block_missing_brace() {
        let content = r#"events
    worker_connections 1024;
}

http {
    server {
        listen 80;
    }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("events"));
    }

    // =========================================================================
    // Tests for raw blocks (lua_block, etc.)
    // =========================================================================

    #[test]
    fn test_lua_block_if_not_detected() {
        // Lua's 'if' should not be detected as nginx block directive
        let content = r#"http {
    server {
        content_by_lua_block {
            if ngx.var.arg_test then
                ngx.say("test")
            end
        }
    }
}
"#;
        let errors = check_braces(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_lua_block_nested_braces() {
        let content = r#"http {
    server {
        content_by_lua_block {
            local t = { a = 1, b = 2 }
            for k, v in pairs(t) do
                ngx.say(k .. "=" .. v)
            end
        }
    }
}
"#;
        let errors = check_braces(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_multiple_lua_blocks() {
        let content = r#"http {
    init_by_lua_block {
        local cjson = require "cjson"
    }

    server {
        access_by_lua_block {
            if ngx.var.remote_addr == "127.0.0.1" then
                return
            end
        }

        content_by_lua_block {
            ngx.say("hello")
        }
    }
}
"#;
        let errors = check_braces(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    // =========================================================================
    // Edge cases and complex scenarios
    // =========================================================================

    #[test]
    fn test_nested_blocks_one_missing_brace() {
        let content = r#"http {
    server {
        location /api {
            if ($request_method = POST)
                proxy_pass http://backend;
            }
        }
    }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("if"));
    }

    #[test]
    fn test_block_directive_with_complex_args() {
        // location with regex
        let content = r#"http {
    server {
        location ~ \.php$
            fastcgi_pass unix:/var/run/php-fpm.sock;
        }
    }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("location"));
    }

    #[test]
    fn test_block_directive_on_multiple_lines() {
        // This should not be detected as missing brace since the line ends correctly
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
    fn test_correctly_matched_all_block_types() {
        let content = r#"events {
    worker_connections 1024;
}

http {
    upstream backend {
        server 127.0.0.1:8080;
    }

    map $uri $new {
        default 0;
    }

    geo $country {
        default unknown;
    }

    server {
        listen 80;

        location / {
            if ($request_method = POST) {
                return 405;
            }
        }

        location /api {
            limit_except GET {
                deny all;
            }
        }
    }
}
"#;
        let errors = check_braces(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_fix_adds_opening_brace() {
        let content = r#"http {
    server
        listen 80;
    }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1);

        let fix = errors[0].fix.as_ref().expect("Expected fix");
        assert!(
            fix.new_text.contains("server {"),
            "Fix should add opening brace, got: {}",
            fix.new_text
        );
    }

    #[test]
    fn test_non_block_directive_not_detected() {
        // 'listen', 'root', etc. are not block directives
        let content = r#"http {
    server {
        listen
        root /var/www;
    }
}
"#;
        let errors = check_braces(content);
        // Should not detect listen or root as block directives
        assert!(
            !errors.iter().any(|e| e.message.contains("listen") || e.message.contains("root")),
            "Should not detect non-block directives: {:?}",
            errors
        );
    }

    #[test]
    fn test_stream_block_missing_brace() {
        let content = r#"stream
    server {
        listen 12345;
        proxy_pass backend;
    }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("stream"));
    }

    #[test]
    fn test_mail_block_missing_brace() {
        let content = r#"mail
    server {
        listen 25;
        protocol smtp;
    }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("mail"));
    }

    #[test]
    fn test_types_block_missing_brace() {
        let content = r#"http {
    types
        text/html html;
        text/css css;
    }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("types"));
    }
}
