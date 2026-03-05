//! Rowan-compatible lexer for nginx configuration files.
//!
//! Produces a flat sequence of `(SyntaxKind, &str)` pairs where every byte
//! of the input is covered (whitespace and newlines are explicit tokens).
//! This is the input expected by the rowan-based [`parser`](crate::parser).

use crate::syntax_kind::SyntaxKind;

/// Tokenise `source` into a lossless sequence of `(SyntaxKind, text)` pairs.
///
/// Every byte of the input is represented exactly once, so
/// `tokens.iter().map(|(_, t)| *t).collect::<String>() == source` always holds.
pub fn tokenize(source: &str) -> Vec<(SyntaxKind, &str)> {
    let mut lexer = RowanLexer::new(source);
    lexer.tokenize_all()
}

/// Internal lexer state.
struct RowanLexer<'a> {
    source: &'a str,
    pos: usize,
    tokens: Vec<(SyntaxKind, &'a str)>,
}

impl<'a> RowanLexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            pos: 0,
            tokens: Vec::new(),
        }
    }

    fn remaining(&self) -> &'a str {
        &self.source[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    /// Peek at the character at offset `n` from current position.
    fn peek_at(&self, n: usize) -> Option<char> {
        self.remaining().chars().nth(n)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.source.len()
    }

    /// Advance by one character (its UTF-8 byte length) and return it.
    fn advance_char(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn emit(&mut self, kind: SyntaxKind, start: usize) {
        let text = &self.source[start..self.pos];
        if !text.is_empty() {
            self.tokens.push((kind, text));
        }
    }

    fn tokenize_all(&mut self) -> Vec<(SyntaxKind, &'a str)> {
        // Track whether the previous non-whitespace token was whitespace-preceded
        // for comment detection (# is only a comment after whitespace or at line start).
        let mut at_line_start = true;

        while !self.at_end() {
            let start = self.pos;
            let ch = self.peek().unwrap();

            match ch {
                '\n' => {
                    self.advance_char();
                    self.emit(SyntaxKind::NEWLINE, start);
                    at_line_start = true;
                }
                ' ' | '\t' => {
                    self.eat_whitespace();
                    self.emit(SyntaxKind::WHITESPACE, start);
                    // at_line_start stays as-is (whitespace doesn't reset it)
                }
                '#' if at_line_start || self.preceded_by_whitespace() => {
                    self.eat_comment();
                    self.emit(SyntaxKind::COMMENT, start);
                    at_line_start = false;
                }
                ';' => {
                    self.advance_char();
                    self.emit(SyntaxKind::SEMICOLON, start);
                    at_line_start = false;
                }
                '{' => {
                    self.advance_char();
                    self.emit(SyntaxKind::L_BRACE, start);
                    at_line_start = false;
                }
                '}' => {
                    self.advance_char();
                    self.emit(SyntaxKind::R_BRACE, start);
                    at_line_start = false;
                }
                '"' => {
                    self.eat_double_quoted_string();
                    self.emit(SyntaxKind::DOUBLE_QUOTED_STRING, start);
                    at_line_start = false;
                }
                '\'' => {
                    self.eat_single_quoted_string();
                    self.emit(SyntaxKind::SINGLE_QUOTED_STRING, start);
                    at_line_start = false;
                }
                '$' => {
                    self.eat_variable();
                    self.emit(SyntaxKind::VARIABLE, start);
                    at_line_start = false;
                }
                _ if is_ident_start(ch) => {
                    // Could be IDENT or ARGUMENT (identifiers that continue
                    // with argument-chars like / or . become ARGUMENT).
                    self.eat_ident_or_argument();
                    let text = &self.source[start..self.pos];
                    let kind = if text
                        .chars()
                        .all(|c| is_ident_continue(c) || is_ident_start(c))
                    {
                        SyntaxKind::IDENT
                    } else {
                        SyntaxKind::ARGUMENT
                    };
                    self.tokens.push((kind, text));
                    at_line_start = false;
                }
                _ if is_argument_char(ch) => {
                    self.eat_argument(ch);
                    self.emit(SyntaxKind::ARGUMENT, start);
                    at_line_start = false;
                }
                _ => {
                    // Unknown character — emit as ERROR token.
                    self.advance_char();
                    self.emit(SyntaxKind::ERROR, start);
                    at_line_start = false;
                }
            }
        }

        std::mem::take(&mut self.tokens)
    }

    // ── whitespace / comment ────────────────────────────────────────

    fn eat_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == ' ' || ch == '\t' {
                self.advance_char();
            } else {
                break;
            }
        }
    }

    fn eat_comment(&mut self) {
        // Consume '#' and everything until (but not including) '\n'.
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                break;
            }
            self.advance_char();
        }
    }

    /// Check if the immediately preceding token was whitespace or we are at
    /// the beginning of a line.
    fn preceded_by_whitespace(&self) -> bool {
        matches!(
            self.tokens.last(),
            Some((SyntaxKind::WHITESPACE, _)) | Some((SyntaxKind::NEWLINE, _)) | None
        )
    }

    // ── strings ─────────────────────────────────────────────────────

    fn eat_double_quoted_string(&mut self) {
        // Opening quote
        self.advance_char(); // "
        loop {
            match self.peek() {
                None => break, // Unterminated at EOF
                Some('\\') => {
                    self.advance_char(); // backslash
                    self.advance_char(); // escaped char (if any)
                }
                Some('"') => {
                    self.advance_char(); // closing quote
                    break;
                }
                Some(_) => {
                    self.advance_char();
                }
            }
        }
    }

    fn eat_single_quoted_string(&mut self) {
        self.advance_char(); // opening '
        loop {
            match self.peek() {
                None => break, // Unterminated at EOF
                Some('\\') => {
                    self.advance_char();
                    self.advance_char();
                }
                Some('\'') => {
                    self.advance_char();
                    break;
                }
                Some(_) => {
                    self.advance_char();
                }
            }
        }
    }

    // ── variable ────────────────────────────────────────────────────

    fn eat_variable(&mut self) {
        self.advance_char(); // '$'
        if self.peek() == Some('{') {
            // ${var} syntax
            self.advance_char(); // '{'
            while let Some(ch) = self.peek() {
                if ch == '}' {
                    self.advance_char();
                    break;
                }
                self.advance_char();
            }
        } else {
            // $var syntax
            while let Some(ch) = self.peek() {
                if ch.is_alphanumeric() || ch == '_' {
                    self.advance_char();
                } else {
                    break;
                }
            }
        }
    }

    // ── identifier / argument ───────────────────────────────────────

    fn eat_ident_or_argument(&mut self) {
        // Read identifier characters first
        while let Some(ch) = self.peek() {
            if is_ident_continue(ch) || is_ident_start(ch) {
                self.advance_char();
            } else {
                break;
            }
        }
        // Continue reading argument characters if present
        self.eat_argument_continuation();
    }

    fn eat_argument(&mut self, _first: char) {
        self.advance_char();
        self.eat_argument_continuation();
    }

    /// Continue reading argument characters including regex quantifiers
    /// like `{8,}` and escaped braces like `\{` and `\}`.
    fn eat_argument_continuation(&mut self) {
        while let Some(ch) = self.peek() {
            if is_argument_char(ch) || is_ident_continue(ch) || is_ident_start(ch) {
                // Check for escaped brace
                if ch == '\\' && matches!(self.peek_at(1), Some('{') | Some('}')) {
                    self.advance_char(); // '\'
                    self.advance_char(); // '{' or '}'
                    continue;
                }
                self.advance_char();
            } else if ch == '{' {
                // Check for regex quantifier
                if let Some(len) = self.peek_regex_quantifier() {
                    for _ in 0..len {
                        self.advance_char();
                    }
                } else {
                    break;
                }
            } else if ch == '$' {
                // Regex end anchor vs variable
                if self.is_regex_end_anchor() {
                    self.advance_char();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    /// Check if `$` at current position is a regex end anchor rather than
    /// a variable reference.
    fn is_regex_end_anchor(&self) -> bool {
        let remaining = self.remaining();
        let mut chars = remaining.chars();
        if chars.next() != Some('$') {
            return false;
        }
        match chars.next() {
            None => true,
            Some(c) if c.is_whitespace() => true,
            Some('{') => false, // ${var}
            Some(c) if c.is_alphanumeric() => false,
            Some('_') => false,
            _ => true,
        }
    }

    /// Look ahead at a potential regex quantifier like `{8}`, `{1,3}`,
    /// `{8,}`.  Returns the byte-length if found, `None` otherwise.
    fn peek_regex_quantifier(&self) -> Option<usize> {
        let remaining = self.remaining();
        if !remaining.starts_with('{') {
            return None;
        }
        let mut chars = remaining.char_indices().peekable();
        chars.next(); // '{'

        // Must have at least one digit
        match chars.peek() {
            Some((_, ch)) if ch.is_ascii_digit() => {
                chars.next();
            }
            _ => return None,
        }
        // More digits
        while let Some(&(_, ch)) = chars.peek() {
            if ch.is_ascii_digit() {
                chars.next();
            } else {
                break;
            }
        }
        match chars.peek() {
            Some(&(idx, '}')) => {
                let _ = idx;
                // Byte length from '{' up to and including '}'
                chars.next();
                let end_offset = chars.peek().map(|(i, _)| *i).unwrap_or(remaining.len());
                Some(end_offset)
            }
            Some(&(_, ',')) => {
                chars.next();
                while let Some(&(_, ch)) = chars.peek() {
                    if ch.is_ascii_digit() {
                        chars.next();
                    } else {
                        break;
                    }
                }
                if chars.peek().map(|(_, ch)| *ch) == Some('}') {
                    chars.next();
                    let end_offset = chars.peek().map(|(i, _)| *i).unwrap_or(remaining.len());
                    Some(end_offset)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

// ── Character classification (mirrors existing lexer.rs) ────────────────

fn is_ident_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-'
}

fn is_argument_char(ch: char) -> bool {
    !ch.is_whitespace() && !matches!(ch, ';' | '{' | '}' | '"' | '\'' | '$')
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: collect just the kinds.
    fn kinds(source: &str) -> Vec<SyntaxKind> {
        tokenize(source).into_iter().map(|(k, _)| k).collect()
    }

    /// The concatenation of all token texts must equal the original source.
    fn assert_lossless(source: &str) {
        let tokens = tokenize(source);
        let reconstructed: String = tokens.iter().map(|(_, t)| *t).collect();
        assert_eq!(reconstructed, source, "lossless round-trip failed");
    }

    #[test]
    fn empty_input() {
        assert_eq!(tokenize(""), vec![]);
    }

    #[test]
    fn simple_directive() {
        let tokens = tokenize("listen 80;");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "listen"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "80"),
                (SyntaxKind::SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn block_directive() {
        let tokens = tokenize("http { }");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "http"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::L_BRACE, "{"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::R_BRACE, "}"),
            ]
        );
    }

    #[test]
    fn double_quoted_string() {
        let tokens = tokenize(r#"return 200 "hello world";"#);
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "return"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "200"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::DOUBLE_QUOTED_STRING, "\"hello world\""),
                (SyntaxKind::SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn single_quoted_string() {
        let tokens = tokenize("return 200 'hello world';");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "return"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "200"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::SINGLE_QUOTED_STRING, "'hello world'"),
                (SyntaxKind::SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn variable() {
        let tokens = tokenize("set $var value;");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "set"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::VARIABLE, "$var"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::IDENT, "value"),
                (SyntaxKind::SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn variable_braces() {
        let tokens = tokenize("return 200 ${request_uri};");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "return"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "200"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::VARIABLE, "${request_uri}"),
                (SyntaxKind::SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn comment() {
        let tokens = tokenize("# this is a comment\nlisten 80;");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::COMMENT, "# this is a comment"),
                (SyntaxKind::NEWLINE, "\n"),
                (SyntaxKind::IDENT, "listen"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "80"),
                (SyntaxKind::SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn path_argument() {
        let tokens = tokenize("root /var/www/html;");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "root"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "/var/www/html"),
                (SyntaxKind::SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn newlines_and_whitespace() {
        let source = "http {\n    listen 80;\n}";
        assert_lossless(source);
        let tokens = tokenize(source);
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "http"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::L_BRACE, "{"),
                (SyntaxKind::NEWLINE, "\n"),
                (SyntaxKind::WHITESPACE, "    "),
                (SyntaxKind::IDENT, "listen"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "80"),
                (SyntaxKind::SEMICOLON, ";"),
                (SyntaxKind::NEWLINE, "\n"),
                (SyntaxKind::R_BRACE, "}"),
            ]
        );
    }

    #[test]
    fn regex_quantifier() {
        let tokens = tokenize(r"location ~ ^/[a-z]{8}$ {");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "location"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "~"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "^/[a-z]{8}$"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::L_BRACE, "{"),
            ]
        );
    }

    #[test]
    fn regex_quantifier_range() {
        let tokens = tokenize(r"location ~ ^/[0-9]{1,3}$ {");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "location"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "~"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "^/[0-9]{1,3}$"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::L_BRACE, "{"),
            ]
        );
    }

    #[test]
    fn escaped_braces_in_regex() {
        let tokens = tokenize(r"location ~ ^/nested/\{[a-z]+\}$ {");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "location"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "~"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, r"^/nested/\{[a-z]+\}$"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::L_BRACE, "{"),
            ]
        );
    }

    #[test]
    fn hash_in_argument() {
        let tokens = tokenize("location ~* foo#bar {");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "location"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "~*"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "foo#bar"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::L_BRACE, "{"),
            ]
        );
    }

    #[test]
    fn hash_comment_after_whitespace() {
        let tokens = tokenize("listen 80; # this is a comment");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "listen"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "80"),
                (SyntaxKind::SEMICOLON, ";"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::COMMENT, "# this is a comment"),
            ]
        );
    }

    #[test]
    fn escape_in_double_quoted_string() {
        let tokens = tokenize(r#"return 200 "hello\nworld";"#);
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "return"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "200"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::DOUBLE_QUOTED_STRING, r#""hello\nworld""#),
                (SyntaxKind::SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn lossless_complex_config() {
        let source = r#"http {
    # Main server
    server {
        listen 80;
        server_name example.com;
        location / {
            proxy_pass http://backend;
        }
    }
}
"#;
        assert_lossless(source);
    }

    #[test]
    fn lossless_utf8() {
        let source = "# これは日本語コメント\nlisten 80;\n";
        assert_lossless(source);
    }

    #[test]
    fn glob_pattern() {
        let tokens = tokenize("include /etc/nginx/conf.d/*.conf;");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "include"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "/etc/nginx/conf.d/*.conf"),
                (SyntaxKind::SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn extension_directive() {
        let tokens = tokenize(r#"more_set_headers "Server: Custom";"#);
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "more_set_headers"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::DOUBLE_QUOTED_STRING, "\"Server: Custom\""),
                (SyntaxKind::SEMICOLON, ";"),
            ]
        );
    }

    #[test]
    fn hash_in_regex_pattern() {
        let tokens = tokenize(r"location ~* (?:#.*#|\.bak)$ {");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::IDENT, "location"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, "~*"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::ARGUMENT, r"(?:#.*#|\.bak)$"),
                (SyntaxKind::WHITESPACE, " "),
                (SyntaxKind::L_BRACE, "{"),
            ]
        );
    }

    #[test]
    fn ident_classification() {
        // Pure identifiers should be IDENT
        let tokens = tokenize("server_name example;");
        assert_eq!(kinds("server_name"), vec![SyntaxKind::IDENT]);
        // Identifiers with argument chars become ARGUMENT
        let toks = tokenize("text/plain");
        assert_eq!(toks, vec![(SyntaxKind::ARGUMENT, "text/plain")]);
    }
}
