//! nginx configuration file parser
//!
//! This crate provides a parser for nginx configuration files, producing an AST
//! suitable for lint rules and autofix. It accepts **any directive name**, so
//! extension modules (ngx_headers_more, lua-nginx-module, etc.) are supported
//! without special configuration.
//!
//! # Quick Start
//!
//! ```
//! use nginx_lint_parser::parse_string;
//!
//! let config = parse_string("http { server { listen 80; } }").unwrap();
//!
//! for directive in config.all_directives() {
//!     println!("{} at line {}", directive.name, directive.span.start.line);
//! }
//! ```
//!
//! To parse from a file on disk:
//!
//! ```no_run
//! use std::path::Path;
//! use nginx_lint_parser::parse_config;
//!
//! let config = parse_config(Path::new("/etc/nginx/nginx.conf")).unwrap();
//! ```
//!
//! # Modules
//!
//! - [`ast`] — AST types: [`ast::Config`], [`ast::Directive`], [`ast::Block`],
//!   [`ast::Argument`], [`ast::Span`], [`ast::Position`]
//! - [`error`] — Error types: [`error::ParseError`], [`error::LexerError`]
//! - [`lexer`] — Tokenizer: [`lexer::Lexer`], [`lexer::Token`], [`lexer::TokenKind`]
//!
//! # Common Patterns
//!
//! ## Iterating over directives
//!
//! [`Config::directives()`](ast::Config::directives) yields only top-level directives.
//! [`Config::all_directives()`](ast::Config::all_directives) recurses into blocks:
//!
//! ```
//! # use nginx_lint_parser::parse_string;
//! let config = parse_string("http { gzip on; server { listen 80; } }").unwrap();
//!
//! // Top-level only → ["http"]
//! let top: Vec<_> = config.directives().map(|d| &d.name).collect();
//! assert_eq!(top, vec!["http"]);
//!
//! // Recursive → ["http", "gzip", "server", "listen"]
//! let all: Vec<_> = config.all_directives().map(|d| &d.name).collect();
//! assert_eq!(all, vec!["http", "gzip", "server", "listen"]);
//! ```
//!
//! ## Checking arguments
//!
//! ```
//! # use nginx_lint_parser::parse_string;
//! let config = parse_string("server_tokens off;").unwrap();
//! let dir = config.directives().next().unwrap();
//!
//! assert!(dir.is("server_tokens"));
//! assert_eq!(dir.first_arg(), Some("off"));
//! assert!(dir.args[0].is_off());
//! assert!(dir.args[0].is_literal());
//! ```
//!
//! ## Inspecting blocks
//!
//! ```
//! # use nginx_lint_parser::parse_string;
//! let config = parse_string("upstream backend { server 127.0.0.1:8080; }").unwrap();
//! let upstream = config.directives().next().unwrap();
//!
//! if let Some(block) = &upstream.block {
//!     for inner in block.directives() {
//!         println!("{}: {}", inner.name, inner.first_arg().unwrap_or(""));
//!     }
//! }
//! ```

pub mod ast;
pub mod error;
pub mod lexer;

#[cfg(feature = "wasm")]
mod wasm;

use ast::{
    Argument, ArgumentValue, BlankLine, Block, Comment, Config, ConfigItem, Directive, Position,
    Span,
};
use error::{ParseError, ParseResult};
use lexer::{Lexer, Token, TokenKind};
use std::fs;
use std::path::Path;

/// Parse a nginx configuration file from disk
pub fn parse_config(path: &Path) -> ParseResult<Config> {
    let content = fs::read_to_string(path).map_err(|e| ParseError::IoError(e.to_string()))?;
    parse_string(&content)
}

/// Parse nginx configuration from a string
pub fn parse_string(source: &str) -> ParseResult<Config> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse()
}

/// Parser for nginx configuration
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn advance(&mut self) -> &Token {
        let token = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    fn skip_newlines(&mut self) {
        while matches!(self.current().kind, TokenKind::Newline) {
            self.advance();
        }
    }

    fn parse(&mut self) -> ParseResult<Config> {
        let items = self.parse_items(false)?;
        Ok(Config {
            items,
            include_context: Vec::new(),
        })
    }

    fn parse_items(&mut self, in_block: bool) -> ParseResult<Vec<ConfigItem>> {
        let mut items = Vec::new();
        let mut consecutive_newlines = 0;

        loop {
            // Check for end of block or file
            if in_block && matches!(self.current().kind, TokenKind::CloseBrace) {
                break;
            }
            if matches!(self.current().kind, TokenKind::Eof) {
                break;
            }

            match &self.current().kind {
                TokenKind::Newline => {
                    let span = self.current().span;
                    let content = self.current().leading_whitespace.clone();
                    self.advance();
                    consecutive_newlines += 1;
                    // Only add blank line if we've seen content and have multiple newlines
                    if consecutive_newlines > 1 && !items.is_empty() {
                        items.push(ConfigItem::BlankLine(BlankLine { span, content }));
                    }
                }
                TokenKind::Comment(text) => {
                    let mut comment = Comment {
                        text: text.clone(),
                        span: self.current().span,
                        leading_whitespace: self.current().leading_whitespace.clone(),
                        trailing_whitespace: String::new(),
                    };
                    self.advance();
                    // Capture trailing whitespace from next newline token
                    if let TokenKind::Newline = &self.current().kind {
                        comment.trailing_whitespace = self.current().leading_whitespace.clone();
                    }
                    items.push(ConfigItem::Comment(comment));
                    consecutive_newlines = 0;
                }
                TokenKind::CloseBrace if !in_block => {
                    let pos = self.current().span.start;
                    return Err(ParseError::UnmatchedCloseBrace { position: pos });
                }
                TokenKind::Ident(_)
                | TokenKind::Argument(_)
                | TokenKind::SingleQuotedString(_)
                | TokenKind::DoubleQuotedString(_) => {
                    let directive = self.parse_directive()?;
                    items.push(ConfigItem::Directive(Box::new(directive)));
                    consecutive_newlines = 0;
                }
                _ => {
                    let token = self.current();
                    return Err(ParseError::UnexpectedToken {
                        expected: "directive or comment".to_string(),
                        found: token.kind.display_name().to_string(),
                        position: token.span.start,
                    });
                }
            }
        }

        Ok(items)
    }

    fn parse_directive(&mut self) -> ParseResult<Directive> {
        let start_pos = self.current().span.start;
        let leading_whitespace = self.current().leading_whitespace.clone();

        // Get directive name (can be identifier, argument, or quoted string for map blocks)
        let (name, name_span, name_raw) = match &self.current().kind {
            TokenKind::Ident(name) => (
                name.clone(),
                self.current().span,
                self.current().raw.clone(),
            ),
            TokenKind::Argument(name) => (
                name.clone(),
                self.current().span,
                self.current().raw.clone(),
            ),
            TokenKind::SingleQuotedString(name) => (
                name.clone(),
                self.current().span,
                self.current().raw.clone(),
            ),
            TokenKind::DoubleQuotedString(name) => (
                name.clone(),
                self.current().span,
                self.current().raw.clone(),
            ),
            _ => {
                return Err(ParseError::ExpectedDirectiveName {
                    position: self.current().span.start,
                });
            }
        };
        let _ = name_raw; // Used for potential future raw reconstruction
        self.advance();

        // Parse arguments
        let mut args = Vec::new();
        let mut trailing_comment = None;

        loop {
            self.skip_newlines();

            match &self.current().kind {
                TokenKind::Semicolon => {
                    let space_before_terminator = self.current().leading_whitespace.clone();
                    let end_pos = self.current().span.end;
                    self.advance();

                    // Capture trailing whitespace (whitespace after ; until newline or comment)
                    let trailing_whitespace;

                    // Check for trailing comment on same line
                    if let TokenKind::Comment(text) = &self.current().kind {
                        // Trailing whitespace before comment is empty (comment's leading_whitespace handles spacing)
                        trailing_whitespace = String::new();
                        trailing_comment = Some(Comment {
                            text: text.clone(),
                            span: self.current().span,
                            leading_whitespace: self.current().leading_whitespace.clone(),
                            trailing_whitespace: String::new(), // Will be captured on newline
                        });
                        self.advance();
                        // Capture comment's trailing whitespace from next newline token
                        if let TokenKind::Newline = &self.current().kind
                            && let Some(ref mut tc) = trailing_comment
                        {
                            tc.trailing_whitespace = self.current().leading_whitespace.clone();
                        }
                    } else if let TokenKind::Newline = &self.current().kind {
                        trailing_whitespace = self.current().leading_whitespace.clone();
                    } else {
                        trailing_whitespace = String::new();
                    }

                    return Ok(Directive {
                        name,
                        name_span,
                        args,
                        block: None,
                        span: Span::new(start_pos, end_pos),
                        trailing_comment,
                        leading_whitespace,
                        space_before_terminator,
                        trailing_whitespace,
                    });
                }
                TokenKind::OpenBrace => {
                    let space_before_terminator = self.current().leading_whitespace.clone();
                    let block_start = self.current().span.start;
                    self.advance();

                    // Capture trailing whitespace after opening brace
                    let opening_brace_trailing = if let TokenKind::Newline = &self.current().kind {
                        self.current().leading_whitespace.clone()
                    } else {
                        String::new()
                    };

                    // Check if this is a raw block directive (like *_lua_block)
                    if is_raw_block_directive(&name) {
                        let (raw_content, block_end) = self.read_raw_block(block_start)?;

                        // Check for trailing comment
                        let mut block_trailing_whitespace = String::new();
                        if let TokenKind::Comment(text) = &self.current().kind {
                            trailing_comment = Some(Comment {
                                text: text.clone(),
                                span: self.current().span,
                                leading_whitespace: self.current().leading_whitespace.clone(),
                                trailing_whitespace: String::new(),
                            });
                            self.advance();
                        } else if let TokenKind::Newline = &self.current().kind {
                            block_trailing_whitespace = self.current().leading_whitespace.clone();
                        }

                        return Ok(Directive {
                            name,
                            name_span,
                            args,
                            block: Some(Block {
                                items: Vec::new(),
                                span: Span::new(block_start, block_end),
                                raw_content: Some(raw_content),
                                closing_brace_leading_whitespace: String::new(),
                                trailing_whitespace: block_trailing_whitespace,
                            }),
                            span: Span::new(start_pos, block_end),
                            trailing_comment,
                            leading_whitespace,
                            space_before_terminator,
                            trailing_whitespace: opening_brace_trailing,
                        });
                    }

                    self.skip_newlines();
                    let block_items = self.parse_items(true)?;

                    // Expect closing brace
                    if !matches!(self.current().kind, TokenKind::CloseBrace) {
                        return Err(ParseError::UnclosedBlock {
                            position: block_start,
                        });
                    }
                    let closing_brace_leading_whitespace =
                        self.current().leading_whitespace.clone();
                    let block_end = self.current().span.end;
                    self.advance();

                    // Capture trailing whitespace after closing brace
                    let mut block_trailing_whitespace = String::new();

                    // Check for trailing comment
                    if let TokenKind::Comment(text) = &self.current().kind {
                        trailing_comment = Some(Comment {
                            text: text.clone(),
                            span: self.current().span,
                            leading_whitespace: self.current().leading_whitespace.clone(),
                            trailing_whitespace: String::new(),
                        });
                        self.advance();
                        // Capture comment's trailing whitespace
                        if let TokenKind::Newline = &self.current().kind
                            && let Some(ref mut tc) = trailing_comment
                        {
                            tc.trailing_whitespace = self.current().leading_whitespace.clone();
                        }
                    } else if let TokenKind::Newline = &self.current().kind {
                        block_trailing_whitespace = self.current().leading_whitespace.clone();
                    }

                    return Ok(Directive {
                        name,
                        name_span,
                        args,
                        block: Some(Block {
                            items: block_items,
                            span: Span::new(block_start, block_end),
                            raw_content: None,
                            closing_brace_leading_whitespace,
                            trailing_whitespace: block_trailing_whitespace,
                        }),
                        span: Span::new(start_pos, block_end),
                        trailing_comment,
                        leading_whitespace,
                        space_before_terminator,
                        trailing_whitespace: opening_brace_trailing,
                    });
                }
                TokenKind::Ident(value) => {
                    args.push(Argument {
                        value: ArgumentValue::Literal(value.clone()),
                        span: self.current().span,
                        raw: self.current().raw.clone(),
                    });
                    self.advance();
                }
                TokenKind::Argument(value) => {
                    args.push(Argument {
                        value: ArgumentValue::Literal(value.clone()),
                        span: self.current().span,
                        raw: self.current().raw.clone(),
                    });
                    self.advance();
                }
                TokenKind::DoubleQuotedString(value) => {
                    args.push(Argument {
                        value: ArgumentValue::QuotedString(value.clone()),
                        span: self.current().span,
                        raw: self.current().raw.clone(),
                    });
                    self.advance();
                }
                TokenKind::SingleQuotedString(value) => {
                    args.push(Argument {
                        value: ArgumentValue::SingleQuotedString(value.clone()),
                        span: self.current().span,
                        raw: self.current().raw.clone(),
                    });
                    self.advance();
                }
                TokenKind::Variable(value) => {
                    args.push(Argument {
                        value: ArgumentValue::Variable(value.clone()),
                        span: self.current().span,
                        raw: self.current().raw.clone(),
                    });
                    self.advance();
                }
                TokenKind::Comment(text) => {
                    // Inline comment - this ends the directive arguments
                    // The directive still needs a semicolon or block
                    trailing_comment = Some(Comment {
                        text: text.clone(),
                        span: self.current().span,
                        leading_whitespace: self.current().leading_whitespace.clone(),
                        trailing_whitespace: String::new(),
                    });
                    self.advance();
                    // Capture trailing whitespace
                    if let TokenKind::Newline = &self.current().kind
                        && let Some(ref mut tc) = trailing_comment
                    {
                        tc.trailing_whitespace = self.current().leading_whitespace.clone();
                    }
                    // Skip to next line
                    self.skip_newlines();
                }
                TokenKind::Eof => {
                    return Err(ParseError::UnexpectedEof {
                        position: self.current().span.start,
                    });
                }
                TokenKind::CloseBrace => {
                    // Missing semicolon before close brace
                    return Err(ParseError::MissingSemicolon {
                        position: self.current().span.start,
                    });
                }
                _ => {
                    let token = self.current();
                    return Err(ParseError::UnexpectedToken {
                        expected: "argument, ';', or '{'".to_string(),
                        found: token.kind.display_name().to_string(),
                        position: token.span.start,
                    });
                }
            }
        }
    }

    /// Read a raw block content (for lua_block directives)
    /// Returns the raw content and the end position
    fn read_raw_block(&mut self, block_start: Position) -> ParseResult<(String, Position)> {
        let mut content = String::new();
        let mut brace_depth = 1;

        loop {
            match &self.current().kind {
                TokenKind::OpenBrace => {
                    content.push('{');
                    brace_depth += 1;
                    self.advance();
                }
                TokenKind::CloseBrace => {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        let end_pos = self.current().span.end;
                        self.advance();
                        // Trim leading/trailing whitespace from content
                        let trimmed = content.trim().to_string();
                        return Ok((trimmed, end_pos));
                    }
                    content.push('}');
                    self.advance();
                }
                TokenKind::Eof => {
                    return Err(ParseError::UnclosedBlock {
                        position: block_start,
                    });
                }
                _ => {
                    // Append raw token text
                    content.push_str(&self.current().raw);
                    // Add space between tokens (but not for newlines)
                    if !matches!(self.current().kind, TokenKind::Newline) {
                        // Check if next token needs spacing
                        self.advance();
                        if !matches!(
                            self.current().kind,
                            TokenKind::Newline
                                | TokenKind::Eof
                                | TokenKind::CloseBrace
                                | TokenKind::Semicolon
                        ) {
                            content.push(' ');
                        }
                    } else {
                        content.push('\n');
                        self.advance();
                    }
                }
            }
        }
    }
}

/// Check if a directive name indicates a raw block (Lua code, etc.)
///
/// Raw block directives contain code (like Lua) that should not be parsed
/// as nginx configuration. The content inside the block is preserved as-is.
///
/// # Examples
/// ```
/// use nginx_lint_parser::is_raw_block_directive;
///
/// assert!(is_raw_block_directive("content_by_lua_block"));
/// assert!(is_raw_block_directive("init_by_lua_block"));
/// assert!(!is_raw_block_directive("server"));
/// ```
pub fn is_raw_block_directive(name: &str) -> bool {
    // OpenResty / lua-nginx-module directives
    // Using ends_with covers all *_by_lua_block patterns
    name.ends_with("_by_lua_block")
}

/// Known nginx block directive names that require `{` instead of `;`
const BLOCK_DIRECTIVES: &[&str] = &[
    // Core
    "http",
    "server",
    "location",
    "upstream",
    "events",
    "stream",
    "mail",
    "types",
    // Conditionals and control
    "if",
    "limit_except",
    "geo",
    "map",
    "split_clients",
    "match",
];

/// Check if a directive is a known block directive that requires `{` instead of `;`
///
/// # Examples
/// ```
/// use nginx_lint_parser::is_block_directive;
///
/// assert!(is_block_directive("server"));
/// assert!(is_block_directive("location"));
/// assert!(!is_block_directive("listen"));
/// ```
pub fn is_block_directive(name: &str) -> bool {
    BLOCK_DIRECTIVES.contains(&name) || is_raw_block_directive(name)
}

/// Check if a directive is a block directive, including custom additions
///
/// This function checks the built-in list plus any additional block directives
/// specified in the configuration.
///
/// # Examples
/// ```
/// use nginx_lint_parser::is_block_directive_with_extras;
///
/// assert!(is_block_directive_with_extras("server", &[]));
/// assert!(is_block_directive_with_extras("my_custom_block", &["my_custom_block".to_string()]));
/// assert!(!is_block_directive_with_extras("listen", &[]));
/// ```
pub fn is_block_directive_with_extras(name: &str, additional: &[String]) -> bool {
    is_block_directive(name) || additional.iter().any(|s| s == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_directive() {
        let config = parse_string("worker_processes auto;").unwrap();
        let directives: Vec<_> = config.directives().collect();
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].name, "worker_processes");
        assert_eq!(directives[0].first_arg(), Some("auto"));
    }

    #[test]
    fn test_block_directive() {
        let config = parse_string("http {\n    server {\n        listen 80;\n    }\n}").unwrap();
        let directives: Vec<_> = config.directives().collect();
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].name, "http");
        assert!(directives[0].block.is_some());

        let all_directives: Vec<_> = config.all_directives().collect();
        assert_eq!(all_directives.len(), 3);
        assert_eq!(all_directives[0].name, "http");
        assert_eq!(all_directives[1].name, "server");
        assert_eq!(all_directives[2].name, "listen");
    }

    #[test]
    fn test_extension_directive() {
        let config = parse_string(r#"more_set_headers "Server: Custom";"#).unwrap();
        let directives: Vec<_> = config.directives().collect();
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].name, "more_set_headers");
        assert_eq!(directives[0].first_arg(), Some("Server: Custom"));
    }

    #[test]
    fn test_ssl_protocols() {
        let config = parse_string("ssl_protocols TLSv1.2 TLSv1.3;").unwrap();
        let directives: Vec<_> = config.directives().collect();
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].name, "ssl_protocols");
        assert_eq!(directives[0].args.len(), 2);
        assert_eq!(directives[0].args[0].as_str(), "TLSv1.2");
        assert_eq!(directives[0].args[1].as_str(), "TLSv1.3");
    }

    #[test]
    fn test_autoindex() {
        let config = parse_string("autoindex on;").unwrap();
        let directives: Vec<_> = config.directives().collect();
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].name, "autoindex");
        assert!(directives[0].args[0].is_on());
    }

    #[test]
    fn test_comment() {
        let config = parse_string("# This is a comment\nworker_processes auto;").unwrap();
        assert_eq!(config.items.len(), 2);
        match &config.items[0] {
            ConfigItem::Comment(c) => assert_eq!(c.text, "# This is a comment"),
            _ => panic!("Expected comment"),
        }
    }

    #[test]
    fn test_full_config() {
        let source = r#"
# Good nginx configuration
worker_processes auto;
error_log /var/log/nginx/error.log;

http {
    server_tokens off;
    gzip on;

    server {
        listen 80;
        server_name example.com;

        location / {
            root /var/www/html;
            index index.html;
        }
    }
}
"#;
        let config = parse_string(source).unwrap();

        let all_directives: Vec<_> = config.all_directives().collect();
        let names: Vec<&str> = all_directives.iter().map(|d| d.name.as_str()).collect();

        assert!(names.contains(&"worker_processes"));
        assert!(names.contains(&"error_log"));
        assert!(names.contains(&"server_tokens"));
        assert!(names.contains(&"gzip"));
        assert!(names.contains(&"listen"));
        assert!(names.contains(&"server_name"));
        assert!(names.contains(&"root"));
        assert!(names.contains(&"index"));
    }

    #[test]
    fn test_server_tokens_on() {
        let config = parse_string("server_tokens on;").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "server_tokens");
        assert!(directive.first_arg_is("on"));
        assert!(directive.args[0].is_on());
    }

    #[test]
    fn test_gzip_on() {
        let config = parse_string("gzip on;").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "gzip");
        assert!(directive.first_arg_is("on"));
    }

    #[test]
    fn test_position_tracking() {
        let config = parse_string("http {\n    listen 80;\n}").unwrap();
        let all_directives: Vec<_> = config.all_directives().collect();

        // "http" at line 1
        assert_eq!(all_directives[0].span.start.line, 1);

        // "listen" at line 2
        assert_eq!(all_directives[1].span.start.line, 2);
    }

    #[test]
    fn test_error_unmatched_brace() {
        let result = parse_string("http {\n    listen 80;\n");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::UnclosedBlock { .. } => {}
            e => panic!("Expected UnclosedBlock error, got {:?}", e),
        }
    }

    #[test]
    fn test_error_missing_semicolon() {
        let result = parse_string("listen 80\n}");
        assert!(result.is_err());
    }

    #[test]
    fn test_roundtrip() {
        let source = "worker_processes auto;\nhttp {\n    listen 80;\n}\n";
        let config = parse_string(source).unwrap();
        let output = config.to_source();

        // Parse the output again to verify it's valid
        let reparsed = parse_string(&output).unwrap();
        let names1: Vec<&str> = config.all_directives().map(|d| d.name.as_str()).collect();
        let names2: Vec<&str> = reparsed.all_directives().map(|d| d.name.as_str()).collect();
        assert_eq!(names1, names2);
    }

    #[test]
    fn test_lua_directive() {
        let config = parse_string("lua_code_cache on;").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "lua_code_cache");
        assert!(directive.first_arg_is("on"));
    }

    #[test]
    fn test_gzip_types() {
        let config = parse_string("gzip_types text/plain text/css application/json;").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "gzip_types");
        assert_eq!(directive.args.len(), 3);
    }

    #[test]
    fn test_lua_block_directive() {
        let config = parse_string(
            r#"content_by_lua_block {
    local cjson = require "cjson"
    ngx.say(cjson.encode({status = "ok"}))
}"#,
        )
        .unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "content_by_lua_block");
        assert!(directive.block.is_some());

        let block = directive.block.as_ref().unwrap();
        assert!(block.is_raw());
        assert!(block.raw_content.is_some());

        let content = block.raw_content.as_ref().unwrap();
        assert!(content.contains("local cjson = require"));
        assert!(content.contains("ngx.say"));
    }

    #[test]
    fn test_map_with_empty_string_key() {
        let config = parse_string(
            r#"map $http_upgrade $connection_upgrade {
    default upgrade;
    '' close;
}"#,
        )
        .unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "map");
        assert!(directive.block.is_some());

        let block = directive.block.as_ref().unwrap();
        let directives: Vec<_> = block.directives().collect();
        assert_eq!(directives.len(), 2);
        assert_eq!(directives[0].name, "default");
        assert_eq!(directives[1].name, ""); // empty string key
    }

    #[test]
    fn test_init_by_lua_block() {
        let config = parse_string(
            r#"init_by_lua_block {
    require "resty.core"
    cjson = require "cjson"
}"#,
        )
        .unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "init_by_lua_block");
        assert!(directive.block.is_some());

        let block = directive.block.as_ref().unwrap();
        assert!(block.is_raw());

        let content = block.raw_content.as_ref().unwrap();
        assert!(content.contains("require \"resty.core\""));
    }

    #[test]
    fn test_whitespace_capture() {
        let config = parse_string("http {\n    listen 80;\n}").unwrap();
        let all_directives: Vec<_> = config.all_directives().collect();

        // "http" has no leading whitespace
        assert_eq!(all_directives[0].leading_whitespace, "");
        // "http" has space before the opening brace
        assert_eq!(all_directives[0].space_before_terminator, " ");

        // "listen" has 4 spaces of leading whitespace
        assert_eq!(all_directives[1].leading_whitespace, "    ");
        // "listen" has no space before the semicolon
        assert_eq!(all_directives[1].space_before_terminator, "");
    }

    #[test]
    fn test_comment_whitespace_capture() {
        let config = parse_string("    # test comment\nlisten 80;").unwrap();

        // Find the comment
        if let ConfigItem::Comment(comment) = &config.items[0] {
            assert_eq!(comment.leading_whitespace, "    ");
        } else {
            panic!("Expected comment");
        }
    }

    #[test]
    fn test_roundtrip_preserves_whitespace() {
        // Test that round-trip preserves original indentation
        let source = "http {\n    server {\n        listen 80;\n    }\n}\n";
        let config = parse_string(source).unwrap();
        let output = config.to_source();

        // Parse the output and check the indentation is preserved
        let reparsed = parse_string(&output).unwrap();
        let all_directives: Vec<_> = reparsed.all_directives().collect();

        // "http" has no leading whitespace
        assert_eq!(all_directives[0].leading_whitespace, "");
        // "server" has 4 spaces
        assert_eq!(all_directives[1].leading_whitespace, "    ");
        // "listen" has 8 spaces
        assert_eq!(all_directives[2].leading_whitespace, "        ");
    }

    // ===== Variable tests =====

    #[test]
    fn test_variable_in_argument() {
        let config = parse_string("set $var value;").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "set");
        // Variable values are stored without the $ prefix
        assert_eq!(directive.args[0].as_str(), "var");
        assert!(directive.args[0].is_variable());
        // But raw contains the original text
        assert_eq!(directive.args[0].raw, "$var");
    }

    #[test]
    fn test_variable_in_proxy_pass() {
        // URLs with variables are split into multiple tokens
        let config = parse_string("proxy_pass http://$backend;").unwrap();
        let directive = config.directives().next().unwrap();
        // First part is the literal "http://"
        assert_eq!(directive.args[0].as_str(), "http://");
        assert!(directive.args[0].is_literal());
        // Second part is the variable
        assert_eq!(directive.args[1].as_str(), "backend");
        assert!(directive.args[1].is_variable());
    }

    #[test]
    fn test_braced_variable() {
        let config = parse_string(r#"add_header X-Request-Id "${request_id}";"#).unwrap();
        let directive = config.directives().next().unwrap();
        // Quoted strings containing variables are treated as quoted strings
        assert!(directive.args[1].is_quoted());
        assert!(directive.args[1].as_str().contains("request_id"));
    }

    // ===== Location directive tests =====

    #[test]
    fn test_location_exact_match() {
        let config = parse_string("location = /exact { return 200; }").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "location");
        assert_eq!(directive.args[0].as_str(), "=");
        assert_eq!(directive.args[1].as_str(), "/exact");
    }

    #[test]
    fn test_location_prefix_match() {
        let config = parse_string("location ^~ /prefix { return 200; }").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.args[0].as_str(), "^~");
        assert_eq!(directive.args[1].as_str(), "/prefix");
    }

    #[test]
    fn test_location_regex_case_sensitive() {
        let config = parse_string(r#"location ~ \.php$ { return 200; }"#).unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.args[0].as_str(), "~");
        assert_eq!(directive.args[1].as_str(), r"\.php$");
    }

    #[test]
    fn test_location_regex_case_insensitive() {
        let config = parse_string(r#"location ~* \.(gif|jpg|png)$ { return 200; }"#).unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.args[0].as_str(), "~*");
        assert_eq!(directive.args[1].as_str(), r"\.(gif|jpg|png)$");
    }

    #[test]
    fn test_named_location() {
        let config = parse_string("location @backend { proxy_pass http://backend; }").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.args[0].as_str(), "@backend");
    }

    // ===== If directive tests =====

    #[test]
    fn test_if_variable_check() {
        let config = parse_string("if ($request_uri ~* /admin) { return 403; }").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "if");
        assert!(directive.block.is_some());
    }

    #[test]
    fn test_if_file_exists() {
        let config = parse_string("if (-f $request_filename) { break; }").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "if");
        assert_eq!(directive.args[0].as_str(), "(-f");
    }

    // ===== Upstream tests =====

    #[test]
    fn test_upstream_basic() {
        let config = parse_string(
            r#"upstream backend {
    server 127.0.0.1:8080;
    server 127.0.0.1:8081;
}"#,
        )
        .unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "upstream");
        assert_eq!(directive.args[0].as_str(), "backend");

        let servers: Vec<_> = directive.block.as_ref().unwrap().directives().collect();
        assert_eq!(servers.len(), 2);
    }

    #[test]
    fn test_upstream_with_options() {
        let config = parse_string(
            r#"upstream backend {
    server 127.0.0.1:8080 weight=5 max_fails=3 fail_timeout=30s;
    keepalive 32;
}"#,
        )
        .unwrap();
        let directive = config.directives().next().unwrap();
        let block = directive.block.as_ref().unwrap();
        let items: Vec<_> = block.directives().collect();

        assert_eq!(items[0].name, "server");
        assert!(items[0].args.iter().any(|a| a.as_str().contains("weight")));
        assert_eq!(items[1].name, "keepalive");
    }

    // ===== Geo and Map tests =====

    #[test]
    fn test_geo_directive() {
        let config = parse_string(
            r#"geo $geo {
    default unknown;
    127.0.0.1 local;
    10.0.0.0/8 internal;
}"#,
        )
        .unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "geo");
        assert!(directive.block.is_some());
    }

    #[test]
    fn test_map_directive() {
        let config = parse_string(
            r#"map $uri $new_uri {
    default $uri;
    /old /new;
    ~^/api/v1/(.*) /api/v2/$1;
}"#,
        )
        .unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "map");
        assert_eq!(directive.args.len(), 2);
    }

    // ===== Quoting tests =====

    #[test]
    fn test_single_quoted_string() {
        let config = parse_string(r#"set $var 'single quoted';"#).unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.args[1].as_str(), "single quoted");
        assert!(directive.args[1].is_quoted());
    }

    #[test]
    fn test_double_quoted_string() {
        let config = parse_string(r#"set $var "double quoted";"#).unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.args[1].as_str(), "double quoted");
        assert!(directive.args[1].is_quoted());
    }

    #[test]
    fn test_quoted_string_with_spaces() {
        let config = parse_string(r#"add_header X-Custom "value with spaces";"#).unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.args[1].as_str(), "value with spaces");
    }

    #[test]
    fn test_escaped_quote_in_string() {
        let config = parse_string(r#"set $var "say \"hello\"";"#).unwrap();
        let directive = config.directives().next().unwrap();
        // The parser preserves escaped quotes in the string content
        let value = directive.args[1].as_str();
        assert!(value.contains("hello"), "value was: {}", value);
    }

    // ===== Include directive tests =====

    #[test]
    fn test_include_directive() {
        let config = parse_string("include /etc/nginx/conf.d/*.conf;").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "include");
        assert_eq!(directive.args[0].as_str(), "/etc/nginx/conf.d/*.conf");
    }

    #[test]
    fn test_include_with_glob() {
        let config = parse_string("include sites-enabled/*;").unwrap();
        let directive = config.directives().next().unwrap();
        assert!(directive.args[0].as_str().contains("*"));
    }

    // ===== Error handling tests =====

    #[test]
    fn test_error_unexpected_closing_brace() {
        let result = parse_string("listen 80; }");
        assert!(result.is_err());
    }

    #[test]
    fn test_error_unclosed_string() {
        let result = parse_string(r#"set $var "unclosed;"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_empty_directive_name() {
        // This should work - empty string as a key in map
        let result = parse_string("map $a $b { '' x; }");
        assert!(result.is_ok());
    }

    // ===== Special nginx patterns =====

    #[test]
    fn test_try_files_directive() {
        let config = parse_string("try_files $uri $uri/ /index.php?$args;").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "try_files");
        // Variables are tokenized separately, so we have more args
        // $uri, $uri/, /index.php?, $args
        assert!(directive.args.len() >= 3);
        assert!(directive.args.iter().any(|a| a.as_str() == "uri"));
    }

    #[test]
    fn test_rewrite_directive() {
        let config = parse_string("rewrite ^/old/(.*)$ /new/$1 permanent;").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "rewrite");
        // /new/$1 is split into /new/ and $1
        assert!(directive.args.len() >= 3);
        assert_eq!(directive.args[0].as_str(), "^/old/(.*)$");
        assert!(directive.args.iter().any(|a| a.as_str() == "permanent"));
    }

    #[test]
    fn test_return_directive() {
        let config = parse_string("return 301 https://$host$request_uri;").unwrap();
        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "return");
        assert_eq!(directive.args[0].as_str(), "301");
    }

    #[test]
    fn test_limit_except_block() {
        let config = parse_string(
            r#"location / {
    limit_except GET POST {
        deny all;
    }
}"#,
        )
        .unwrap();
        let all: Vec<_> = config.all_directives().collect();
        assert!(all.iter().any(|d| d.name == "limit_except"));
    }

    // ===== Complex configuration tests =====

    #[test]
    fn test_ssl_configuration() {
        let config = parse_string(
            r#"server {
    listen 443 ssl http2;
    ssl_certificate /etc/ssl/cert.pem;
    ssl_certificate_key /etc/ssl/key.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256;
    ssl_prefer_server_ciphers on;
}"#,
        )
        .unwrap();

        let all: Vec<_> = config.all_directives().collect();
        assert!(all.iter().any(|d| d.name == "ssl_certificate"));
        assert!(all.iter().any(|d| d.name == "ssl_protocols"));
    }

    #[test]
    fn test_proxy_configuration() {
        let config = parse_string(
            r#"location /api {
    proxy_pass http://backend;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_connect_timeout 60s;
    proxy_read_timeout 60s;
}"#,
        )
        .unwrap();

        let all: Vec<_> = config.all_directives().collect();
        let proxy_headers: Vec<_> = all
            .iter()
            .filter(|d| d.name == "proxy_set_header")
            .collect();
        assert_eq!(proxy_headers.len(), 3);
    }

    #[test]
    fn test_deeply_nested_blocks() {
        let config = parse_string(
            r#"http {
    server {
        location / {
            if ($request_method = POST) {
                return 405;
            }
        }
    }
}"#,
        )
        .unwrap();

        let all: Vec<_> = config.all_directives().collect();
        assert_eq!(all.len(), 5); // http, server, location, if, return
    }

    // ===== Argument helper method tests =====

    #[test]
    fn test_argument_is_on_off() {
        let config = parse_string("gzip on; gzip_static off;").unwrap();
        let directives: Vec<_> = config.directives().collect();

        assert!(directives[0].args[0].is_on());
        assert!(!directives[0].args[0].is_off());

        assert!(directives[1].args[0].is_off());
        assert!(!directives[1].args[0].is_on());
    }

    #[test]
    fn test_argument_is_literal() {
        let config = parse_string(r#"set $var "quoted"; set $var2 literal;"#).unwrap();
        let directives: Vec<_> = config.directives().collect();

        assert!(!directives[0].args[1].is_literal());
        assert!(directives[1].args[1].is_literal());
    }

    // ===== Blank line handling tests =====

    #[test]
    fn test_blank_lines_preserved() {
        let config =
            parse_string("worker_processes 1;\n\nerror_log /var/log/error.log;\n").unwrap();

        // Should have 3 items: directive, blank line, directive
        assert_eq!(config.items.len(), 3);
        assert!(matches!(config.items[1], ConfigItem::BlankLine(_)));
    }

    #[test]
    fn test_multiple_blank_lines() {
        let config = parse_string("a 1;\n\n\nb 2;\n").unwrap();

        let blank_count = config
            .items
            .iter()
            .filter(|i| matches!(i, ConfigItem::BlankLine(_)))
            .count();
        assert_eq!(blank_count, 2);
    }

    // ===== Events block tests =====

    #[test]
    fn test_events_block() {
        let config = parse_string(
            r#"events {
    worker_connections 1024;
    use epoll;
    multi_accept on;
}"#,
        )
        .unwrap();

        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "events");

        let inner: Vec<_> = directive.block.as_ref().unwrap().directives().collect();
        assert_eq!(inner.len(), 3);
    }

    // ===== Stream block tests =====

    #[test]
    fn test_stream_block() {
        let config = parse_string(
            r#"stream {
    server {
        listen 12345;
        proxy_pass backend;
    }
}"#,
        )
        .unwrap();

        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "stream");
    }

    // ===== Types block tests =====

    #[test]
    fn test_types_block() {
        let config = parse_string(
            r#"types {
    text/html html htm;
    text/css css;
    application/javascript js;
}"#,
        )
        .unwrap();

        let directive = config.directives().next().unwrap();
        assert_eq!(directive.name, "types");

        let inner: Vec<_> = directive.block.as_ref().unwrap().directives().collect();
        assert_eq!(inner.len(), 3);
        assert_eq!(inner[0].name, "text/html");
    }

    #[test]
    fn test_utf8_comment_column_tracking() {
        // Columns should be character-based, not byte-based
        // "# 開発環境" has 6 characters but 14 bytes
        let config = parse_string("# 開発環境\nlisten 80;").unwrap();
        // Check comment span
        if let ast::ConfigItem::Comment(c) = &config.items[0] {
            assert_eq!(c.span.start.line, 1);
            assert_eq!(c.span.start.column, 1);
            // End column should be 1 + 6 chars = 7 (character-based)
            // not 1 + 14 bytes = 15
            assert_eq!(c.span.end.column, 7);
        } else {
            panic!("expected Comment");
        }
        // "listen" on line 2 should still be at column 1
        let directives: Vec<_> = config.all_directives().collect();
        assert_eq!(directives[0].span.start.line, 2);
        assert_eq!(directives[0].span.start.column, 1);
    }

    #[test]
    fn test_utf8_comment_byte_offset_tracking() {
        // Byte offsets should be byte-based (not character-based)
        let config = parse_string("# 開発環境\nlisten 80;").unwrap();
        if let ast::ConfigItem::Comment(c) = &config.items[0] {
            // "# 開発環境" = 14 bytes, offset starts at 0
            assert_eq!(c.span.start.offset, 0);
            assert_eq!(c.span.end.offset, 14);
        } else {
            panic!("expected Comment");
        }
        // "listen" starts after "# 開発環境\n" = 15 bytes
        let directives: Vec<_> = config.all_directives().collect();
        assert_eq!(directives[0].span.start.offset, 15);
    }
}
