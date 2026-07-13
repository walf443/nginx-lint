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
        let mut last_double: Option<(String, usize)> = None;
        let mut last_single: Option<(String, usize)> = None;

        Self::walk_node(
            &root,
            &line_index,
            &mut errors,
            &mut last_double,
            &mut last_single,
        );

        errors
    }

    /// Recursively walk a CST node, collecting unclosed quote errors.
    ///
    /// BLOCK nodes belonging to raw block directives (lua etc.) are skipped
    /// entirely, avoiding per-token ancestor checks.
    ///
    /// `last_double`/`last_single` track the most recently seen
    /// DOUBLE_QUOTED_STRING/SINGLE_QUOTED_STRING token's own text and start
    /// offset, in document order across the whole file — used by
    /// [`redirect_to_prior_open_quote`](Self::redirect_to_prior_open_quote)
    /// to find the real culprit when an unrelated later quote happens to
    /// make the lexer's naive pairing land on the wrong token.
    fn walk_node(
        node: &SyntaxNode,
        line_index: &LineIndex,
        errors: &mut Vec<LintError>,
        last_double: &mut Option<(String, usize)>,
        last_single: &mut Option<(String, usize)>,
    ) {
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Node(child_node) => {
                    // Skip raw block contents
                    if child_node.kind() == SyntaxKind::BLOCK
                        && crate::parser::is_raw_block_cst_node(&child_node)
                    {
                        continue;
                    }
                    Self::walk_node(&child_node, line_index, errors, last_double, last_single);
                }
                SyntaxElement::Token(token) => {
                    let (quote_char, quote_name) = match token.kind() {
                        SyntaxKind::DOUBLE_QUOTED_STRING => ('"', "double quote"),
                        SyntaxKind::SINGLE_QUOTED_STRING => ('\'', "single quote"),
                        _ => continue,
                    };

                    let text = token.text();
                    let offset: usize = token.text_range().start().into();
                    let prior = if quote_char == '"' {
                        &mut *last_double
                    } else {
                        &mut *last_single
                    };

                    // A properly closed string starts and ends with the same quote
                    if text.len() >= 2 && text.ends_with(quote_char) {
                        // Remember it in case a later token turns out to be a
                        // dangling artifact caused by this one swallowing more
                        // than one line (see redirect_to_prior_open_quote).
                        *prior = Some((text.to_string(), offset));
                        continue;
                    }

                    // Unclosed quote detected (per the lexer's naive pairing).
                    // If this looks like a dangling artifact rather than a
                    // genuine unclosed string, redirect to the real culprit.
                    let (report_offset, fix) =
                        match Self::redirect_to_prior_open_quote(quote_char, text, prior) {
                            Some((prior_offset, fix)) => (prior_offset, Some(fix)),
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

    /// If `text` looks like a dangling artifact rather than a genuinely
    /// unclosed string, and `prior` (the preceding same-kind quoted-string
    /// token) looks like the real culprit, return a redirected `(offset,
    /// fix)` pointing at `prior` instead.
    ///
    /// A single unclosed `"`/`'` followed later by an unrelated line that
    /// happens to contain the same quote character makes the lexer pair
    /// them up: e.g. `add_header X-Custom "value;\nreturn 200 "ok";` lexes
    /// as one (nominally "closed") string spanning both lines, followed by
    /// a second string starting right after `ok`'s closing quote and
    /// running to EOF (or the next quote) — genuinely unclosed, but not the
    /// real problem. That dangling token has essentially no content of its
    /// own before its line's semicolon (it starts right where a real
    /// quote's closing character was); `prior` spanning multiple lines with
    /// a semicolon on its own first line is the actual missing-quote line.
    fn redirect_to_prior_open_quote(
        quote: char,
        text: &str,
        prior: &Option<(String, usize)>,
    ) -> Option<(usize, Fix)> {
        let first_line = text.lines().next().unwrap_or(text);
        let semicolon_pos = first_line.rfind(';')?;
        let before_semicolon = &first_line[..semicolon_pos];

        // The token always starts with a quote character, so before_semicolon
        // is at least 1 byte. Guard defensively anyway.
        if before_semicolon.is_empty() {
            return None;
        }
        if !before_semicolon[1..].trim().is_empty() {
            return None; // has real content of its own; not a dangling artifact
        }

        let (prior_text, prior_offset) = prior.as_ref()?;
        if !prior_text.contains('\n') {
            return None; // prior didn't span multiple lines, nothing to redirect
        }
        let prior_first_line = prior_text.lines().next().unwrap_or(prior_text);
        if !prior_first_line.contains(';') {
            return None;
        }

        let fix = Self::create_fix(quote, prior_text, *prior_offset)?;
        Some((*prior_offset, fix))
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
    /// but for single-quoted strings — `last_double`/`last_single` are
    /// tracked independently, so this exercises that path separately.
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
}
