use crate::docs::RuleDoc;
use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use crate::parser::line_index::LineIndex;
use crate::parser::parse_string_rowan;
use crate::parser::syntax_kind::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
use std::fs;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "unclosed-quote",
    category: "syntax",
    description: "Detects unclosed quotes in directive values",
    severity: "error",
    why: r#"When quotes are not closed, nginx cannot parse the configuration
correctly and may fail to start or behave unexpectedly.

Strings enclosed in quotes must be closed with the same quote type."#,
    bad_example: include_str!("unclosed_quote/bad.conf"),
    good_example: include_str!("unclosed_quote/good.conf"),
    references: &[],
    ..RuleDoc::DEFAULTS
};

/// Check for unclosed string quotes
pub struct UnclosedQuote;

impl UnclosedQuote {
    /// nginx directive keywords that should not be inside quoted strings
    const NGINX_KEYWORDS: &'static [&'static str] = &[
        "permanent",
        "redirect",
        "last",
        "break", // rewrite flags
        "default",
        "backup",
        "down",
        "weight",
        "max_fails", // upstream
        "always",    // add_header flag
    ];

    /// Check content directly (used by WASM)
    pub fn check_content(&self, content: &str) -> Vec<LintError> {
        self.check_cst(content)
    }

    /// CST-based unclosed quote detection.
    ///
    /// Recursively walks CST nodes, skipping raw block contents (e.g.
    /// `content_by_lua_block`), and checks whether quoted-string tokens
    /// are properly terminated.
    fn check_cst(&self, source: &str) -> Vec<LintError> {
        let (root, _syntax_errors) = parse_string_rowan(source);
        let line_index = LineIndex::new(source);
        let mut errors = Vec::new();

        Self::walk_node(&root, &line_index, &mut errors);

        errors
    }

    /// Recursively walk a CST node, collecting unclosed quote errors.
    ///
    /// BLOCK nodes belonging to raw block directives (lua etc.) are skipped
    /// entirely, avoiding per-token ancestor checks.
    fn walk_node(node: &SyntaxNode, line_index: &LineIndex, errors: &mut Vec<LintError>) {
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Node(child_node) => {
                    // Skip raw block contents
                    if child_node.kind() == SyntaxKind::BLOCK
                        && crate::parser::is_raw_block_cst_node(&child_node)
                    {
                        continue;
                    }
                    Self::walk_node(&child_node, line_index, errors);
                }
                SyntaxElement::Token(token) => {
                    let (quote_char, quote_name) = match token.kind() {
                        SyntaxKind::DOUBLE_QUOTED_STRING => ('"', "double quote"),
                        SyntaxKind::SINGLE_QUOTED_STRING => ('\'', "single quote"),
                        _ => continue,
                    };

                    let text = token.text();

                    // A properly closed string starts and ends with the same quote
                    if text.len() >= 2 && text.ends_with(quote_char) {
                        continue;
                    }

                    let offset: usize = token.text_range().start().into();

                    // The lexer pairs quotes greedily, so a missing quote
                    // earlier in the statement can shift every following
                    // pairing and leave a *later* token flagged instead. If we
                    // can pin the real culprit within the same directive,
                    // report and fix there; otherwise fix this token directly.
                    let (report_offset, fix) = match Self::locate_real_culprit(&token, quote_char) {
                        Some((culprit_offset, fix)) => (culprit_offset, Some(fix)),
                        None => (offset, Self::create_fix(quote_char, text, offset)),
                    };

                    let pos = line_index.position(report_offset);
                    let message =
                        format!("Unclosed {} - missing closing {}", quote_name, quote_char);

                    let mut error =
                        LintError::new("unclosed-quote", "syntax", &message, Severity::Error)
                            .with_location(pos.line, pos.column);

                    if let Some(f) = fix {
                        error = error.with_fix(f);
                    }

                    errors.push(error);
                }
            }
        }
    }

    /// Try to create a fix for an unclosed quote.
    ///
    /// Looks at the first line of the token text to find a semicolon and
    /// inserts the closing quote before it (or before an nginx keyword).
    /// `token_offset` is the byte offset of the token start in the source.
    fn create_fix(quote: char, token_text: &str, token_offset: usize) -> Option<Fix> {
        // Only consider the first line of the token (the line where the quote opened)
        let first_line = token_text.lines().next().unwrap_or(token_text);

        // If the first line doesn't contain a semicolon, we can't auto-fix
        let semicolon_pos = first_line.rfind(';')?;
        let before_semicolon = &first_line[..semicolon_pos];

        // The token always starts with a quote character, so before_semicolon
        // is at least 1 byte. Guard defensively anyway.
        if before_semicolon.is_empty() {
            return None;
        }
        let after_quote = &before_semicolon[1..];

        // Check if there's an nginx keyword at the end that should be outside the string
        for keyword in Self::NGINX_KEYWORDS {
            let pattern = format!(" {}", keyword);
            if after_quote.ends_with(&pattern) {
                let keyword_start = before_semicolon.len() - keyword.len() - 1;
                let insert_offset = token_offset + keyword_start;
                return Some(Fix::replace_range(
                    insert_offset,
                    insert_offset,
                    &quote.to_string(),
                ));
            }
        }

        // Default: insert quote before semicolon
        let insert_offset = token_offset + semicolon_pos;
        Some(Fix::replace_range(
            insert_offset,
            insert_offset,
            &quote.to_string(),
        ))
    }

    /// Find the token the user actually failed to close when the flagged
    /// `unclosed` token is really a downstream artifact of the lexer's greedy
    /// quote pairing, returning a `(offset, fix)` pointing at the real culprit.
    ///
    /// The lexer pairs quote characters greedily left-to-right and knows
    /// nothing about `#` comments or line boundaries inside a string, so a
    /// single missing quote earlier in a statement shifts every subsequent
    /// pairing and can leave a later token flagged instead. All the mispaired
    /// tokens share one `DIRECTIVE` CST node, so we search that node (bounding
    /// the search — a correctly-closed string elsewhere in the file is never a
    /// target) for the earliest same-kind quoted-string token that spuriously
    /// crosses a boundary it shouldn't.
    ///
    /// The directive should have ended at the first `;` on such a token's line;
    /// anything the string swallowed *past* that `;` is the tell. If what
    /// follows runs onto the next line (`"value;` … `return 200 "`, issue #295)
    /// or contains a `#` comment (`"value; # note "`, issue #299), the closing
    /// quote paired across a boundary and this token is the real culprit —
    /// [`create_fix`] then places the missing quote before its `;`. Keying on
    /// content *after* the first `;` keeps legitimately-closed values whose own
    /// data contains `#` before the `;` (e.g. a `"#aabbcc"` colour) from being
    /// misread as culprits.
    ///
    /// Known limitation: a closed value that legitimately swallows a `#` after
    /// a `;` (e.g. `"a;#b"`) before an unclosed quote in the same directive
    /// could still be misread; such values are rare in nginx configs.
    fn locate_real_culprit(unclosed: &SyntaxToken, quote: char) -> Option<(usize, Fix)> {
        let kind = unclosed.kind();
        let unclosed_start: usize = unclosed.text_range().start().into();
        let directive = unclosed.parent()?;

        for child in directive.children_with_tokens() {
            let SyntaxElement::Token(token) = child else {
                continue;
            };
            if token.kind() != kind {
                continue;
            }
            let start: usize = token.text_range().start().into();
            if start >= unclosed_start {
                break; // reached the flagged token; nothing earlier remains
            }

            let text = token.text();
            let first_line = text.lines().next().unwrap_or(text);
            // The directive should have ended at the first `;` on this line;
            // content the string swallowed *past* it is the tell of a mispairing.
            let Some(semi) = first_line.find(';') else {
                continue; // no terminator to close before
            };
            let spans_newline = text.contains('\n');
            let swallowed_comment = first_line[semi + 1..].contains('#');
            if spans_newline || swallowed_comment {
                let fix = Self::create_fix(quote, text, start)?;
                return Some((start, fix));
            }
        }

        None
    }
}

impl LintRule for UnclosedQuote {
    fn name(&self) -> &'static str {
        "unclosed-quote"
    }

    fn category(&self) -> &'static str {
        "syntax"
    }

    fn description(&self) -> &'static str {
        "Detects unclosed string quotes"
    }

    fn check(&self, _config: &Config, path: &Path) -> Vec<LintError> {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        self.check_cst(&content)
    }

    fn wants_content(&self) -> bool {
        true
    }

    fn check_with_content(&self, _config: &Config, _path: &Path, content: &str) -> Vec<LintError> {
        self.check_cst(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_quotes(content: &str) -> Vec<LintError> {
        let rule = UnclosedQuote;
        rule.check_content(content)
    }

    fn apply_fix(content: &str, fix: &Fix) -> String {
        let mut result = content.to_string();
        if let (Some(start), Some(end)) = (fix.start_offset, fix.end_offset) {
            result.replace_range(start..end, &fix.new_text);
        }
        result
    }

    #[test]
    fn test_closed_double_quote() {
        let content = r#"server {
    return 200 "hello world";
}
"#;
        let errors = check_quotes(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_closed_single_quote() {
        let content = r#"server {
    return 200 'hello world';
}
"#;
        let errors = check_quotes(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_unclosed_double_quote_at_eof() {
        // Unclosed quote at end of file should be detected
        let content = r#"server {
    return 200 "hello world;
}"#;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("double quote"));
        assert_eq!(errors[0].line, Some(2));
    }

    #[test]
    fn test_unclosed_single_quote_at_eof() {
        // Unclosed quote at end of file should be detected
        let content = r#"server {
    return 200 'hello world;
}"#;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("single quote"));
        assert_eq!(errors[0].line, Some(2));
    }

    #[test]
    fn test_quote_in_comment() {
        let content = r#"server {
    # This is a comment with "quote
    return 200 "ok";
}
"#;
        let errors = check_quotes(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_escaped_quote() {
        let content = r#"server {
    return 200 "hello \"world\"";
}
"#;
        let errors = check_quotes(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_mixed_quotes() {
        let content = r#"server {
    return 200 "it's ok";
}
"#;
        let errors = check_quotes(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_multiline_string() {
        // Multi-line strings are allowed in nginx (e.g., for Lua/mruby code)
        let content = r#"server {
    content_by_lua_block '
        ngx.say("hello")
    ';
}
"#;
        let errors = check_quotes(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_multiline_unclosed_at_eof() {
        // Unclosed multi-line string at end of file
        let content = r#"server {
    content_by_lua_block '
        ngx.say("hello")
}"#;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("single quote"));
    }

    #[test]
    fn test_fix_inserts_closing_quote_before_semicolon() {
        let content = r#"location / {
    add_header X-Custom "value;
}
"#;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1);
        let fix = errors[0].fixes.first().expect("Expected a fix");
        assert_eq!(fix.new_text, "\"");
        // The fix should insert `"` right before the `;`
        let result = apply_fix(content, fix);
        assert!(
            result.contains(r#"add_header X-Custom "value";"#),
            "Fix should close the quote before semicolon, got: {}",
            result
        );
    }

    #[test]
    fn test_fix_inserts_closing_quote_before_keyword() {
        let content = "rewrite ^/old \"http://example.com/new permanent;\n";
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1);
        let fix = errors[0].fixes.first().expect("Expected a fix");
        let result = apply_fix(content, fix);
        assert!(
            result.contains("\"http://example.com/new\" permanent;"),
            "Fix should close the quote before keyword, got: {}",
            result
        );
    }

    #[test]
    fn test_fix_keyword_in_value_treated_as_flag() {
        // When a value happens to end with an nginx keyword (e.g. "permanent"),
        // the fix inserts the closing quote before the keyword, treating it as
        // a flag. This is a known limitation: the fix logic cannot distinguish
        // between a rewrite flag and a value that coincidentally ends with a
        // keyword.
        let content = "add_header X-Redirect \"moved permanent;\n";
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1);
        let fix = errors[0].fixes.first().expect("Expected a fix");
        let result = apply_fix(content, fix);
        // The fix treats "permanent" as a flag and closes the quote before it,
        // even though it's part of the value.
        assert!(
            result.contains("\"moved\" permanent;"),
            "Fix splits before keyword (known limitation), got: {}",
            result
        );
    }

    #[test]
    fn test_no_fix_without_semicolon() {
        // Unclosed quote on a line without semicolon should not generate a fix
        let content = r#"server {
    content_by_lua_block '
        ngx.say("hello")
}"#;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].fixes.is_empty(),
            "Expected no fix when no semicolon on the line"
        );
    }

    #[test]
    fn test_multiple_unclosed_quotes_same_type_even() {
        // Two unclosed double quotes: the second `"` closes the first string
        // from the lexer's perspective, so neither is reported as unclosed.
        let content = r#"server {
    add_header X-A "value1;
    add_header X-B "value2;
}
"#;
        let errors = check_quotes(content);
        assert!(
            errors.is_empty(),
            "Even number of unclosed quotes pair up, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_multiple_unclosed_quotes_same_type_odd() {
        // Three unclosed double quotes: first pairs with second, third is
        // truly unclosed and reported.
        let content = r#"server {
    add_header X-A "value1;
    add_header X-B "value2;
    add_header X-C "value3;
}
"#;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("double quote"));
    }

    /// Regression test for https://github.com/walf443/nginx-lint/issues/295.
    ///
    /// `add_header`'s string is genuinely unclosed; `return`'s string on the
    /// next line is already fine. The lexer's naive pairing makes
    /// `add_header`'s opening quote look "closed" by `return`'s opening
    /// quote (spanning both lines), leaving `return`'s own closing quote as
    /// a dangling, genuinely-unclosed token to EOF — the wrong place to
    /// report and fix. The fix must redirect to the real culprit
    /// (`add_header`'s line) instead of adding a spurious `"` next to
    /// `return`'s already-correct string.
    #[test]
    fn test_fix_redirects_to_real_unclosed_line_not_dangling_artifact() {
        let content = r#"location / {
    add_header X-Custom "value;
    return 200 "ok";
}
"#;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert_eq!(
            errors[0].line,
            Some(2),
            "Error should be reported on add_header's line, not return's"
        );

        let fix = errors[0].fixes.first().expect("Expected a fix");
        let result = apply_fix(content, fix);
        assert_eq!(
            result,
            r#"location / {
    add_header X-Custom "value";
    return 200 "ok";
}
"#,
            "Fix should close add_header's quote, not add a spurious quote to return's line"
        );
    }

    /// Same redirect as `test_fix_redirects_to_real_unclosed_line_not_dangling_artifact`,
    /// but for single-quoted strings — a separate token kind, so this
    /// exercises the same-kind matching in `locate_real_culprit` for `'`.
    #[test]
    fn test_fix_redirects_to_real_unclosed_line_not_dangling_artifact_single_quote() {
        let content = r#"location / {
    add_header X-Custom 'value;
    return 200 'ok';
}
"#;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert_eq!(
            errors[0].line,
            Some(2),
            "Error should be reported on add_header's line, not return's"
        );

        let fix = errors[0].fixes.first().expect("Expected a fix");
        let result = apply_fix(content, fix);
        assert_eq!(
            result,
            r#"location / {
    add_header X-Custom 'value';
    return 200 'ok';
}
"#,
            "Fix should close add_header's quote, not add a spurious quote to return's line"
        );
    }

    /// Regression test for https://github.com/walf443/nginx-lint/issues/299.
    ///
    /// A `"` inside a trailing `#` comment makes the lexer pair the real
    /// unclosed quote's opening `"` with the comment's `"`, so the culprit
    /// token (`"value; # trailing comment with "`) is single-line and the
    /// one-hop redirect from #298 couldn't reach it. Searching the whole
    /// directive for the earliest token that swallowed a `#` past its `;`
    /// pins `add_header`'s line, leaving the `return` line untouched.
    #[test]
    fn test_fix_redirects_across_comment_embedded_quote() {
        let content = r#"add_header X-Custom "value; # trailing comment with "quote" inside
    return 200 "ok";
"#;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert_eq!(
            errors[0].line,
            Some(1),
            "Error should be reported on add_header's line"
        );

        let fix = errors[0].fixes.first().expect("Expected a fix");
        let result = apply_fix(content, fix);
        assert_eq!(
            result,
            r#"add_header X-Custom "value"; # trailing comment with "quote" inside
    return 200 "ok";
"#,
            "Fix should close add_header's quote before its ;, leaving the comment and return line intact"
        );
        // The fixed content must itself be free of unclosed-quote errors.
        assert!(
            check_quotes(&result).is_empty(),
            "fixed content should re-lint clean, got: {:?}",
            check_quotes(&result)
        );
    }

    /// A legitimately-closed value whose data contains `#` *before* its `;`
    /// (e.g. a `"#aabbcc"` colour) must not be mistaken for the culprit when
    /// a genuinely-unclosed quote follows it in the same directive — the fix
    /// must target the real unclosed token, not corrupt the colour value.
    #[test]
    fn test_fix_does_not_redirect_onto_hash_colour_value() {
        // `r##"…"##` because the content contains the `"#` sequence.
        let content = r##"add_header X-C "#aabbcc; note" "value;
"##;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);

        let fix = errors[0].fixes.first().expect("Expected a fix");
        let result = apply_fix(content, fix);
        assert_eq!(
            result,
            r##"add_header X-C "#aabbcc; note" "value";
"##,
            "Fix should close the real unclosed quote, not the colour value"
        );
    }

    #[test]
    fn test_unclosed_quote_bare_directive() {
        // Unclosed quote without any surrounding block context
        let content = "add_header X-Test \"value;\n";
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("double quote"));
        assert_eq!(errors[0].line, Some(1));
        let fix = errors[0].fixes.first().expect("Expected a fix");
        let result = apply_fix(content, fix);
        assert!(
            result.contains("\"value\""),
            "Fix should close the quote, got: {}",
            result
        );
    }

    #[test]
    fn test_lua_block_unclosed_quote_ignored() {
        // Lua comment `-- it's` contains a single quote that the nginx lexer
        // sees as the start of an unterminated SINGLE_QUOTED_STRING.
        // Tokens inside raw blocks must be skipped.
        let content = r#"http {
    server {
        content_by_lua_block {
            -- it's a lua comment
            ngx.say("hello")
        }
    }
}
"#;
        let errors = check_quotes(content);
        assert!(
            errors.is_empty(),
            "Expected no errors for quotes inside lua block, got: {:?}",
            errors
        );
    }

    /// A well-formed raw (lua) block whose content legitimately contains `"`,
    /// `;`, and `#` must be skipped entirely — and must not interfere with
    /// fixing a genuinely-unclosed quote in a later directive. The
    /// directive-scoped culprit search only looks within the flagged token's
    /// own directive, and the raw block's tokens live in a skipped BLOCK node.
    #[test]
    fn test_unclosed_quote_after_lua_block_fixes_only_the_real_one() {
        let content = r#"http {
    content_by_lua_block {
        ngx.say("hi; # not a comment")
    }
    add_header X-Custom "value;
}
"#;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert_eq!(
            errors[0].line,
            Some(5),
            "Only add_header's line should be flagged, not the lua content"
        );

        let fix = errors[0].fixes.first().expect("Expected a fix");
        let result = apply_fix(content, fix);
        assert_eq!(
            result,
            r#"http {
    content_by_lua_block {
        ngx.say("hi; # not a comment")
    }
    add_header X-Custom "value";
}
"#,
            "Fix should close add_header's quote and leave the lua block untouched"
        );
    }
}
