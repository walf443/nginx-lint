use crate::docs::RuleDoc;
use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use crate::parser::line_index::LineIndex;
use crate::parser::parse_string_rowan;
use crate::parser::syntax_kind::SyntaxKind;
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
    /// Walks all tokens in the rowan CST and checks whether quoted-string
    /// tokens are properly terminated.
    fn check_cst(&self, source: &str) -> Vec<LintError> {
        let (root, _syntax_errors) = parse_string_rowan(source);
        let line_index = LineIndex::new(source);
        let mut errors = Vec::new();

        for element in root.descendants_with_tokens() {
            let token = match element.as_token() {
                Some(t) => t,
                None => continue,
            };

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

            // Unclosed quote detected
            let offset: usize = token.text_range().start().into();
            let pos = line_index.position(offset);
            let message = format!("Unclosed {} - missing closing {}", quote_name, quote_char);

            let fix = Self::create_fix(quote_char, text, offset);

            let mut error = LintError::new("unclosed-quote", "syntax", &message, Severity::Error)
                .with_location(pos.line, pos.column);

            if let Some(f) = fix {
                error = error.with_fix(f);
            }

            errors.push(error);
        }

        errors
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

        // Content after the opening quote character
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_quotes(content: &str) -> Vec<LintError> {
        let rule = UnclosedQuote;
        rule.check_content(content)
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
}
