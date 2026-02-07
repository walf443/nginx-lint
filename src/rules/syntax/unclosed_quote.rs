use crate::docs::RuleDoc;
use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
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

    /// Try to create a fix for an unclosed quote
    /// Analyzes the line to find the best position to insert the closing quote
    /// line_start_offset is the byte offset where the line starts in the content
    fn create_fix(
        quote: char,
        line_content: &str,
        line_start_offset: usize,
        quote_start_col: usize,
    ) -> Option<Fix> {
        let trimmed = line_content.trim_end();

        // If the line doesn't end with semicolon, we can't auto-fix
        if !trimmed.ends_with(';') {
            return None;
        }

        let semicolon_pos = trimmed.rfind(';').unwrap();
        let before_semicolon = &trimmed[..semicolon_pos];

        // Get the part after the opening quote
        // quote_start_col is 1-based column
        let quote_pos = quote_start_col - 1;
        if quote_pos >= before_semicolon.len() {
            return None;
        }

        let after_quote = &before_semicolon[quote_pos + 1..];

        // Check if there's an nginx keyword at the end that should be outside the string
        // Look for pattern: "string_content keyword" where keyword is an nginx keyword
        for keyword in Self::NGINX_KEYWORDS {
            // Check if the line ends with " keyword" (space + keyword before semicolon)
            let pattern = format!(" {}", keyword);
            if after_quote.ends_with(&pattern) {
                // Insert quote before the space preceding the keyword
                let keyword_start = before_semicolon.len() - keyword.len() - 1;
                // Use range-based fix: insert quote at the keyword_start position
                let insert_offset = line_start_offset + keyword_start;
                return Some(Fix::replace_range(
                    insert_offset,
                    insert_offset,
                    &quote.to_string(),
                ));
            }
        }

        // Default: insert quote before semicolon
        // Use range-based fix: insert quote right before the semicolon
        let insert_offset = line_start_offset + semicolon_pos;
        Some(Fix::replace_range(
            insert_offset,
            insert_offset,
            &quote.to_string(),
        ))
    }
}

impl UnclosedQuote {
    /// Check content directly (used by WASM)
    pub fn check_content(&self, content: &str) -> Vec<LintError> {
        self.check_content_impl(content)
    }

    fn check_content_impl(&self, content: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        let mut in_comment = false;
        let mut string_start: Option<(char, usize, usize, String, usize)> = None; // (quote_char, line, column, line_content, line_start_offset)
        let mut prev_char = ' ';
        let lines_vec: Vec<&str> = content.lines().collect();

        // Track byte offsets for each line
        let mut line_start_offset: usize = 0;

        for (line_num, line) in lines_vec.iter().enumerate() {
            let line_number = line_num + 1;
            let current_line_offset = line_start_offset;
            // Update offset for next line
            line_start_offset += line.len() + 1; // +1 for '\n'
            let chars: Vec<char> = line.chars().collect();

            for (col, &ch) in chars.iter().enumerate() {
                let column = col + 1;

                // Handle comments (only outside strings)
                if ch == '#' && string_start.is_none() {
                    in_comment = true;
                }

                if in_comment {
                    prev_char = ch;
                    continue;
                }

                // Start of string
                if (ch == '"' || ch == '\'') && string_start.is_none() {
                    string_start = Some((
                        ch,
                        line_number,
                        column,
                        line.to_string(),
                        current_line_offset,
                    ));
                    prev_char = ch;
                    continue;
                }

                // End string only with matching quote (and not escaped)
                if let Some((quote, _, _, _, _)) = string_start {
                    if ch == quote && prev_char != '\\' {
                        string_start = None;
                    }
                    prev_char = ch;
                    continue;
                }

                prev_char = ch;
            }

            // At end of line, check if there's an unclosed string that started on this line
            // and the line ends with semicolon (indicating a directive that should be complete)
            let trimmed = line.trim_end();
            if let Some((quote, start_line, start_col, ref start_line_content, start_line_offset)) =
                string_start
            {
                // If the string started on this line and the line ends with semicolon,
                // it's likely an unclosed quote error (not a multi-line string)
                if start_line == line_number && trimmed.ends_with(';') {
                    let quote_name = if quote == '"' {
                        "double quote"
                    } else {
                        "single quote"
                    };
                    let message = format!("Unclosed {} - missing closing {}", quote_name, quote);

                    let fix =
                        Self::create_fix(quote, start_line_content, start_line_offset, start_col);

                    let mut error =
                        LintError::new(self.name(), self.category(), &message, Severity::Error)
                            .with_location(start_line, start_col);

                    if let Some(f) = fix {
                        error = error.with_fix(f);
                    }

                    errors.push(error);
                    // Reset string_start since we've reported this error
                    string_start = None;
                }
            }

            // Reset comment flag at end of line
            in_comment = false;
            prev_char = ' '; // Reset prev_char at end of line for proper detection
        }

        // Report unclosed strings at end of file (for multi-line strings that never closed)
        if let Some((quote, start_line, start_col, start_line_content, start_line_offset)) =
            string_start
        {
            let quote_name = if quote == '"' {
                "double quote"
            } else {
                "single quote"
            };
            let message = format!("Unclosed {} - missing closing {}", quote_name, quote);

            // Try to create a fix: if the line ends with semicolon, insert quote before it
            let fix = Self::create_fix(quote, &start_line_content, start_line_offset, start_col);

            let mut error = LintError::new(self.name(), self.category(), &message, Severity::Error)
                .with_location(start_line, start_col);

            if let Some(f) = fix {
                error = error.with_fix(f);
            }

            errors.push(error);
        }

        errors
    }

    fn name(&self) -> &'static str {
        "unclosed-quote"
    }

    fn category(&self) -> &'static str {
        "syntax"
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

        self.check_content_impl(&content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::Config;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn check_quotes(content: &str) -> Vec<LintError> {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        let path = file.path().to_path_buf();

        let rule = UnclosedQuote;
        let config = Config::new();
        rule.check(&config, &path)
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
