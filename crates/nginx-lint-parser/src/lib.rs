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
//! - [`error`] — Error types: [`error::ParseError`]
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
pub mod context;
pub mod error;
pub mod syntax_kind;

pub mod lexer_rowan;
pub mod line_index;
pub mod parser;
pub mod rowan_to_ast;

#[cfg(feature = "wasm")]
mod wasm;

use syntax_kind::SyntaxNode;

/// Parse a source string into a rowan lossless concrete syntax tree.
///
/// Returns the root `SyntaxNode` and any parse errors encountered.
///
/// ```
/// use nginx_lint_parser::parse_string_rowan;
///
/// let (root, errors) = parse_string_rowan("listen 80;");
/// assert!(errors.is_empty());
/// assert_eq!(root.text().to_string(), "listen 80;");
/// ```
pub fn parse_string_rowan(source: &str) -> (SyntaxNode, Vec<parser::SyntaxError>) {
    let tokens = lexer_rowan::tokenize(source);
    let (green, errors) = parser::parse(tokens);
    (SyntaxNode::new_root(green), errors)
}

/// Parse a source string into an AST [`Config`] using the rowan-based parser.
///
/// This is now equivalent to [`parse_string`]. Prefer using [`parse_string`] directly.
///
/// ```
/// use nginx_lint_parser::parse_string_via_rowan;
///
/// let config = parse_string_via_rowan("listen 80;").unwrap();
/// let d = config.directives().next().unwrap();
/// assert_eq!(d.name, "listen");
/// assert_eq!(d.first_arg(), Some("80"));
/// ```
#[deprecated(note = "Use parse_string() instead, which now uses rowan internally")]
pub fn parse_string_via_rowan(source: &str) -> ParseResult<Config> {
    parse_string(source)
}

use ast::Config;
use error::{ParseError, ParseResult};
use std::fs;
use std::path::Path;

/// Parse a nginx configuration file from disk
pub fn parse_config(path: &Path) -> ParseResult<Config> {
    let content = fs::read_to_string(path).map_err(|e| ParseError::IoError(e.to_string()))?;
    parse_string(&content)
}

/// Parse nginx configuration from a string
///
/// Uses the rowan-based lossless CST parser internally and converts to AST.
/// Returns an error if the source contains syntax errors.
pub fn parse_string(source: &str) -> ParseResult<Config> {
    let (root, errors) = parse_string_rowan(source);
    if let Some(err) = errors.first() {
        return Err(ParseError::UnexpectedToken {
            expected: "valid syntax".to_string(),
            found: err.message.clone(),
            position: line_index::LineIndex::new(source).position(err.offset),
        });
    }
    Ok(rowan_to_ast::convert(&root, source))
}

/// Parse nginx configuration from a string, returning AST even when syntax errors exist.
///
/// Unlike [`parse_string`], this function always produces a [`Config`] AST by
/// leveraging rowan's error-recovery. Syntax errors are returned alongside the
/// AST so callers can report them without aborting the lint pipeline.
pub fn parse_string_with_errors(source: &str) -> (Config, Vec<parser::SyntaxError>) {
    let (root, errors) = parse_string_rowan(source);
    let config = rowan_to_ast::convert(&root, source);
    (config, errors)
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
    use ast::ConfigItem;

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
            ParseError::UnclosedBlock { .. } | ParseError::UnexpectedToken { .. } => {}
            e => panic!(
                "Expected UnclosedBlock or UnexpectedToken error, got {:?}",
                e
            ),
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
        // The rowan parser uses byte-based columns
        // "# 開発環境" has 6 characters but 14 bytes (# + space + 4×3-byte kanji)
        let config = parse_string("# 開発環境\nlisten 80;").unwrap();
        // Check comment span
        if let ast::ConfigItem::Comment(c) = &config.items[0] {
            assert_eq!(c.span.start.line, 1);
            assert_eq!(c.span.start.column, 1);
            // End column is byte-based: 1 + 14 bytes = 15
            assert_eq!(c.span.end.column, 15);
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
