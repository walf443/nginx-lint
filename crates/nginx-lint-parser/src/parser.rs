//! Rowan-based recursive-descent parser for nginx configuration files.
//!
//! Takes the token sequence from [`lexer_rowan::tokenize`](crate::lexer_rowan::tokenize)
//! and builds a lossless green tree using [`rowan::GreenNodeBuilder`].

use crate::syntax_kind::SyntaxKind;
use rowan::GreenNode;
use rowan::GreenNodeBuilder;

/// Parse errors collected during tree construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub message: String,
    pub offset: usize,
}

/// Parse a flat token list into a rowan green tree.
///
/// Returns the root green node and any errors encountered during parsing.
pub fn parse(tokens: Vec<(SyntaxKind, &str)>) -> (GreenNode, Vec<SyntaxError>) {
    let mut parser = Parser::new(tokens);
    parser.parse_root();
    (parser.builder.finish(), parser.errors)
}

// ── Parser ──────────────────────────────────────────────────────────────

struct Parser<'a> {
    tokens: Vec<(SyntaxKind, &'a str)>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    errors: Vec<SyntaxError>,
    /// Byte offset into the original source (sum of consumed token lengths).
    offset: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: Vec<(SyntaxKind, &'a str)>) -> Self {
        Self {
            tokens,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
            offset: 0,
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────

    /// Current token kind, or `None` at EOF.
    fn current(&self) -> Option<SyntaxKind> {
        self.tokens.get(self.pos).map(|(k, _)| *k)
    }

    /// Current token text.
    fn current_text(&self) -> &'a str {
        self.tokens.get(self.pos).map(|(_, t)| *t).unwrap_or("")
    }

    /// Check if the current token is `kind`.
    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == Some(kind)
    }

    /// Check if we're at end of file.
    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// Consume the current token and add it as a leaf to the builder.
    fn bump(&mut self) {
        if let Some(&(kind, text)) = self.tokens.get(self.pos) {
            self.builder.token(kind.into(), text);
            self.offset += text.len();
            self.pos += 1;
        }
    }

    /// Consume whitespace and newline tokens (trivia), adding them to the tree.
    fn eat_trivia(&mut self) {
        while let Some(kind) = self.current() {
            if kind == SyntaxKind::WHITESPACE || kind == SyntaxKind::NEWLINE {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// Peek at the next non-trivia token kind.
    fn peek_non_trivia(&self) -> Option<SyntaxKind> {
        let mut i = self.pos;
        while i < self.tokens.len() {
            let kind = self.tokens[i].0;
            if kind != SyntaxKind::WHITESPACE && kind != SyntaxKind::NEWLINE {
                return Some(kind);
            }
            i += 1;
        }
        None
    }

    fn error(&mut self, message: impl Into<String>) {
        self.errors.push(SyntaxError {
            message: message.into(),
            offset: self.offset,
        });
    }

    // ── Grammar rules ───────────────────────────────────────────────

    /// ROOT → item*
    fn parse_root(&mut self) {
        self.builder.start_node(SyntaxKind::ROOT.into());
        self.parse_items(false);
        self.builder.finish_node();
    }

    /// Parse items until EOF (if `in_block` is false) or R_BRACE (if `in_block` is true).
    fn parse_items(&mut self, in_block: bool) {
        loop {
            match self.current() {
                None => break,
                Some(SyntaxKind::R_BRACE) if in_block => break,
                Some(SyntaxKind::R_BRACE) => {
                    // Unexpected '}' at top level — wrap in ERROR node.
                    self.error("unexpected '}'");
                    self.builder.start_node(SyntaxKind::ERROR.into());
                    self.bump();
                    self.builder.finish_node();
                }
                Some(SyntaxKind::WHITESPACE) => {
                    // Check for blank line: WHITESPACE followed by NEWLINE,
                    // where previous token was NEWLINE (or start of input).
                    if self.is_blank_line_start() {
                        self.parse_blank_line();
                    } else {
                        self.bump(); // plain leading whitespace
                    }
                }
                Some(SyntaxKind::NEWLINE) => {
                    // Could be a blank line (consecutive newlines) or just a newline.
                    // If the next token is also NEWLINE or WHITESPACE+NEWLINE, it's blank.
                    self.bump();
                }
                Some(SyntaxKind::COMMENT) => {
                    self.bump();
                }
                Some(kind) if is_directive_start(kind) => {
                    self.parse_directive();
                }
                Some(SyntaxKind::ERROR) => {
                    self.error("unexpected token");
                    self.bump();
                }
                Some(_) => {
                    // Any other token at item level is an error.
                    self.error(format!("unexpected token: {:?}", self.current().unwrap()));
                    self.builder.start_node(SyntaxKind::ERROR.into());
                    self.bump();
                    self.builder.finish_node();
                }
            }
        }
    }

    /// Check if current WHITESPACE token starts a blank line.
    /// A blank line is WHITESPACE followed by NEWLINE where we're at the start
    /// of a line (previous token was NEWLINE or we're at position 0).
    fn is_blank_line_start(&self) -> bool {
        if !self.at(SyntaxKind::WHITESPACE) {
            return false;
        }
        // Check that next token is NEWLINE
        let next = self.tokens.get(self.pos + 1).map(|(k, _)| *k);
        if next != Some(SyntaxKind::NEWLINE) {
            return false;
        }
        // Check that we're at start of a line
        if self.pos == 0 {
            return true;
        }
        let prev = self.tokens[self.pos - 1].0;
        prev == SyntaxKind::NEWLINE
    }

    /// Parse a BLANK_LINE node: WHITESPACE NEWLINE
    fn parse_blank_line(&mut self) {
        self.builder.start_node(SyntaxKind::BLANK_LINE.into());
        self.bump(); // WHITESPACE
        self.bump(); // NEWLINE
        self.builder.finish_node();
    }

    /// DIRECTIVE → (IDENT | argument-token) argument* (SEMICOLON | block) COMMENT?
    ///
    /// Most directives start with IDENT, but inside `map`/`geo`/`split_clients`
    /// blocks, entries can start with ARGUMENT, quoted strings, or VARIABLE.
    fn parse_directive(&mut self) {
        self.builder.start_node(SyntaxKind::DIRECTIVE.into());

        // Directive name (or first token of a map/geo entry)
        let name = self.current_text().to_string();
        self.bump(); // IDENT or argument-like token

        // Arguments (consume whitespace + argument tokens)
        self.parse_arguments();

        // Check for lua block
        let is_lua_block = name.ends_with("_by_lua_block");

        // Terminator: semicolon or block
        match self.peek_non_trivia() {
            Some(SyntaxKind::SEMICOLON) => {
                self.eat_trivia();
                self.bump(); // SEMICOLON
                // Consume trailing whitespace + comment on same line
                self.eat_trailing_comment();
            }
            Some(SyntaxKind::L_BRACE) => {
                self.eat_trivia();
                if is_lua_block {
                    self.parse_raw_block();
                } else {
                    self.parse_block();
                }
            }
            _ => {
                // Missing terminator — error recovery
                self.error("expected ';' or '{'");
            }
        }

        self.builder.finish_node(); // DIRECTIVE
    }

    /// Parse directive arguments: sequences of ARGUMENT, IDENT, VARIABLE,
    /// DOUBLE_QUOTED_STRING, SINGLE_QUOTED_STRING separated by whitespace/newlines.
    ///
    /// nginx treats newlines as whitespace between tokens, so arguments can
    /// span multiple lines (e.g. `log_format ... '...'\n    "...";`).
    fn parse_arguments(&mut self) {
        loop {
            // Peek past whitespace and newlines to see if next meaningful token
            // is an argument.
            let mut lookahead = self.pos;
            while lookahead < self.tokens.len() {
                let kind = self.tokens[lookahead].0;
                if kind == SyntaxKind::WHITESPACE || kind == SyntaxKind::NEWLINE {
                    lookahead += 1;
                } else {
                    break;
                }
            }
            if lookahead >= self.tokens.len() {
                break;
            }
            let next_kind = self.tokens[lookahead].0;

            if is_argument_kind(next_kind) {
                // Consume trivia (whitespace + newlines) before the argument
                self.eat_trivia();
                self.bump(); // the argument token
            } else {
                break;
            }
        }
    }

    /// Eat optional trailing whitespace + comment on the same line as a semicolon.
    ///
    /// WHITESPACE is only consumed when followed by COMMENT — bare trailing
    /// whitespace belongs to the directive's `space_before_terminator` or
    /// `trailing_whitespace` and is handled by the AST conversion layer.
    fn eat_trailing_comment(&mut self) {
        if self.at(SyntaxKind::WHITESPACE) {
            let next = self.tokens.get(self.pos + 1).map(|(k, _)| *k);
            if next == Some(SyntaxKind::COMMENT) {
                self.bump(); // WHITESPACE
                self.bump(); // COMMENT
            }
        }
    }

    /// BLOCK → L_BRACE item* R_BRACE
    fn parse_block(&mut self) {
        self.builder.start_node(SyntaxKind::BLOCK.into());
        self.bump(); // L_BRACE

        self.parse_items(true);

        if self.at(SyntaxKind::R_BRACE) {
            self.bump(); // R_BRACE
        } else {
            self.error("expected '}'");
        }
        self.builder.finish_node();
    }

    /// Parse a raw block for `*_by_lua_block` directives.
    /// All tokens between L_BRACE and matching R_BRACE are consumed as-is,
    /// tracking brace depth.
    fn parse_raw_block(&mut self) {
        self.builder.start_node(SyntaxKind::BLOCK.into());
        self.bump(); // L_BRACE

        let mut depth: u32 = 1;
        while !self.at_end() && depth > 0 {
            match self.current() {
                Some(SyntaxKind::L_BRACE) => {
                    depth += 1;
                    self.bump();
                }
                Some(SyntaxKind::R_BRACE) => {
                    depth -= 1;
                    if depth == 0 {
                        self.bump(); // closing R_BRACE
                    } else {
                        self.bump(); // nested R_BRACE
                    }
                }
                Some(_) => {
                    self.bump();
                }
                None => break,
            }
        }

        if depth > 0 {
            self.error("expected '}' for lua block");
        }

        self.builder.finish_node();
    }
}

/// Returns `true` if `kind` can appear as a directive argument.
fn is_argument_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ARGUMENT
            | SyntaxKind::IDENT
            | SyntaxKind::VARIABLE
            | SyntaxKind::DOUBLE_QUOTED_STRING
            | SyntaxKind::SINGLE_QUOTED_STRING
    )
}

/// Returns `true` if `kind` can start a directive.
///
/// Besides IDENT (normal directives), map/geo/split_clients block entries
/// can start with ARGUMENT, quoted strings, or VARIABLE.
fn is_directive_start(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IDENT
            | SyntaxKind::ARGUMENT
            | SyntaxKind::VARIABLE
            | SyntaxKind::DOUBLE_QUOTED_STRING
            | SyntaxKind::SINGLE_QUOTED_STRING
    )
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer_rowan::tokenize;
    use crate::syntax_kind::SyntaxNode;

    fn parse_source(source: &str) -> (SyntaxNode, Vec<SyntaxError>) {
        let tokens = tokenize(source);
        let (green, errors) = parse(tokens);
        (SyntaxNode::new_root(green), errors)
    }

    /// The tree text must equal the original source (lossless).
    fn assert_lossless(source: &str) {
        let (root, _) = parse_source(source);
        assert_eq!(
            root.text().to_string(),
            source,
            "lossless round-trip failed"
        );
    }

    /// Assert no parse errors.
    fn assert_no_errors(source: &str) -> SyntaxNode {
        let (root, errors) = parse_source(source);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        root
    }

    /// Find the first DIRECTIVE child node under root.
    fn first_directive(root: &SyntaxNode) -> SyntaxNode {
        root.children()
            .find(|n| n.kind() == SyntaxKind::DIRECTIVE)
            .expect("no DIRECTIVE node found")
    }

    /// Collect child kinds of a node.
    fn child_kinds(node: &SyntaxNode) -> Vec<SyntaxKind> {
        node.children_with_tokens()
            .map(|child| child.kind())
            .collect()
    }

    // ── Basic directive tests ───────────────────────────────────────

    #[test]
    fn simple_directive() {
        let source = "listen 80;";
        let root = assert_no_errors(source);
        assert_lossless(source);

        let dir = first_directive(&root);
        let kinds = child_kinds(&dir);
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::IDENT,
                SyntaxKind::WHITESPACE,
                SyntaxKind::ARGUMENT,
                SyntaxKind::SEMICOLON
            ]
        );
    }

    #[test]
    fn directive_no_args() {
        let source = "accept_mutex on;";
        let root = assert_no_errors(source);
        assert_lossless(source);

        let dir = first_directive(&root);
        let kinds = child_kinds(&dir);
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::IDENT,
                SyntaxKind::WHITESPACE,
                SyntaxKind::IDENT,
                SyntaxKind::SEMICOLON
            ]
        );
    }

    // ── Block directive tests ───────────────────────────────────────

    #[test]
    fn block_directive() {
        let source = "server { listen 80; }";
        let root = assert_no_errors(source);
        assert_lossless(source);

        let dir = first_directive(&root);
        let kinds = child_kinds(&dir);
        // DIRECTIVE: IDENT WHITESPACE BLOCK
        assert!(kinds.contains(&SyntaxKind::IDENT));
        assert!(kinds.contains(&SyntaxKind::BLOCK));
    }

    #[test]
    fn nested_blocks() {
        let source = "http { server { listen 80; } }";
        assert_no_errors(source);
        assert_lossless(source);
    }

    // ── Multiline with indentation ──────────────────────────────────

    #[test]
    fn multiline_config() {
        let source = "http {\n    server {\n        listen 80;\n    }\n}";
        assert_no_errors(source);
        assert_lossless(source);
    }

    // ── Comments ────────────────────────────────────────────────────

    #[test]
    fn comment_standalone() {
        let source = "# this is a comment\nlisten 80;";
        assert_no_errors(source);
        assert_lossless(source);
    }

    #[test]
    fn comment_after_directive() {
        let source = "listen 80; # port";
        let root = assert_no_errors(source);
        assert_lossless(source);

        // The comment should be inside the DIRECTIVE node
        let dir = first_directive(&root);
        let kinds = child_kinds(&dir);
        assert!(kinds.contains(&SyntaxKind::COMMENT));
    }

    // ── Quoted strings and variables ────────────────────────────────

    #[test]
    fn double_quoted_string_arg() {
        let source = r#"return 200 "hello world";"#;
        let root = assert_no_errors(source);
        assert_lossless(source);

        let dir = first_directive(&root);
        let kinds = child_kinds(&dir);
        assert!(kinds.contains(&SyntaxKind::DOUBLE_QUOTED_STRING));
    }

    #[test]
    fn single_quoted_string_arg() {
        let source = "return 200 'hello world';";
        let root = assert_no_errors(source);
        assert_lossless(source);

        let dir = first_directive(&root);
        let kinds = child_kinds(&dir);
        assert!(kinds.contains(&SyntaxKind::SINGLE_QUOTED_STRING));
    }

    #[test]
    fn variable_arg() {
        let source = "set $var value;";
        let root = assert_no_errors(source);
        assert_lossless(source);

        let dir = first_directive(&root);
        let kinds = child_kinds(&dir);
        assert!(kinds.contains(&SyntaxKind::VARIABLE));
    }

    // ── Lua block ───────────────────────────────────────────────────

    #[test]
    fn lua_block() {
        let source = "content_by_lua_block {\n    ngx.say(\"hello\")\n}";
        let root = assert_no_errors(source);
        assert_lossless(source);

        let dir = first_directive(&root);
        let kinds = child_kinds(&dir);
        assert!(kinds.contains(&SyntaxKind::BLOCK));
    }

    #[test]
    fn lua_block_nested_braces() {
        let source =
            "content_by_lua_block {\n    if true then\n        local t = {1, 2}\n    end\n}";
        assert_no_errors(source);
        assert_lossless(source);
    }

    // ── Error recovery ──────────────────────────────────────────────

    #[test]
    fn missing_semicolon() {
        // nginx treats newlines as whitespace, so `listen 80\nserver_name ...`
        // is parsed as a single directive. A true missing-semicolon case
        // requires EOF without terminator.
        let source = "listen 80";
        let (_root, errors) = parse_source(source);
        assert_lossless(source);
        assert!(!errors.is_empty(), "should report missing semicolon");
    }

    #[test]
    fn missing_closing_brace() {
        let source = "server { listen 80;";
        let (_root, errors) = parse_source(source);
        assert_lossless(source);
        assert!(!errors.is_empty(), "should report missing '}}'");
    }

    #[test]
    fn unexpected_closing_brace() {
        let source = "} listen 80;";
        let (_root, errors) = parse_source(source);
        assert_lossless(source);
        assert!(!errors.is_empty(), "should report unexpected '}}'");
    }

    // ── Lossless round-trip tests ───────────────────────────────────

    #[test]
    fn lossless_empty() {
        assert_lossless("");
    }

    #[test]
    fn lossless_whitespace_only() {
        assert_lossless("  \n  \n");
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
        assert_no_errors(source);
    }

    #[test]
    fn lossless_blank_lines() {
        let source = "listen 80;\n\nlisten 443;\n";
        assert_lossless(source);
        assert_no_errors(source);
    }

    #[test]
    fn lossless_utf8() {
        let source = "# これは日本語コメント\nlisten 80;\n";
        assert_lossless(source);
        assert_no_errors(source);
    }

    #[test]
    fn location_with_regex() {
        let source = "location ~ ^/api/(.*) {\n    proxy_pass http://backend;\n}";
        assert_no_errors(source);
        assert_lossless(source);
    }

    #[test]
    fn multiple_directives() {
        let source = "worker_processes auto;\nevents {\n    worker_connections 1024;\n}\n";
        assert_no_errors(source);
        assert_lossless(source);
    }
}
