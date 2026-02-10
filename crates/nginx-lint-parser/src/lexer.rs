use crate::ast::{Position, Span};
use crate::error::{LexerError, ParseResult};

/// Token types for nginx configuration
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// Identifier or keyword (http, server, listen, more_set_headers, etc.)
    Ident(String),
    /// Unquoted argument (80, /path/to/file, on, off, etc.)
    /// Arguments can contain special chars like *, ?, etc.
    Argument(String),
    /// Double-quoted string (includes the processed content without quotes)
    DoubleQuotedString(String),
    /// Single-quoted string (includes the processed content without quotes)
    SingleQuotedString(String),
    /// Variable ($variable_name)
    Variable(String),
    /// Semicolon ;
    Semicolon,
    /// Open brace {
    OpenBrace,
    /// Close brace }
    CloseBrace,
    /// Comment (# ...)
    Comment(String),
    /// Newline (for tracking blank lines)
    Newline,
    /// End of file
    Eof,
}

impl TokenKind {
    /// Returns a human-readable name for this token kind, used in error messages.
    pub fn display_name(&self) -> &str {
        match self {
            TokenKind::Ident(_) => "identifier",
            TokenKind::Argument(_) => "argument",
            TokenKind::DoubleQuotedString(_) => "string",
            TokenKind::SingleQuotedString(_) => "string",
            TokenKind::Variable(_) => "variable",
            TokenKind::Semicolon => "';'",
            TokenKind::OpenBrace => "'{'",
            TokenKind::CloseBrace => "'}'",
            TokenKind::Comment(_) => "comment",
            TokenKind::Newline => "newline",
            TokenKind::Eof => "end of file",
        }
    }
}

/// A token with its position in the source.
#[derive(Debug, Clone)]
pub struct Token {
    /// The kind and optional payload of this token.
    pub kind: TokenKind,
    /// Source span of the token (excluding leading whitespace).
    pub span: Span,
    /// Original source text of the token (e.g. `"hello"` including quotes).
    pub raw: String,
    /// Whitespace characters that appeared before this token on the same line.
    pub leading_whitespace: String,
}

/// Lexer for tokenizing nginx configuration files.
///
/// Converts source text into a stream of [`Token`]s. Use [`new`](Lexer::new) to
/// create a lexer and [`tokenize`](Lexer::tokenize) to consume the entire input.
pub struct Lexer<'a> {
    source: &'a str,
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    line: usize,
    column: usize,
    offset: usize,
}

impl<'a> Lexer<'a> {
    /// Creates a new lexer for the given source text.
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: source.char_indices().peekable(),
            line: 1,
            column: 1,
            offset: 0,
        }
    }

    fn position(&self) -> Position {
        Position::new(self.line, self.column, self.offset)
    }

    fn advance(&mut self) -> Option<(usize, char)> {
        if let Some((idx, ch)) = self.chars.next() {
            self.offset = idx + ch.len_utf8();
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            Some((idx, ch))
        } else {
            None
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().map(|(_, ch)| *ch)
    }

    fn skip_whitespace_same_line(&mut self) -> String {
        let mut whitespace = String::new();
        while let Some(ch) = self.peek() {
            if ch == ' ' || ch == '\t' {
                whitespace.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        whitespace
    }

    pub fn next_token(&mut self) -> ParseResult<Token> {
        let leading_whitespace = self.skip_whitespace_same_line();

        let start_pos = self.position();
        let start_offset = self.offset;

        let Some((_, ch)) = self.advance() else {
            return Ok(Token {
                kind: TokenKind::Eof,
                span: Span::new(start_pos, start_pos),
                raw: String::new(),
                leading_whitespace,
            });
        };

        let kind = match ch {
            '\n' => TokenKind::Newline,
            ';' => TokenKind::Semicolon,
            '{' => TokenKind::OpenBrace,
            '}' => TokenKind::CloseBrace,
            '#' if !leading_whitespace.is_empty() || start_pos.column == 1 => {
                // Comment - only when preceded by whitespace or at start of line
                let mut text = String::from('#');
                while let Some(c) = self.peek() {
                    if c == '\n' {
                        break;
                    }
                    text.push(c);
                    self.advance();
                }
                TokenKind::Comment(text)
            }
            '#' => {
                // # not preceded by whitespace - treat as part of argument
                let value = self.read_argument(ch);
                TokenKind::Argument(value)
            }
            '"' => self.read_double_quoted_string(start_pos)?,
            '\'' => self.read_single_quoted_string(start_pos)?,
            '$' => {
                // Variable
                let name = self.read_variable_name();
                TokenKind::Variable(name)
            }
            _ if is_ident_start(ch) => {
                // Identifier or argument
                let value = self.read_identifier(ch);
                TokenKind::Ident(value)
            }
            _ if is_argument_char(ch) => {
                // Unquoted argument (numbers, paths, etc.)
                let value = self.read_argument(ch);
                TokenKind::Argument(value)
            }
            _ => {
                return Err(LexerError::UnexpectedChar {
                    ch,
                    position: start_pos,
                }
                .into());
            }
        };

        let end_pos = self.position();
        let raw = self.source[start_offset..self.offset].to_string();

        Ok(Token {
            kind,
            span: Span::new(start_pos, end_pos),
            raw,
            leading_whitespace,
        })
    }

    fn read_double_quoted_string(&mut self, start_pos: Position) -> ParseResult<TokenKind> {
        let mut value = String::new();

        loop {
            match self.advance() {
                Some((_, '"')) => break,
                Some((_, '\\')) => {
                    // Escape sequence
                    match self.advance() {
                        Some((_, 'n')) => value.push('\n'),
                        Some((_, 't')) => value.push('\t'),
                        Some((_, 'r')) => value.push('\r'),
                        Some((_, '\\')) => value.push('\\'),
                        Some((_, '"')) => value.push('"'),
                        Some((_, '$')) => value.push('$'),
                        Some((_, c)) => {
                            // For unknown escapes, keep the backslash and char
                            value.push('\\');
                            value.push(c);
                        }
                        None => {
                            return Err(LexerError::UnterminatedString {
                                position: start_pos,
                            }
                            .into());
                        }
                    }
                }
                Some((_, ch)) => value.push(ch),
                None => {
                    return Err(LexerError::UnterminatedString {
                        position: start_pos,
                    }
                    .into());
                }
            }
        }

        Ok(TokenKind::DoubleQuotedString(value))
    }

    fn read_single_quoted_string(&mut self, start_pos: Position) -> ParseResult<TokenKind> {
        let mut value = String::new();

        loop {
            match self.advance() {
                Some((_, '\'')) => break,
                Some((_, '\\')) => {
                    // Escape sequence
                    match self.advance() {
                        Some((_, '\\')) => value.push('\\'),
                        Some((_, '\'')) => value.push('\''),
                        Some((_, c)) => {
                            // For unknown escapes, keep the backslash and char
                            value.push('\\');
                            value.push(c);
                        }
                        None => {
                            return Err(LexerError::UnterminatedString {
                                position: start_pos,
                            }
                            .into());
                        }
                    }
                }
                Some((_, ch)) => value.push(ch),
                None => {
                    return Err(LexerError::UnterminatedString {
                        position: start_pos,
                    }
                    .into());
                }
            }
        }

        Ok(TokenKind::SingleQuotedString(value))
    }

    fn read_variable_name(&mut self) -> String {
        let mut name = String::new();

        // Check for ${var} syntax
        if self.peek() == Some('{') {
            self.advance(); // consume '{'
            while let Some(ch) = self.peek() {
                if ch == '}' {
                    self.advance();
                    break;
                }
                name.push(ch);
                self.advance();
            }
        } else {
            // Regular $var syntax
            while let Some(ch) = self.peek() {
                if ch.is_alphanumeric() || ch == '_' {
                    name.push(ch);
                    self.advance();
                } else {
                    break;
                }
            }
        }

        name
    }

    fn read_identifier(&mut self, first: char) -> String {
        let mut value = String::from(first);

        // Read identifier characters first
        while let Some(ch) = self.peek() {
            if is_ident_continue(ch) {
                value.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // Continue reading if we have argument characters (like / or .)
        // This handles cases like "text/plain", "TLSv1.2", etc.
        self.read_argument_continuation(&mut value);

        value
    }

    fn read_argument(&mut self, first: char) -> String {
        let mut value = String::from(first);
        self.read_argument_continuation(&mut value);
        value
    }

    /// Continue reading argument characters, including regex quantifiers like {8,}
    /// and escaped braces like \{ and \}
    fn read_argument_continuation(&mut self, value: &mut String) {
        while let Some(ch) = self.peek() {
            if is_argument_char(ch) || is_ident_continue(ch) {
                // Check for escaped brace: if current char is '\' and next is '{' or '}'
                if ch == '\\'
                    && let Some(escaped) = self.peek_escaped_brace()
                {
                    value.push('\\');
                    self.advance(); // consume '\'
                    value.push(escaped);
                    self.advance(); // consume '{' or '}'
                    continue;
                }
                value.push(ch);
                self.advance();
            } else if ch == '{' {
                // Check if this looks like a regex quantifier using lookahead
                if let Some(quantifier) = self.peek_regex_quantifier() {
                    // Consume the quantifier
                    for _ in 0..quantifier.len() {
                        self.advance();
                    }
                    value.push_str(&quantifier);
                } else {
                    break;
                }
            } else if ch == '$' {
                // Check if this is a regex end anchor ($) rather than a variable
                // If $ is followed by whitespace or {, it's part of the regex pattern
                if self.is_regex_end_anchor() {
                    value.push(ch);
                    self.advance();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    /// Check if $ is a regex end anchor (followed by whitespace or {)
    fn is_regex_end_anchor(&self) -> bool {
        let remaining = &self.source[self.offset..];
        let mut chars = remaining.chars();

        // First char should be '$'
        if chars.next() != Some('$') {
            return false;
        }

        // Check what follows $
        match chars.next() {
            None => true,                            // End of input
            Some(c) if c.is_whitespace() => true,    // Followed by whitespace
            Some('{') => false, // Followed by '{' - this is ${var} syntax, not end anchor
            Some(c) if c.is_alphanumeric() => false, // Followed by variable name
            Some('_') => false, // Followed by variable name
            _ => true,          // Other chars - treat as end anchor
        }
    }

    /// Peek ahead to check if we have an escaped brace (\{ or \})
    /// Returns the brace character if found
    fn peek_escaped_brace(&self) -> Option<char> {
        let remaining = &self.source[self.offset..];
        let mut chars = remaining.chars();

        // First char should be '\'
        if chars.next() != Some('\\') {
            return None;
        }

        // Second char should be '{' or '}'
        match chars.next() {
            Some('{') => Some('{'),
            Some('}') => Some('}'),
            _ => None,
        }
    }

    /// Peek ahead to check if we have a regex quantifier pattern like {8}, {8,}, {1,3}
    /// This doesn't consume any characters, just looks ahead in the source
    fn peek_regex_quantifier(&self) -> Option<String> {
        // Get remaining source from current position
        let remaining = &self.source[self.offset..];

        // Must start with '{'
        if !remaining.starts_with('{') {
            return None;
        }

        let mut chars = remaining.chars().peekable();
        chars.next(); // consume '{'

        let mut quantifier = String::from("{");

        // Must have at least one digit
        match chars.peek() {
            Some(ch) if ch.is_ascii_digit() => {
                quantifier.push(*ch);
                chars.next();
            }
            _ => return None,
        }

        // Read more digits
        while let Some(&ch) = chars.peek() {
            if ch.is_ascii_digit() {
                quantifier.push(ch);
                chars.next();
            } else {
                break;
            }
        }

        // Check for ',' or '}'
        match chars.peek() {
            Some('}') => {
                quantifier.push('}');
                Some(quantifier)
            }
            Some(',') => {
                quantifier.push(',');
                chars.next();

                // Read optional second number
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_digit() {
                        quantifier.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }

                // Must end with '}'
                if chars.peek() == Some(&'}') {
                    quantifier.push('}');
                    Some(quantifier)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Tokenize the entire input and return all tokens
    pub fn tokenize(&mut self) -> ParseResult<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token()?;
            let is_eof = matches!(token.kind, TokenKind::Eof);
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }
}

/// Check if a character can start an identifier
fn is_ident_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_'
}

/// Check if a character can continue an identifier
fn is_ident_continue(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-'
}

/// Check if a character is valid in an unquoted argument
fn is_argument_char(ch: char) -> bool {
    // Arguments can contain most characters except whitespace and special chars
    // Note: '#' is allowed inside arguments (e.g., regex patterns like (?:#.*#|...))
    // '#' only starts a comment when preceded by whitespace
    !ch.is_whitespace() && !matches!(ch, ';' | '{' | '}' | '"' | '\'' | '$')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(source: &str) -> Vec<TokenKind> {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize().unwrap();
        tokens.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn test_simple_directive() {
        let tokens = tokenize("listen 80;");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("listen".to_string()),
                TokenKind::Argument("80".to_string()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_block() {
        let tokens = tokenize("http { }");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("http".to_string()),
                TokenKind::OpenBrace,
                TokenKind::CloseBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_double_quoted_string() {
        let tokens = tokenize(r#"return 200 "hello world";"#);
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("return".to_string()),
                TokenKind::Argument("200".to_string()),
                TokenKind::DoubleQuotedString("hello world".to_string()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_single_quoted_string() {
        let tokens = tokenize("return 200 'hello world';");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("return".to_string()),
                TokenKind::Argument("200".to_string()),
                TokenKind::SingleQuotedString("hello world".to_string()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_escape_sequences() {
        let tokens = tokenize(r#"return 200 "hello\nworld";"#);
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("return".to_string()),
                TokenKind::Argument("200".to_string()),
                TokenKind::DoubleQuotedString("hello\nworld".to_string()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_variable() {
        let tokens = tokenize("set $var value;");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("set".to_string()),
                TokenKind::Variable("var".to_string()),
                TokenKind::Ident("value".to_string()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_variable_braces() {
        let tokens = tokenize("return 200 ${request_uri};");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("return".to_string()),
                TokenKind::Argument("200".to_string()),
                TokenKind::Variable("request_uri".to_string()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_comment() {
        let tokens = tokenize("# this is a comment\nlisten 80;");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Comment("# this is a comment".to_string()),
                TokenKind::Newline,
                TokenKind::Ident("listen".to_string()),
                TokenKind::Argument("80".to_string()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_path_argument() {
        let tokens = tokenize("root /var/www/html;");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("root".to_string()),
                TokenKind::Argument("/var/www/html".to_string()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_extension_directive() {
        let tokens = tokenize(r#"more_set_headers "Server: Custom";"#);
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("more_set_headers".to_string()),
                TokenKind::DoubleQuotedString("Server: Custom".to_string()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_glob_pattern() {
        let tokens = tokenize("include /etc/nginx/conf.d/*.conf;");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("include".to_string()),
                TokenKind::Argument("/etc/nginx/conf.d/*.conf".to_string()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_utf8_comment() {
        let tokens = tokenize("# これは日本語コメント\nlisten 80;");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Comment("# これは日本語コメント".to_string()),
                TokenKind::Newline,
                TokenKind::Ident("listen".to_string()),
                TokenKind::Argument("80".to_string()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_utf8_string() {
        let tokens = tokenize(r#"return 200 "こんにちは";"#);
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("return".to_string()),
                TokenKind::Argument("200".to_string()),
                TokenKind::DoubleQuotedString("こんにちは".to_string()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_position_tracking() {
        let mut lexer = Lexer::new("http {\n    listen 80;\n}");
        let tokens = lexer.tokenize().unwrap();

        // "http" at line 1, column 1
        assert_eq!(tokens[0].span.start.line, 1);
        assert_eq!(tokens[0].span.start.column, 1);

        // "{" at line 1, column 6
        assert_eq!(tokens[1].span.start.line, 1);
        assert_eq!(tokens[1].span.start.column, 6);

        // newline at end of line 1
        assert_eq!(tokens[2].span.start.line, 1);

        // "listen" at line 2, column 5
        assert_eq!(tokens[3].span.start.line, 2);
        assert_eq!(tokens[3].span.start.column, 5);
    }

    #[test]
    fn test_regex_quantifier() {
        // Regex quantifier {8} should be part of the argument
        let tokens = tokenize(r"location ~ ^/[a-z]{8}$ {");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("location".to_string()),
                TokenKind::Argument("~".to_string()),
                TokenKind::Argument("^/[a-z]{8}$".to_string()),
                TokenKind::OpenBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_regex_quantifier_range() {
        // Regex quantifier {1,3} should be part of the argument
        let tokens = tokenize(r"location ~ ^/[0-9]{1,3}$ {");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("location".to_string()),
                TokenKind::Argument("~".to_string()),
                TokenKind::Argument("^/[0-9]{1,3}$".to_string()),
                TokenKind::OpenBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_regex_quantifier_open_ended() {
        // Regex quantifier {8,} should be part of the argument
        let tokens = tokenize(r"location ~ ^/[a-z]{8,}$ {");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("location".to_string()),
                TokenKind::Argument("~".to_string()),
                TokenKind::Argument("^/[a-z]{8,}$".to_string()),
                TokenKind::OpenBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_escaped_braces_in_regex() {
        // Escaped braces \{ and \} should be part of the argument
        let tokens = tokenize(r"location ~ ^/nested/\{[a-z]+\}$ {");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("location".to_string()),
                TokenKind::Argument("~".to_string()),
                TokenKind::Argument(r"^/nested/\{[a-z]+\}$".to_string()),
                TokenKind::OpenBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_multiple_escaped_braces() {
        // Multiple escaped braces in pattern
        let tokens = tokenize(r"location ~ ^/data/\{id\}/\{name\}$ {");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("location".to_string()),
                TokenKind::Argument("~".to_string()),
                TokenKind::Argument(r"^/data/\{id\}/\{name\}$".to_string()),
                TokenKind::OpenBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_hash_in_argument() {
        // # inside an argument should not be treated as comment
        let tokens = tokenize("location ~* foo#bar {");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("location".to_string()),
                TokenKind::Argument("~*".to_string()),
                TokenKind::Ident("foo#bar".to_string()),
                TokenKind::OpenBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_hash_in_regex_pattern() {
        // Emacs auto-save pattern: #.*#
        let tokens = tokenize(r"location ~* (?:#.*#|\.bak)$ {");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("location".to_string()),
                TokenKind::Argument("~*".to_string()),
                TokenKind::Argument(r"(?:#.*#|\.bak)$".to_string()),
                TokenKind::OpenBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_hash_comment_after_whitespace() {
        // # after whitespace is still a comment
        let tokens = tokenize("listen 80; # this is a comment");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("listen".to_string()),
                TokenKind::Argument("80".to_string()),
                TokenKind::Semicolon,
                TokenKind::Comment("# this is a comment".to_string()),
                TokenKind::Eof,
            ]
        );
    }
}
