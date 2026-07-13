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
                    // can pin the real culprit earlier in the statement, report
                    // and fix there; otherwise fix this token directly.
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

        // Close before the last semicolon on the line: for a plain unclosed
        // value that legitimately contains `;` (e.g. `"a; b;`) the final one
        // is the directive terminator.
        let semicolon_pos = first_line.rfind(';')?;
        Self::build_close_fix(quote, first_line, token_offset, semicolon_pos)
    }

    /// Build a fix that inserts `quote` before the semicolon at
    /// `semicolon_pos` (a byte index into `first_line`), unless the value
    /// ends with an nginx flag keyword (`permanent`, `redirect`, …) that
    /// belongs *outside* the string, in which case it closes before that
    /// keyword instead. `token_offset` is the token's start offset in source.
    fn build_close_fix(
        quote: char,
        first_line: &str,
        token_offset: usize,
        semicolon_pos: usize,
    ) -> Option<Fix> {
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

    /// The byte index within `first_line` of the `;` that terminates the
    /// directive: the earliest `;` whose remainder on the line is blank or a
    /// trailing `# comment`. Everything past it belongs to the next line or a
    /// comment, so the quoted value should close right before it — even when
    /// the value itself legitimately contains earlier `;`s (`"a; b"; # note`)
    /// or the comment contains later ones (`"value"; # a; b`). Returns `None`
    /// if no `;` on the line has a blank-or-comment remainder.
    fn terminator_semicolon(first_line: &str) -> Option<usize> {
        let mut from = 0;
        while let Some(rel) = first_line[from..].find(';') {
            let pos = from + rel;
            let rest = first_line[pos + 1..].trim_start();
            if rest.is_empty() || rest.starts_with('#') {
                return Some(pos);
            }
            from = pos + 1;
        }
        None
    }

    /// Find the token the user actually failed to close when the flagged
    /// `unclosed` token is really a downstream artifact of the lexer's greedy
    /// quote pairing, returning a `(offset, fix)` pointing at the real culprit.
    ///
    /// The lexer pairs quote characters greedily left-to-right and knows
    /// nothing about `#` comments or line boundaries inside a string, so a
    /// single missing quote earlier in a statement shifts every subsequent
    /// pairing and can leave a later token flagged instead. We walk backward
    /// through the token stream — stopping at the first real `;` or brace,
    /// which the mispairing can't cross (a `;`/brace *inside* a swallowed
    /// string isn't a boundary token) — and pick the earliest same-kind
    /// quoted-string token that spuriously crosses a boundary it shouldn't.
    /// This bounds the search so a correctly-closed string in another
    /// statement is never a target.
    ///
    /// A `#` after the first `;` on such a token's line, or the string running
    /// onto the next line, is the tell of a mispairing: if what follows the
    /// first `;` runs onto the next line (`"value;` … `return 200 "`, issue
    /// #295) or opens a `#` comment (`"value; # note "`, issue #299), the
    /// closing quote paired across a boundary and this token is the real
    /// culprit. The missing quote is placed before the directive's real
    /// terminator via [`terminator_semicolon`](Self::terminator_semicolon).
    /// Keying detection on content *after* the first `;` keeps
    /// legitimately-closed values whose own data contains `#` before the `;`
    /// (e.g. a `"#aabbcc"` colour) from being misread as culprits.
    ///
    /// Fundamental limitations (rare; no corruption, just an imperfect or
    /// missing fix):
    /// - A value *intended* to contain `;` then `#` (a literal `"a; #b"`) is
    ///   indistinguishable from a value plus a trailing comment.
    /// - When the mispairing exposes a `;` or brace that was meant to be raw
    ///   block content (lua code has both — `ngx.say("a; b")`, `local t = {1}`)
    ///   as a real boundary token between the culprit and the flagged token,
    ///   the backward walk stops there and the culprit isn't reached, so no fix
    ///   is offered. This is acceptable: that exposed token belongs to raw
    ///   block content, which shouldn't drive quote fixing anyway.
    ///
    /// Neither can be resolved without knowing the author's intent.
    fn locate_real_culprit(unclosed: &SyntaxToken, quote: char) -> Option<(usize, Fix)> {
        let kind = unclosed.kind();

        // Walk backward in document order, collecting same-kind quoted-string
        // tokens, until a real boundary. The greedy mispairing chain can't
        // cross a terminated directive (`;`) or a block edge (`{`/`}`) — a
        // `;`/brace *inside* a swallowed string is part of a string token, not
        // a boundary token, so it doesn't stop the walk. (The parser can split
        // the mispaired strings across sibling DIRECTIVE nodes — e.g. an
        // unclosed quote before a lua block — so searching only the flagged
        // token's own node would miss the culprit.)
        let mut candidates: Vec<SyntaxToken> = Vec::new();
        let mut cursor = unclosed.prev_token();
        while let Some(token) = cursor {
            match token.kind() {
                SyntaxKind::SEMICOLON | SyntaxKind::L_BRACE | SyntaxKind::R_BRACE => break,
                k if k == kind => candidates.push(token.clone()),
                _ => {}
            }
            cursor = token.prev_token();
        }

        // `candidates` is nearest-first; scan earliest-first for the culprit.
        for token in candidates.iter().rev() {
            let text = token.text();
            let first_line = text.lines().next().unwrap_or(text);
            // A `#` after the first `;` (comment) or the string running onto
            // the next line means the closing quote paired across a boundary.
            let Some(first_semi) = first_line.find(';') else {
                continue;
            };
            let spans_newline = text.contains('\n');
            let swallowed_comment = first_line[first_semi + 1..].contains('#');
            if spans_newline || swallowed_comment {
                // Close before the real terminator — the earliest `;` whose
                // remainder is blank or a comment (not the last `;`, which
                // could sit inside a swallowed comment). Fall back to the last
                // `;` so the fix still targets this culprit, never the flagged
                // token on another line.
                let semi =
                    Self::terminator_semicolon(first_line).or_else(|| first_line.rfind(';'))?;
                let start: usize = token.text_range().start().into();
                let fix = Self::build_close_fix(quote, first_line, start, semi)?;
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

    /// Robustness when the unclosed quote sits at the very end of the config:
    /// no trailing newline (offset arithmetic reaches EOF), and the #299 case
    /// where the dangling flagged token runs all the way to EOF (the backward
    /// walk must still reach the culprit).
    #[test]
    fn test_fix_handles_unclosed_quote_at_end_of_file() {
        // Directly at EOF, no trailing newline (apply_fix inserts in place,
        // so no trailing newline is appended here).
        let errors = check_quotes("add_header X-Custom \"value;");
        let fix = errors
            .first()
            .and_then(|e| e.fixes.first().cloned())
            .expect("Expected a fix");
        assert_eq!(
            apply_fix("add_header X-Custom \"value;", &fix),
            "add_header X-Custom \"value\";"
        );

        // #299 comment where the dangling token extends to EOF.
        let content = "add_header X-Custom \"value; # note \"quote\" inside";
        let fix = check_quotes(content)
            .first()
            .and_then(|e| e.fixes.first().cloned())
            .expect("Expected a fix");
        let result = apply_fix(content, &fix);
        assert_eq!(
            result,
            "add_header X-Custom \"value\"; # note \"quote\" inside"
        );
        assert!(check_quotes(&result).is_empty());
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

    /// When the swallowed comment itself contains a `;`, the quote must close
    /// before the directive's terminator (the `;` before the comment), not the
    /// last `;` — otherwise the fix would fold the comment text into the value
    /// (`"value; # note"`).
    #[test]
    fn test_fix_ignores_semicolon_inside_swallowed_comment() {
        let content = r#"add_header X-Custom "value; # note; with "quote" inside
    return 200 "ok";
"#;
        let fix = check_quotes(content)
            .first()
            .and_then(|e| e.fixes.first().cloned())
            .expect("Expected a fix");
        let result = apply_fix(content, &fix);
        assert_eq!(
            result,
            r#"add_header X-Custom "value"; # note; with "quote" inside
    return 200 "ok";
"#,
            "Quote must close before the terminator, keeping the comment whole"
        );
        assert!(check_quotes(&result).is_empty());
    }

    /// The mirror concern: a value that legitimately contains a `;` before its
    /// terminator (`"a; b"`) must not be split at that internal `;`. The
    /// terminator is the `;` whose remainder is a comment (or the line end),
    /// so the quote closes after the whole value.
    #[test]
    fn test_fix_keeps_value_with_internal_semicolon_intact() {
        // Comment path (embedded quote forces the mispairing) and newline path.
        let comment = r#"add_header X-Custom "a; b; # note "quote" inside
    return 200 "ok";
"#;
        let fix = check_quotes(comment)
            .first()
            .and_then(|e| e.fixes.first().cloned())
            .expect("Expected a fix");
        assert_eq!(
            apply_fix(comment, &fix),
            r#"add_header X-Custom "a; b"; # note "quote" inside
    return 200 "ok";
"#,
            "Value 'a; b' must stay whole; close before the ; that precedes the comment"
        );

        let newline = r#"add_header X-Custom "a; b;
    return 200 "ok";
"#;
        let fix = check_quotes(newline)
            .first()
            .and_then(|e| e.fixes.first().cloned())
            .expect("Expected a fix");
        assert_eq!(
            apply_fix(newline, &fix),
            r#"add_header X-Custom "a; b";
    return 200 "ok";
"#,
            "Value 'a; b' must stay whole; close before the ; that precedes the newline"
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

    /// An unclosed quote *before* a `content_by_lua_block` is the dangerous
    /// direction: the greedy lexer swallows the block-opening (`{`) and the
    /// lua body's own `"` into one string, so the block is never recognised
    /// as raw. All the resulting tokens land in one directive node, so the
    /// culprit search still redirects to `add_header`'s line, and the fixed
    /// output re-lints clean (the lua block then parses correctly).
    #[test]
    fn test_unclosed_quote_before_lua_block_fixes_the_real_one() {
        let content = r#"http {
    add_header X-Custom "value;
    content_by_lua_block {
        ngx.say("hello")
    }
}
"#;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert_eq!(
            errors[0].line,
            Some(2),
            "Error should be reported on add_header's line, not inside the lua body"
        );

        let fix = errors[0].fixes.first().expect("Expected a fix");
        let result = apply_fix(content, fix);
        assert_eq!(
            result,
            r#"http {
    add_header X-Custom "value";
    content_by_lua_block {
        ngx.say("hello")
    }
}
"#,
            "Fix should close add_header's quote, leaving the lua block intact"
        );
        assert!(
            check_quotes(&result).is_empty(),
            "fixed content should re-lint clean, got: {:?}",
            check_quotes(&result)
        );
    }

    /// A culprit that spans a newline but has no clean terminator on its first
    /// line (`"a; b` with no `;` before the newline) falls back to closing
    /// before the last `;` on the culprit's own line. The important property is
    /// that the fix targets the *culprit* line, never the dangling flagged
    /// token on a later line (which would spuriously double-quote it).
    #[test]
    fn test_fix_targets_culprit_line_when_no_clean_terminator() {
        let content = r#"add_header X-Custom "a; b
    return 200 "ok";
"#;
        let errors = check_quotes(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        let result = apply_fix(content, errors[0].fixes.first().expect("Expected a fix"));
        // The already-correct `return 200 "ok";` line must be untouched.
        assert!(
            result.contains(r#"    return 200 "ok";"#),
            "the return line must not be corrupted, got:\n{result}"
        );
        assert!(check_quotes(&result).is_empty());
    }

    /// Limitation guard: an unclosed quote before a lua block whose body has a
    /// `;` can't be auto-fixed (the exposed lua `;` stops the culprit search),
    /// but it must still be *reported* and must never corrupt the lua body.
    #[test]
    fn test_unclosed_quote_before_lua_block_with_semicolon_is_safe() {
        let content = r#"http {
    add_header X-Custom "value;
    content_by_lua_block {
        ngx.say("a; b")
    }
}
"#;
        let errors = check_quotes(content);
        assert!(
            !errors.is_empty(),
            "the unclosed quote must still be reported"
        );
        // Applying whatever fix (if any) must not corrupt the lua body.
        let fixes: Vec<_> = errors.iter().flat_map(|e| e.fixes.iter()).collect();
        let (result, _) = crate::apply_fixes_to_content(content, &fixes);
        assert!(
            result.contains(r#"        ngx.say("a; b")"#),
            "the lua body must stay intact, got:\n{result}"
        );
    }

    /// Safety invariant across adversarial / doubly-broken inputs: applying
    /// the offered fixes must never *increase* the unclosed-quote error count.
    /// When the culprit can't be confidently located the rule declines to fix
    /// (a no-op) rather than corrupting a line — it never makes things worse.
    #[test]
    fn test_fixes_never_increase_error_count() {
        let cases = [
            "foo \"a; b\nbar; \"ok\";\n",
            "map $x $y {\n    default \"a;\n}\nadd_header X-C \"value;\n",
            "server {\n    listen 80;\n    add_header \"a; # x\nreturn 200 \"b\";\n}\n",
            "http { \"a; } \"b; } \"c;\n",
            "x y;\n\"z;\n",
            "a \"b\" \"c\" \"d;\n",
            "location / {\n  try_files $uri \"a; b\n  index \"x;\n}\n",
            "content_by_lua_block {\n    ngx.say(\"a; b\")\nadd_header X-C \"value;\n}\n",
        ];
        for content in cases {
            let before = check_quotes(content).len();
            let fixes: Vec<_> = check_quotes(content)
                .iter()
                .flat_map(|e| e.fixes.clone())
                .collect();
            let fix_refs: Vec<&Fix> = fixes.iter().collect();
            let (result, _) = crate::apply_fixes_to_content(content, &fix_refs);
            let after = check_quotes(&result).len();
            assert!(
                after <= before,
                "fix increased error count ({before} -> {after}) for {content:?}\n -> {result:?}"
            );
        }
    }
}
