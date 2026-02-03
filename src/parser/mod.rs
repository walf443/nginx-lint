//! Custom nginx configuration file parser
//!
//! This module provides a parser for nginx configuration files that accepts
//! any directive name, allowing extension modules like ngx_headers_more,
//! lua-nginx-module, etc. to be linted.

pub mod ast;
pub mod error;
pub mod lexer;

use ast::{Argument, ArgumentValue, BlankLine, Block, Comment, Config, ConfigItem, Directive, Position, Span};
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
        Ok(Config { items })
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
                    let comment = Comment {
                        text: text.clone(),
                        span: self.current().span,
                        leading_whitespace: self.current().leading_whitespace.clone(),
                    };
                    self.advance();
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
            TokenKind::Ident(name) => (name.clone(), self.current().span, self.current().raw.clone()),
            TokenKind::Argument(name) => (name.clone(), self.current().span, self.current().raw.clone()),
            TokenKind::SingleQuotedString(name) => (name.clone(), self.current().span, self.current().raw.clone()),
            TokenKind::DoubleQuotedString(name) => (name.clone(), self.current().span, self.current().raw.clone()),
            _ => {
                return Err(ParseError::ExpectedDirectiveName {
                    position: self.current().span.start,
                })
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

                    // Check for trailing comment on same line
                    if let TokenKind::Comment(text) = &self.current().kind {
                        trailing_comment = Some(Comment {
                            text: text.clone(),
                            span: self.current().span,
                            leading_whitespace: self.current().leading_whitespace.clone(),
                        });
                        self.advance();
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
                    });
                }
                TokenKind::OpenBrace => {
                    let space_before_terminator = self.current().leading_whitespace.clone();
                    let block_start = self.current().span.start;
                    self.advance();

                    // Check if this is a raw block directive (like *_lua_block)
                    if is_raw_block_directive(&name) {
                        let (raw_content, block_end) = self.read_raw_block(block_start)?;

                        // Check for trailing comment
                        if let TokenKind::Comment(text) = &self.current().kind {
                            trailing_comment = Some(Comment {
                                text: text.clone(),
                                span: self.current().span,
                                leading_whitespace: self.current().leading_whitespace.clone(),
                            });
                            self.advance();
                        }

                        return Ok(Directive {
                            name,
                            name_span,
                            args,
                            block: Some(Block {
                                items: Vec::new(),
                                span: Span::new(block_start, block_end),
                                raw_content: Some(raw_content),
                            }),
                            span: Span::new(start_pos, block_end),
                            trailing_comment,
                            leading_whitespace,
                            space_before_terminator,
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
                    let block_end = self.current().span.end;
                    self.advance();

                    // Check for trailing comment
                    if let TokenKind::Comment(text) = &self.current().kind {
                        trailing_comment = Some(Comment {
                            text: text.clone(),
                            span: self.current().span,
                            leading_whitespace: self.current().leading_whitespace.clone(),
                        });
                        self.advance();
                    }

                    return Ok(Directive {
                        name,
                        name_span,
                        args,
                        block: Some(Block {
                            items: block_items,
                            span: Span::new(block_start, block_end),
                            raw_content: None,
                        }),
                        span: Span::new(start_pos, block_end),
                        trailing_comment,
                        leading_whitespace,
                        space_before_terminator,
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
                    });
                    self.advance();
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
/// use nginx_lint::parser::is_raw_block_directive;
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
/// use nginx_lint::parser::is_block_directive;
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
/// use nginx_lint::parser::is_block_directive_with_extras;
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
        let config =
            parse_string("gzip_types text/plain text/css application/json;").unwrap();
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
}
