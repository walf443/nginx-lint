//! Custom nginx configuration file parser
//!
//! This module provides a parser for nginx configuration files that accepts
//! any directive name, allowing extension modules like ngx_headers_more,
//! lua-nginx-module, etc. to be linted.

pub mod ast;
pub mod error;
pub mod lexer;

use ast::{Argument, ArgumentValue, Block, Comment, Config, ConfigItem, Directive, Span};
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
                    self.advance();
                    consecutive_newlines += 1;
                    // Only add blank line if we've seen content and have multiple newlines
                    if consecutive_newlines > 1 && !items.is_empty() {
                        items.push(ConfigItem::BlankLine(span));
                    }
                }
                TokenKind::Comment(text) => {
                    let comment = Comment {
                        text: text.clone(),
                        span: self.current().span,
                    };
                    self.advance();
                    items.push(ConfigItem::Comment(comment));
                    consecutive_newlines = 0;
                }
                TokenKind::CloseBrace if !in_block => {
                    let pos = self.current().span.start;
                    return Err(ParseError::UnmatchedCloseBrace { position: pos });
                }
                TokenKind::Ident(_) | TokenKind::Argument(_) => {
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

        // Get directive name
        let (name, name_span) = match &self.current().kind {
            TokenKind::Ident(name) => (name.clone(), self.current().span),
            TokenKind::Argument(name) => (name.clone(), self.current().span),
            _ => {
                return Err(ParseError::ExpectedDirectiveName {
                    position: self.current().span.start,
                })
            }
        };
        self.advance();

        // Parse arguments
        let mut args = Vec::new();
        let mut trailing_comment = None;

        loop {
            self.skip_newlines();

            match &self.current().kind {
                TokenKind::Semicolon => {
                    let end_pos = self.current().span.end;
                    self.advance();

                    // Check for trailing comment on same line
                    if let TokenKind::Comment(text) = &self.current().kind {
                        trailing_comment = Some(Comment {
                            text: text.clone(),
                            span: self.current().span,
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
                    });
                }
                TokenKind::OpenBrace => {
                    let block_start = self.current().span.start;
                    self.advance();
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
                        }),
                        span: Span::new(start_pos, block_end),
                        trailing_comment,
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
}
