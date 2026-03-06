use crate::config::IndentSize;
use crate::docs::RuleDoc;
use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::{Config, ConfigItem};
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
    /// Check indentation on content string directly (used by WASM and docs)
    pub fn check_content(&self, content: &str) -> Vec<LintError> {
        // Parse with error recovery so we get an AST even for broken configs
        let (config, _errors) = crate::parser::parse_string_with_errors(content);
        self.check_config(&config)
    }

    /// Check indentation using the parsed AST
    pub fn check_config(&self, config: &Config) -> Vec<LintError> {
        let indent_size = self.effective_indent_size(&config.items);
        let mut errors = Vec::new();
        self.check_items(&config.items, 0, indent_size, &mut errors);
        errors
    }

    /// Get the effective indent size (auto-detected or fixed)
    fn effective_indent_size(&self, items: &[ConfigItem]) -> usize {
        match self.indent_size {
            IndentSize::Fixed(size) => size,
            IndentSize::Auto => detect_indent_size_from_ast(items).unwrap_or(2),
        }
    }

    /// Recursively walk the AST and check indentation at each depth
    fn check_items(
        &self,
        items: &[ConfigItem],
        depth: usize,
        indent_size: usize,
        errors: &mut Vec<LintError>,
    ) {
        for item in items {
            match item {
                ConfigItem::Directive(d) => {
                    let ws_start = d.span.start.offset - d.leading_whitespace.len();
                    check_whitespace(
                        errors,
                        &d.leading_whitespace,
                        depth,
                        indent_size,
                        d.span.start.line,
                        ws_start,
                    );

                    if let Some(block) = &d.block {
                        // Recurse into non-raw blocks (raw block contents are not nginx config)
                        if !block.is_raw() {
                            self.check_items(&block.items, depth + 1, indent_size, errors);
                        }
                        // Check closing brace indentation for all blocks (including raw blocks)
                        let closing_ws = &block.closing_brace_leading_whitespace;
                        if !closing_ws.is_empty() || depth > 0 {
                            let closing_brace_offset = block.span.end.offset - 1;
                            let closing_ws_start = closing_brace_offset - closing_ws.len();
                            let closing_line = if block.span.end.column > 1 {
                                block.span.end.line
                            } else {
                                block.span.end.line.saturating_sub(1)
                            };
                            check_whitespace(
                                errors,
                                closing_ws,
                                depth,
                                indent_size,
                                closing_line,
                                closing_ws_start,
                            );
                        }
                    }
                }
                ConfigItem::Comment(c) => {
                    let ws_start = c.span.start.offset - c.leading_whitespace.len();
                    check_whitespace(
                        errors,
                        &c.leading_whitespace,
                        depth,
                        indent_size,
                        c.span.start.line,
                        ws_start,
                    );
                }
                ConfigItem::BlankLine(_) => {}
            }
        }
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

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        self.check_config(config)
    }
}

/// Detect indent size from AST by finding the first item inside a top-level block
fn detect_indent_size_from_ast(items: &[ConfigItem]) -> Option<usize> {
    for item in items {
        if let ConfigItem::Directive(d) = item
            && let Some(block) = &d.block
        {
            // Find the first non-blank item in this block
            for inner in &block.items {
                let ws = match inner {
                    ConfigItem::Directive(d) => &d.leading_whitespace,
                    ConfigItem::Comment(c) => &c.leading_whitespace,
                    ConfigItem::BlankLine(_) => continue,
                };
                if !ws.is_empty() && !ws.contains('\t') {
                    return Some(ws.len());
                }
            }
        }
    }
    None
}

/// Check a single leading_whitespace value against expected indentation
fn check_whitespace(
    errors: &mut Vec<LintError>,
    leading_ws: &str,
    expected_depth: usize,
    indent_size: usize,
    line: usize,
    ws_start_offset: usize,
) {
    let expected_spaces = expected_depth * indent_size;

    // Detect tabs
    if leading_ws.contains('\t') {
        let correct_indent = " ".repeat(expected_spaces);
        let fix = Fix::replace_range(
            ws_start_offset,
            ws_start_offset + leading_ws.len(),
            &correct_indent,
        );
        errors.push(
            LintError::new(
                "indent",
                "style",
                "Use spaces instead of tabs for indentation",
                Severity::Warning,
            )
            .with_location(line, 1)
            .with_fix(fix),
        );
        return;
    }

    // Check space count
    if leading_ws.len() != expected_spaces {
        let message = format!(
            "Expected {} spaces of indentation, found {}",
            expected_spaces,
            leading_ws.len()
        );
        let correct_indent = " ".repeat(expected_spaces);
        let fix = Fix::replace_range(
            ws_start_offset,
            ws_start_offset + leading_ws.len(),
            &correct_indent,
        );
        errors.push(
            LintError::new("indent", "style", &message, Severity::Warning)
                .with_location(line, 1)
                .with_fix(fix),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_content(content: &str) -> Vec<LintError> {
        let rule = Indent::default();
        rule.check_content(content)
    }

    /// Apply range-based fixes to content (sorted by offset descending)
    fn apply_range_fixes(content: &str, errors: &[LintError]) -> String {
        let mut fixes: Vec<&crate::linter::Fix> =
            errors.iter().flat_map(|e| e.fixes.iter()).collect();
        fixes.sort_by(|a, b| {
            b.start_offset
                .unwrap_or(0)
                .cmp(&a.start_offset.unwrap_or(0))
        });
        let mut result = content.to_string();
        for fix in &fixes {
            if let (Some(start), Some(end)) = (fix.start_offset, fix.end_offset) {
                if start <= result.len() && end <= result.len() {
                    result.replace_range(start..end, &fix.new_text);
                }
            }
        }
        result
    }

    fn check_content_with_config(content: &str) -> Vec<LintError> {
        let config = crate::parser::parse_string(content).unwrap();
        let rule = Indent::default();
        rule.check(&config, Path::new("test.conf"))
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
    fn test_lua_block_closing_brace_wrong_indentation() {
        // Closing brace of lua_block should be checked for indentation
        let content = r#"http {
  server {
    content_by_lua_block {
      local x = 1
}
  }
}
"#;
        let errors = check_content(content);
        assert!(
            !errors.is_empty(),
            "Expected indentation error for lua_block closing brace"
        );
        assert!(
            errors.iter().any(|e| e.line == Some(5)),
            "Expected error on line 5 (closing brace), got: {:?}",
            errors
        );
    }

    #[test]
    fn test_lua_block_closing_brace_autofix() {
        let content = r#"http {
  server {
    content_by_lua_block {
      local x = 1
}
  }
}
"#;
        let expected = r#"http {
  server {
    content_by_lua_block {
      local x = 1
    }
  }
}
"#;
        let errors = check_content(content);
        let result = apply_range_fixes(content, &errors);
        assert_eq!(result, expected, "Autofix should only change closing brace indentation\nGot:\n{}", result);
    }

    #[test]
    fn test_lua_block_with_multiple_indent_errors_autofix() {
        let content = r#"http {
server {
content_by_lua_block {
      local x = 1
}
}
}
"#;
        let expected = r#"http {
  server {
    content_by_lua_block {
      local x = 1
    }
  }
}
"#;
        let errors = check_content(content);
        let result = apply_range_fixes(content, &errors);
        assert_eq!(result, expected, "Autofix result mismatch\nGot:\n{}", result);
    }

    #[test]
    fn test_lua_block_nested_braces_autofix() {
        let content = r#"http {
  server {
    content_by_lua_block {
      local t = {1, 2, 3}
      if true then
        ngx.say(t)
      end
  }
  }
}
"#;
        let expected = r#"http {
  server {
    content_by_lua_block {
      local t = {1, 2, 3}
      if true then
        ngx.say(t)
      end
    }
  }
}
"#;
        let errors = check_content(content);
        let result = apply_range_fixes(content, &errors);
        assert_eq!(result, expected, "Autofix result mismatch\nGot:\n{}", result);
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
        let file_errors = check_content_with_config(content);
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
        assert!(
            errors.is_empty(),
            "Expected no errors with auto-detection, got: {:?}",
            errors
        );
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
        assert!(
            errors.is_empty(),
            "Expected no errors with auto-detection, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_detect_indent_size() {
        // Test the detection function directly
        let config_4 = crate::parser::parse_string("http {\n    server {\n    }\n}\n").unwrap();
        assert_eq!(detect_indent_size_from_ast(&config_4.items), Some(4));

        let config_2 = crate::parser::parse_string("http {\n  server {\n  }\n}\n").unwrap();
        assert_eq!(detect_indent_size_from_ast(&config_2.items), Some(2));

        let config_tab = crate::parser::parse_string("http {\n\tserver {\n\t}\n}\n").unwrap();
        // Tab indentation returns None (not space-based)
        assert_eq!(detect_indent_size_from_ast(&config_tab.items), None);
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
        assert!(
            !errors.is_empty(),
            "Expected errors with mismatched indent size"
        );
    }

    #[test]
    fn test_lua_block_autofix_preserves_content() {
        let content = concat!(
            "http {\n",
            "  server {\n",
            "        location /api {\n",
            "            content_by_lua_block {\n",
            "                local cjson = require \"cjson\"\n",
            "                ngx.say(cjson.encode({status = \"ok\"}))\n",
            "            }\n",
            "        }\n",
            "    location /static/ {\n",
            "      alias /var/www/static/;\n",
            "    }\n",
            "  }\n",
            "}\n",
        );
        let errors = check_content(content);
        let result = apply_range_fixes(content, &errors);

        assert!(
            result.contains("local cjson = require"),
            "Lua block content should be preserved after autofix"
        );
        assert!(
            result.contains("location /static/"),
            "Directives after lua block should be preserved"
        );
    }
}
