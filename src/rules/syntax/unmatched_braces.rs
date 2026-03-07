use crate::docs::RuleDoc;
use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use crate::parser::line_index::LineIndex;
use crate::parser::parse_string_rowan;
use crate::parser::syntax_kind::{SyntaxElement, SyntaxKind, SyntaxNode};
use std::fs;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "unmatched-braces",
    category: "syntax",
    description: "Detects unmatched opening or closing braces",
    severity: "error",
    why: r#"When braces are unmatched, nginx cannot parse the configuration
file correctly and will fail to start.

This rule checks that opening braces '{' and closing braces '}'
are balanced, and that block directives have their opening brace."#,
    bad_example: include_str!("unmatched_braces/bad.conf"),
    good_example: include_str!("unmatched_braces/good.conf"),
    references: &["https://nginx.org/en/docs/beginners_guide.html"],
};

/// Check for unmatched braces
pub struct UnmatchedBraces;

/// Information about an opening brace on the stack.
#[derive(Debug, Clone)]
struct BraceInfo {
    /// Byte offset of the `{` token.
    offset: usize,
    /// Indentation (in bytes) of the line containing the `{`.
    indent: usize,
}

/// A flattened CST token with its byte offset and context.
#[derive(Debug, Clone)]
struct FlatToken {
    kind: SyntaxKind,
    offset: usize,
    len: usize,
}

impl UnmatchedBraces {
    /// Check content directly (used by WASM)
    pub fn check_content(&self, content: &str) -> Vec<LintError> {
        self.check_cst(content, &[])
    }

    /// Check content with additional block directives
    pub fn check_content_with_extras(
        &self,
        content: &str,
        additional_block_directives: &[String],
    ) -> Vec<LintError> {
        self.check_cst(content, additional_block_directives)
    }

    /// CST-based unmatched brace detection.
    ///
    /// Flattens the CST into a token stream (skipping raw block contents),
    /// then performs line-based analysis and brace-stack matching — similar
    /// to the original text-based approach but benefiting from proper
    /// tokenization of strings, comments, and raw blocks.
    fn check_cst(&self, source: &str, additional_block_directives: &[String]) -> Vec<LintError> {
        let (root, _) = parse_string_rowan(source);
        let line_index = LineIndex::new(source);

        // Flatten CST tokens, skipping raw block internals.
        let tokens = Self::flatten_tokens(&root);

        let mut errors = Vec::new();
        let mut brace_stack: Vec<BraceInfo> = Vec::new();

        // Group tokens by line for line-based analysis.
        let mut line_tokens: Vec<Vec<&FlatToken>> = vec![Vec::new()];
        for token in &tokens {
            if token.kind == SyntaxKind::NEWLINE {
                line_tokens.push(Vec::new());
            } else {
                line_tokens.last_mut().unwrap().push(token);
            }
        }

        for (line_idx, line_toks) in line_tokens.iter().enumerate() {
            // Process braces on this line
            for tok in line_toks {
                match tok.kind {
                    SyntaxKind::L_BRACE => {
                        let indent = Self::line_indent(source, tok.offset);
                        brace_stack.push(BraceInfo {
                            offset: tok.offset,
                            indent,
                        });
                    }
                    SyntaxKind::R_BRACE => {
                        if brace_stack.pop().is_none() {
                            // Extra closing brace
                            let pos = line_index.position(tok.offset);
                            let fix = Self::build_remove_brace_fix(source, tok.offset);
                            let mut error = LintError::new(
                                "unmatched-braces",
                                "syntax",
                                "Unexpected closing brace '}' without matching opening brace",
                                Severity::Error,
                            )
                            .with_location(pos.line, pos.column);
                            if let Some(f) = fix {
                                error = error.with_fix(f);
                            }
                            errors.push(error);
                        }
                    }
                    _ => {}
                }
            }

            // Check for block directive missing opening brace.
            // Skip lines that are comment-only or empty.
            let meaningful: Vec<&&FlatToken> =
                line_toks.iter().filter(|t| !t.kind.is_trivia()).collect();

            if meaningful.is_empty() {
                continue;
            }

            // First meaningful token must be IDENT (directive name)
            let first = meaningful[0];
            if first.kind != SyntaxKind::IDENT {
                continue;
            }

            let name = &source[first.offset..first.offset + first.len];
            if !crate::parser::is_block_directive_with_extras(name, additional_block_directives) {
                continue;
            }

            // Check if line ends with `{`, `;`, or `}` (last meaningful token)
            let last = meaningful.last().unwrap();
            if matches!(
                last.kind,
                SyntaxKind::L_BRACE | SyntaxKind::SEMICOLON | SyntaxKind::R_BRACE
            ) {
                continue;
            }

            // Check if the first non-empty/non-comment line after this one
            // starts with `{` (brace-on-next-line style).
            let next_starts_with_brace = line_tokens[line_idx + 1..]
                .iter()
                .find_map(|next_line| {
                    if next_line.is_empty() {
                        return None; // skip blank lines
                    }
                    // Skip comment-only lines (check before filtering trivia)
                    if next_line
                        .iter()
                        .all(|t| t.kind.is_trivia() || t.kind == SyntaxKind::COMMENT)
                    {
                        return None;
                    }
                    let next_meaningful: Vec<&&FlatToken> =
                        next_line.iter().filter(|t| !t.kind.is_trivia()).collect();
                    if next_meaningful.is_empty() {
                        return None;
                    }
                    Some(next_meaningful[0].kind == SyntaxKind::L_BRACE)
                })
                .unwrap_or(false);

            if next_starts_with_brace {
                continue;
            }

            // This is a block directive missing its opening brace.
            let pos = line_index.position(last.offset + last.len - 1);

            // Fix: insert " {" after the last meaningful content on this line
            let fix_offset = last.offset + last.len;
            let fix = Fix::replace_range(fix_offset, fix_offset, " {");

            errors.push(
                LintError::new(
                    "unmatched-braces",
                    "syntax",
                    &format!("Block directive '{}' is missing opening brace '{{'", name),
                    Severity::Error,
                )
                .with_location(pos.line, pos.column)
                .with_fix(fix),
            );

            // Push a virtual brace so the matching `}` doesn't also get flagged
            let indent = Self::line_indent(source, first.offset);
            brace_stack.push(BraceInfo {
                offset: fix_offset,
                indent,
            });
        }

        // Remaining unclosed braces
        while let Some(unclosed) = brace_stack.pop() {
            let pos = line_index.position(unclosed.offset);

            let closing_brace = format!("{}}}", " ".repeat(unclosed.indent));
            let insert_offset = source.len();
            let new_text = if !source.ends_with('\n') {
                format!("\n{}", closing_brace)
            } else {
                format!("{}\n", closing_brace)
            };

            errors.push(
                LintError::new(
                    "unmatched-braces",
                    "syntax",
                    "Unclosed brace '{' - missing closing brace '}'",
                    Severity::Error,
                )
                .with_location(pos.line, pos.column)
                .with_fix(Fix::replace_range(
                    insert_offset,
                    insert_offset,
                    &new_text,
                )),
            );
        }

        errors
    }

    /// Flatten the CST into a linear token stream, skipping interior tokens
    /// of raw blocks (e.g. `content_by_lua_block { ... }`).
    ///
    /// The L_BRACE and R_BRACE of raw blocks are preserved so that brace
    /// counting and line analysis work correctly.
    fn flatten_tokens(root: &SyntaxNode) -> Vec<FlatToken> {
        let mut tokens = Vec::new();
        Self::collect_tokens(root, &mut tokens);
        tokens
    }

    fn collect_tokens(node: &SyntaxNode, tokens: &mut Vec<FlatToken>) {
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(token) => {
                    let offset: usize = token.text_range().start().into();
                    let len = token.text_range().len().into();
                    tokens.push(FlatToken {
                        kind: token.kind(),
                        offset,
                        len,
                    });
                }
                SyntaxElement::Node(child_node) => {
                    if child_node.kind() == SyntaxKind::BLOCK
                        && crate::parser::is_raw_block_cst_node(&child_node)
                    {
                        // For raw blocks, only emit L_BRACE and R_BRACE so
                        // brace counting and line analysis work correctly.
                        // Skip all interior tokens.
                        for raw_child in child_node.children_with_tokens() {
                            if let SyntaxElement::Token(t) = raw_child
                                && (t.kind() == SyntaxKind::L_BRACE
                                    || t.kind() == SyntaxKind::R_BRACE)
                            {
                                let offset: usize = t.text_range().start().into();
                                let len = t.text_range().len().into();
                                tokens.push(FlatToken {
                                    kind: t.kind(),
                                    offset,
                                    len,
                                });
                            }
                        }
                        continue;
                    }
                    Self::collect_tokens(&child_node, tokens);
                }
            }
        }
    }

    /// Get the indentation (in bytes) of the line containing the given offset.
    fn line_indent(source: &str, offset: usize) -> usize {
        let line_start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
        source[line_start..]
            .bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count()
    }

    /// Build a fix to remove a standalone `}` line, or `None` if `}` shares
    /// the line with other content.
    fn build_remove_brace_fix(source: &str, brace_offset: usize) -> Option<Fix> {
        let line_start = source[..brace_offset].rfind('\n').map_or(0, |i| i + 1);
        let line_end = source[brace_offset..]
            .find('\n')
            .map_or(source.len(), |i| brace_offset + i);

        let line = &source[line_start..line_end];
        if line.trim() == "}" {
            if line_start > 0 {
                Some(Fix::replace_range(line_start - 1, line_end, ""))
            } else if line_end < source.len() {
                Some(Fix::replace_range(line_start, line_end + 1, ""))
            } else {
                Some(Fix::replace_range(line_start, line_end, ""))
            }
        } else {
            None
        }
    }
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

        self.check_cst(&content, &[])
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
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
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
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
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
        assert!(!errors[0].fixes.is_empty(), "Expected fix to be provided");
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
        assert!(!errors[0].fixes.is_empty(), "Expected fix to be provided");
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

        let fix = errors[0].fixes.first().expect("Expected fix");
        // Range-based fix appends " {" to the line
        assert!(
            fix.new_text.contains("{"),
            "Fix should add opening brace, got: {}",
            fix.new_text
        );
        // Verify it's a range-based fix
        assert!(
            fix.start_offset.is_some() && fix.end_offset.is_some(),
            "Expected range-based fix"
        );
    }

    #[test]
    fn test_non_block_directive_not_detected() {
        // 'listen', 'root', etc. are not block directives.
        // 'listen' on its own line without ';' should not be flagged as
        // a block directive missing braces.
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
            !errors
                .iter()
                .any(|e| e.message.contains("listen") || e.message.contains("root")),
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
    // Edge cases
    // =========================================================================

    #[test]
    fn test_empty_input() {
        let errors = UnmatchedBraces.check_content("");
        assert!(errors.is_empty(), "Expected no errors for empty input");
    }

    #[test]
    fn test_only_braces() {
        let errors = UnmatchedBraces.check_content("{}");
        assert!(errors.is_empty(), "Expected no errors for matched braces");

        let errors = UnmatchedBraces.check_content("{");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Unclosed brace"));

        let errors = UnmatchedBraces.check_content("}");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Unexpected closing brace"));
    }

    #[test]
    fn test_server_in_upstream_with_trailing_comment() {
        // server directive in upstream with trailing comment should not be
        // detected as a block directive missing braces
        let content = r#"http {
    upstream backend {
        server 10.0.0.1:8080; # backend-a
        server 10.0.0.2:8080; # backend-b
    }

    server {
        listen 80;
        location / {
            proxy_pass http://backend;
        }
    }
}
"#;
        let errors = check_braces(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }
}
