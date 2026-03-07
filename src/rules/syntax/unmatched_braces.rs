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
                        if brace_stack.is_empty() {
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
                        } else {
                            let rbrace_indent = Self::line_indent(source, tok.offset);
                            // Find the best matching opening brace by indent.
                            // Search from top of stack downward for a brace
                            // whose indent matches the closing brace's indent.
                            let match_idx =
                                brace_stack.iter().rposition(|b| b.indent == rbrace_indent);

                            let pop_from = match match_idx {
                                Some(idx) => idx,
                                // No indent match; fall back to popping the top
                                None => brace_stack.len() - 1,
                            };

                            // Everything above pop_from is unclosed
                            while brace_stack.len() > pop_from + 1 {
                                let unclosed = brace_stack.pop().unwrap();
                                errors.push(Self::build_unclosed_brace_error(
                                    source,
                                    &line_index,
                                    &line_tokens,
                                    &unclosed,
                                ));
                            }
                            // Pop the matched brace
                            brace_stack.pop();
                        }
                    }
                    _ => {}
                }
            }

            // Check for block directive missing opening brace.
            // Skip lines that are comment-only or empty.
            let first = match line_toks.iter().find(|t| !t.kind.is_trivia()) {
                Some(tok) => tok,
                None => continue,
            };

            // First meaningful token must be IDENT (directive name)
            if first.kind != SyntaxKind::IDENT {
                continue;
            }

            let name = &source[first.offset..first.offset + first.len];
            if !crate::parser::is_block_directive_with_extras(name, additional_block_directives) {
                continue;
            }

            // Check if line ends with `{`, `;`, or `}` (last meaningful token)
            let last = line_toks
                .iter()
                .rev()
                .find(|t| !t.kind.is_trivia())
                .unwrap();
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
                    // Skip blank lines and comment-only lines
                    if next_line.iter().all(|t| t.kind.is_trivia()) {
                        return None;
                    }
                    let first_meaningful = next_line.iter().find(|t| !t.kind.is_trivia()).unwrap();
                    Some(first_meaningful.kind == SyntaxKind::L_BRACE)
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

        // Remaining unclosed braces — find the best insertion point using
        // indentation analysis rather than always appending at EOF.
        while let Some(unclosed) = brace_stack.pop() {
            errors.push(Self::build_unclosed_brace_error(
                source,
                &line_index,
                &line_tokens,
                &unclosed,
            ));
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

    /// Build a `LintError` for an unclosed brace, including a fix that
    /// inserts `}` at the best position determined by indentation analysis.
    fn build_unclosed_brace_error(
        source: &str,
        line_index: &LineIndex,
        line_tokens: &[Vec<&FlatToken>],
        unclosed: &BraceInfo,
    ) -> LintError {
        let pos = line_index.position(unclosed.offset);
        let brace_line = pos.line;
        let closing_brace = format!("{}}}", " ".repeat(unclosed.indent));
        let insert_offset =
            Self::find_close_brace_offset(source, line_tokens, brace_line, unclosed.indent);

        let new_text = if insert_offset == source.len() {
            if !source.ends_with('\n') {
                format!("\n{}", closing_brace)
            } else {
                format!("{}\n", closing_brace)
            }
        } else {
            format!("{}\n", closing_brace)
        };

        LintError::new(
            "unmatched-braces",
            "syntax",
            "Unclosed brace '{' - missing closing brace '}'",
            Severity::Error,
        )
        .with_location(pos.line, pos.column)
        .with_fix(Fix::replace_range(insert_offset, insert_offset, &new_text))
    }

    /// Find the byte offset where a closing `}` should be inserted for an
    /// unclosed brace.
    ///
    /// Scans lines after `brace_line` (1-based) looking for the first line
    /// whose first non-trivia token starts at indentation ≤ `brace_indent`.
    /// Returns the byte offset of that line's start (so `}` is inserted
    /// before it). Falls back to `source.len()` (EOF) if no such line exists.
    fn find_close_brace_offset(
        source: &str,
        line_tokens: &[Vec<&FlatToken>],
        brace_line: usize,
        brace_indent: usize,
    ) -> usize {
        // line_tokens is 0-indexed; brace_line is 1-based.
        // Start scanning from the line after the brace.
        for line_toks in line_tokens.iter().skip(brace_line) {
            let mut meaningful = line_toks.iter().filter(|t| !t.kind.is_trivia());
            let first_meaningful = match meaningful.next() {
                Some(tok) => tok,
                None => continue, // blank or comment-only line
            };

            // Skip lines that only contain `}` — those closing braces were
            // already matched by the brace stack during the main loop.
            if first_meaningful.kind == SyntaxKind::R_BRACE && meaningful.next().is_none() {
                continue;
            }
            let indent = Self::line_indent(source, first_meaningful.offset);
            if indent <= brace_indent {
                // At same or lower indentation — treat as block boundary
                // only if strictly lower, or if the line starts a new block
                // (contains `{`). Simple directives at the same indent may
                // just have broken indentation.
                let is_block_boundary = indent < brace_indent
                    || line_toks.iter().any(|t| t.kind == SyntaxKind::L_BRACE);
                if is_block_boundary {
                    let line_start = source[..first_meaningful.offset]
                        .rfind('\n')
                        .map_or(0, |i| i + 1);
                    return Self::skip_blank_lines_backward(source, line_start);
                }
            }
        }

        // No line with lower indentation found — insert at EOF
        source.len()
    }

    /// Given a byte offset at a line start, skip backward past any preceding
    /// blank lines so that `}` is inserted before the blank lines rather than
    /// after them.
    fn skip_blank_lines_backward(source: &str, offset: usize) -> usize {
        let mut pos = offset;
        while pos > 0 {
            // pos points to start of a line. The previous line ends at pos-1 ('\n').
            // Find the start of the previous line.
            let prev_line_start = source[..pos - 1].rfind('\n').map_or(0, |i| i + 1);
            let prev_line = &source[prev_line_start..pos - 1];
            if prev_line.trim().is_empty() {
                pos = prev_line_start;
            } else {
                break;
            }
        }
        // Don't skip past the beginning of the file
        if pos == 0 && offset > 0 { offset } else { pos }
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

    // =========================================================================
    // Tests for smart fix positioning (indentation-based)
    // =========================================================================

    fn apply_fix(content: &str, fix: &Fix) -> String {
        let mut result = content.to_string();
        if let (Some(start), Some(end)) = (fix.start_offset, fix.end_offset) {
            result.replace_range(start..end, &fix.new_text);
        }
        result
    }

    #[test]
    fn test_fix_unclosed_http_before_events() {
        // `http {` is unclosed; `events {` at the same indent level indicates
        // where `}` should be inserted (before `events`).
        let content = r#"http {
    server_tokens on;
    autoindex on;
    upstream backend {
        server api.example.com:8080;
    }

events {
    worker_connections 1024;
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("Unclosed brace"));

        let fix = errors[0].fixes.first().expect("Expected fix");
        let result = apply_fix(content, fix);
        // The closing `}` should be inserted before the blank line and `events {`
        assert!(
            result.contains("}\n}\n\nevents {"),
            "Fix should insert }} before blank line, got:\n{}",
            result
        );
    }

    #[test]
    fn test_fix_unclosed_nested_block_uses_indent() {
        // `http {` is unclosed (only `server` has matching braces).
        // The fix should insert `}` before EOF, matching `http`'s indent (0).
        let content = "http {\n    server {\n        listen 80;\n    }\n";
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("Unclosed brace"));

        let fix = errors[0].fixes.first().expect("Expected fix");
        let result = apply_fix(content, fix);
        // `http` is at indent 0, no line at indent ≤ 0 after it,
        // so fix appends at EOF.
        assert!(
            result.ends_with("}\n"),
            "Fix should append at EOF, got:\n{}",
            result
        );
    }

    #[test]
    fn test_fix_unclosed_at_eof_fallback() {
        // When there's no subsequent line at lower indent, fix goes at EOF
        let content = "http {\n    server_tokens on;\n";
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);

        let fix = errors[0].fixes.first().expect("Expected fix");
        let result = apply_fix(content, fix);
        assert!(
            result.ends_with("}\n"),
            "Fix should append at EOF, got:\n{}",
            result
        );
    }

    #[test]
    fn test_skip_blank_lines_backward_stops_at_content() {
        let source = "http {\n    server_tokens on;\n\n\nserver {\n";
        let offset = source.find("server {").unwrap();
        let result = UnmatchedBraces::skip_blank_lines_backward(source, offset);
        // Should skip back past blank lines to right after "server_tokens on;\n"
        assert!(
            result < offset,
            "Should skip back past blank lines, got {}",
            result
        );
        // Inserting at result should place `}` right after the content line
        assert_eq!(
            &source[result..result + 1],
            "\n",
            "Result should point to start of first blank line"
        );
    }

    #[test]
    fn test_skip_blank_lines_backward_does_not_go_past_file_start() {
        let source = "\n\nserver {\n";
        let offset = source.find("server {").unwrap();
        let result = UnmatchedBraces::skip_blank_lines_backward(source, offset);
        assert_eq!(
            result, offset,
            "Should not skip past file start, got offset {}",
            result
        );
    }

    #[test]
    fn test_fix_unclosed_upstream_skips_rbrace_line() {
        // upstream backend { is unclosed, but the stack matches it with http's `}`.
        // So `http {` (indent 0) is reported as unclosed.
        // R_BRACE-only lines (`}`) are skipped, so the fix falls back to EOF
        // rather than inserting before the existing `}`.
        let content = "http {\n  server_tokens on;\n  autoindex on;\n\n  upstream backend {\n    server api.example.com:8080;\n  \n\n  server {\n    listen 80;\n    server_name example.com;\n  }\n}\n";
        let rule = UnmatchedBraces;
        let errors = rule.check_content(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("Unclosed brace"));

        let fix = errors[0].fixes.first().expect("Expected fix");
        let result = apply_fix(content, fix);
        // With R_BRACE-only line skipping, the fix appends at EOF
        // (no content line at indent ≤ 0 exists after the brace)
        assert!(
            result.ends_with("}\n"),
            "Fix should append at EOF, got:\n{}",
            result
        );
    }

    #[test]
    fn test_fix_unclosed_upstream_inserts_before_server() {
        // upstream backend { is missing its closing brace.
        // The fix should insert `}` before `server {` (same indent level),
        // not at EOF.
        let content = r#"http {
  server_tokens on;
  autoindex on;

  upstream backend {
    server api.example.com:8080;


  server {
    listen 80 ;
    server_name example.com;

    add_header X-Frame-Options "SAMEORIGIN";

    location / {
      root /var/www/html;
    }

    location /api {
      proxy_pass http://api.example.com:8080;

      add_header X-Request-ID $request_id;
    }

    location /static/ {
      alias /var/www/static;
    }
  }
}
"#;
        let errors = check_braces(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("Unclosed brace"));

        let fix = errors[0].fixes.first().expect("Expected fix");
        let result = apply_fix(content, fix);
        // The closing `}` should be inserted before the blank lines,
        // not right before `server {`
        assert!(
            result.contains("    server api.example.com:8080;\n  }\n\n\n  server {"),
            "Fix should insert }} before blank lines, got:\n{}",
            result
        );
    }

    #[test]
    fn test_fix_unclosed_upstream_with_broken_indent_inside() {
        // upstream backend { is missing its closing brace, AND one of its
        // child directives has broken indentation (same level as upstream).
        // The fix should still insert `}` before `server {`, not before the
        // broken-indent line.
        let content = r#"http {
  server_tokens on;

  upstream backend {
    server api1.example.com:8080;
  server api2.example.com:8080;

  server {
    listen 80;
  }
}
"#;
        let errors = check_braces(content);
        let unclosed_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("Unclosed brace"))
            .collect();
        assert_eq!(
            unclosed_errors.len(),
            1,
            "Expected 1 unclosed brace error, got: {:?}",
            errors
        );

        let fix = unclosed_errors[0].fixes.first().expect("Expected fix");
        let result = apply_fix(content, fix);
        // The `}` should be inserted before the blank line and `server {`,
        // not before the broken-indent `server api2` line
        assert!(
            result.contains("  }\n\n  server {"),
            "Fix should insert }} before blank line, got:\n{}",
            result
        );
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
