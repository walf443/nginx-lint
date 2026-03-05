//! Convert a rowan lossless CST into the existing AST types.
//!
//! The entry point is [`convert`], which takes the root `SyntaxNode` produced by
//! [`crate::parser::parse`] and the original source text, and returns a [`Config`].

use crate::ast::{
    Argument, ArgumentValue, BlankLine, Block, Comment, Config, ConfigItem, Directive, Span,
};
use crate::is_raw_block_directive;
use crate::line_index::LineIndex;
use crate::syntax_kind::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

/// Convert a rowan CST root node into the existing AST [`Config`].
pub fn convert(root: &SyntaxNode, source: &str) -> Config {
    let line_index = LineIndex::new(source);
    let ctx = ConvertCtx {
        line_index: &line_index,
    };
    let items = ctx.convert_items(root);
    Config {
        items,
        include_context: Vec::new(),
    }
}

/// Shared context for the conversion.
struct ConvertCtx<'a> {
    line_index: &'a LineIndex,
}

impl<'a> ConvertCtx<'a> {
    // ── helpers ───────────────────────────────────────────────────────

    fn span_of(&self, node: &SyntaxNode) -> Span {
        self.line_index.span(node.text_range())
    }

    fn span_of_token(&self, token: &SyntaxToken) -> Span {
        self.line_index.span(token.text_range())
    }

    // ── items (root / block body) ────────────────────────────────────

    /// Convert the children of a ROOT or BLOCK node into `Vec<ConfigItem>`.
    ///
    /// Handles the structural mapping where:
    /// - Leading whitespace before a DIRECTIVE belongs to that directive
    /// - Comments at the top level become `ConfigItem::Comment`
    /// - BLANK_LINE nodes become `ConfigItem::BlankLine`
    fn convert_items(&self, parent: &SyntaxNode) -> Vec<ConfigItem> {
        let mut items: Vec<ConfigItem> = Vec::new();
        let children: Vec<SyntaxElement> = parent.children_with_tokens().collect();
        let len = children.len();
        let mut i = 0;

        // Track consecutive newlines for blank-line detection (matching the
        // original parser's behaviour where the first newline after content is
        // *not* a blank line).
        let mut consecutive_newlines: usize = 0;

        while i < len {
            let child = &children[i];
            match child.kind() {
                SyntaxKind::DIRECTIVE => {
                    // Collect leading whitespace from preceding sibling tokens.
                    let leading_ws = self.collect_leading_whitespace(&children, i);
                    let node = child.as_node().unwrap();
                    let directive = self.convert_directive(node, &leading_ws, &children, i);
                    items.push(ConfigItem::Directive(Box::new(directive)));
                    consecutive_newlines = 0;
                    i += 1;
                }
                SyntaxKind::COMMENT => {
                    let token = child.as_token().unwrap();
                    let leading_ws = self.collect_leading_whitespace(&children, i);
                    let comment = Comment {
                        text: token.text().to_string(),
                        span: self.span_of_token(token),
                        leading_whitespace: leading_ws,
                        // Comments consume everything up to (but not including)
                        // '\n', so there is no trailing whitespace between
                        // COMMENT and NEWLINE in the rowan tree.
                        trailing_whitespace: String::new(),
                    };
                    items.push(ConfigItem::Comment(comment));
                    consecutive_newlines = 0;
                    i += 1;
                }
                SyntaxKind::BLANK_LINE => {
                    let node = child.as_node().unwrap();
                    // Only emit a blank line if there's already content (matches
                    // the original parser which requires consecutive_newlines > 1
                    // *and* prior items).
                    consecutive_newlines += 1;
                    if !items.is_empty() {
                        let text = node.text().to_string();
                        // The content is whitespace-only part (strip trailing newline)
                        let content = text.strip_suffix('\n').unwrap_or(&text).to_string();
                        let span = self.span_of(node);
                        items.push(ConfigItem::BlankLine(BlankLine { span, content }));
                    }
                    i += 1;
                }
                SyntaxKind::NEWLINE => {
                    consecutive_newlines += 1;
                    // A bare NEWLINE between items: check if consecutive newlines
                    // form a blank line (mimicking the original parser logic).
                    if consecutive_newlines > 1 && !items.is_empty() {
                        let span = if let Some(tok) = child.as_token() {
                            self.span_of_token(tok)
                        } else {
                            Span::default()
                        };
                        items.push(ConfigItem::BlankLine(BlankLine {
                            span,
                            content: String::new(),
                        }));
                    }
                    i += 1;
                }
                // WHITESPACE, L_BRACE, R_BRACE, ERROR tokens at this level are
                // skipped (whitespace is collected as leading_whitespace of the
                // next directive/comment).
                _ => {
                    // Non-newline tokens don't count as consecutive newlines
                    if child.kind() != SyntaxKind::WHITESPACE {
                        consecutive_newlines = 0;
                    }
                    i += 1;
                }
            }
        }

        items
    }

    /// Walk backwards from index `i` to collect leading whitespace text.
    ///
    /// The original AST parser stores whitespace that precedes a directive on
    /// the same line (indentation). In the rowan tree this is a sibling
    /// WHITESPACE token immediately before the DIRECTIVE node.
    fn collect_leading_whitespace(&self, children: &[SyntaxElement], i: usize) -> String {
        if i == 0 {
            return String::new();
        }
        // The token immediately before should be WHITESPACE (indentation).
        // But we also need to verify it's on the same line (preceded by NEWLINE
        // or is the first token).
        let prev = &children[i - 1];
        if prev.kind() == SyntaxKind::WHITESPACE
            && let Some(tok) = prev.as_token()
        {
            // Verify this is indentation (preceded by NEWLINE or start)
            if i < 2 {
                return tok.text().to_string();
            }
            let before = &children[i - 2];
            if before.kind() == SyntaxKind::NEWLINE {
                return tok.text().to_string();
            }
        }
        String::new()
    }

    // ── directive ────────────────────────────────────────────────────

    fn convert_directive(
        &self,
        node: &SyntaxNode,
        leading_ws: &str,
        parent_children: &[SyntaxElement],
        parent_idx: usize,
    ) -> Directive {
        let children: Vec<SyntaxElement> = node.children_with_tokens().collect();

        // 1. Find directive name (first non-trivia token)
        let (name, name_span, name_idx) = self.find_directive_name(&children);

        // 2. Collect arguments (tokens after name, before terminator/block)
        let args = self.collect_arguments(&children, name_idx);

        // 3. Find terminator and block
        let mut block: Option<Block> = None;
        let mut trailing_comment: Option<Comment> = None;
        let mut space_before_terminator = String::new();
        let mut trailing_whitespace = String::new();
        // The original parser's directive span ends at the terminator (semicolon
        // or closing brace), NOT including the trailing comment.
        let mut dir_span_end = name_span.end;

        // Find the semicolon, block, or determine what the terminator is
        let terminator_info = self.find_terminator(&children);

        match terminator_info {
            Terminator::Semicolon { idx } => {
                space_before_terminator = self.whitespace_before(&children, idx);

                // Directive span ends at the end of the semicolon
                if let Some(tok) = children[idx].as_token() {
                    dir_span_end = self.span_of_token(tok).end;
                }

                // Check for trailing comment inside the DIRECTIVE node (after semicolon)
                trailing_comment = self.find_trailing_comment(&children, idx);

                // trailing_whitespace: whitespace after semicolon (and after comment if present)
                // on the same line, from AFTER the directive node
                trailing_whitespace =
                    self.collect_directive_trailing_whitespace(parent_children, parent_idx);
            }
            Terminator::Block { idx } => {
                let block_node = children[idx].as_node().unwrap();
                let is_raw = is_raw_block_directive(&name);

                // space_before_terminator is whitespace before the BLOCK node
                space_before_terminator = self.whitespace_before(&children, idx);

                // trailing_whitespace for a block directive is whitespace after
                // the opening brace (on the same line) — this corresponds to
                // opening_brace_trailing in the original parser.
                trailing_whitespace = self.opening_brace_trailing(block_node);

                block = Some(self.convert_block(block_node, is_raw));

                // Directive span ends at the end of the block (closing brace)
                dir_span_end = self.span_of(block_node).end;

                // Check for trailing comment after the block's closing brace
                trailing_comment =
                    self.find_trailing_comment_after_block(parent_children, parent_idx);

                // If there's a trailing comment, block.trailing_whitespace is empty
                // and directive's trailing_whitespace stays as opening_brace_trailing.
                // If no trailing comment, capture block trailing whitespace from parent.
                if trailing_comment.is_none()
                    && let Some(ref mut b) = block
                {
                    b.trailing_whitespace =
                        self.collect_directive_trailing_whitespace(parent_children, parent_idx);
                }
            }
            Terminator::Missing => {
                // Error recovery: no terminator found
            }
        }

        let dir_span = Span::new(name_span.start, dir_span_end);

        Directive {
            name,
            name_span,
            args,
            block,
            span: dir_span,
            trailing_comment,
            leading_whitespace: leading_ws.to_string(),
            space_before_terminator,
            trailing_whitespace,
        }
    }

    /// Find the directive name: first non-trivia token.
    fn find_directive_name(&self, children: &[SyntaxElement]) -> (String, Span, usize) {
        for (idx, child) in children.iter().enumerate() {
            match child.kind() {
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => continue,
                _ => {
                    if let Some(token) = child.as_token() {
                        let raw = token.text().to_string();
                        let name = match child.kind() {
                            SyntaxKind::DOUBLE_QUOTED_STRING | SyntaxKind::SINGLE_QUOTED_STRING => {
                                // Strip quotes for name
                                strip_quotes(&raw)
                            }
                            _ => raw.clone(),
                        };
                        let span = self.span_of_token(token);
                        return (name, span, idx);
                    }
                }
            }
        }
        // Should not happen for valid DIRECTIVE nodes
        (String::new(), Span::default(), 0)
    }

    /// Collect arguments from tokens after the directive name.
    fn collect_arguments(&self, children: &[SyntaxElement], name_idx: usize) -> Vec<Argument> {
        let mut args = Vec::new();

        for child in children.iter().skip(name_idx + 1) {
            match child.kind() {
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => continue,
                SyntaxKind::SEMICOLON | SyntaxKind::COMMENT => break,
                SyntaxKind::BLOCK => break,
                kind if is_argument_token(kind) => {
                    if let Some(token) = child.as_token() {
                        args.push(self.token_to_argument(token));
                    }
                }
                _ => continue,
            }
        }

        args
    }

    /// Convert a token to an Argument.
    fn token_to_argument(&self, token: &SyntaxToken) -> Argument {
        let raw = token.text().to_string();
        let span = self.span_of_token(token);
        let value = match token.kind() {
            SyntaxKind::DOUBLE_QUOTED_STRING => ArgumentValue::QuotedString(strip_quotes(&raw)),
            SyntaxKind::SINGLE_QUOTED_STRING => {
                ArgumentValue::SingleQuotedString(strip_quotes(&raw))
            }
            SyntaxKind::VARIABLE => {
                let var_name = if raw.starts_with("${") && raw.ends_with('}') {
                    raw[2..raw.len() - 1].to_string()
                } else if let Some(stripped) = raw.strip_prefix('$') {
                    stripped.to_string()
                } else {
                    raw.clone()
                };
                ArgumentValue::Variable(var_name)
            }
            // IDENT and ARGUMENT both become Literal
            _ => ArgumentValue::Literal(raw.clone()),
        };
        Argument { value, span, raw }
    }

    /// Find the terminator of a directive (SEMICOLON or BLOCK node).
    fn find_terminator(&self, children: &[SyntaxElement]) -> Terminator {
        for (idx, child) in children.iter().enumerate() {
            match child.kind() {
                SyntaxKind::SEMICOLON => return Terminator::Semicolon { idx },
                SyntaxKind::BLOCK => return Terminator::Block { idx },
                _ => continue,
            }
        }
        Terminator::Missing
    }

    /// Get whitespace text immediately before index `idx`.
    fn whitespace_before(&self, children: &[SyntaxElement], idx: usize) -> String {
        if idx == 0 {
            return String::new();
        }
        let prev = &children[idx - 1];
        if prev.kind() == SyntaxKind::WHITESPACE
            && let Some(tok) = prev.as_token()
        {
            return tok.text().to_string();
        }
        String::new()
    }

    /// Find trailing comment after a semicolon in a DIRECTIVE node.
    fn find_trailing_comment(
        &self,
        children: &[SyntaxElement],
        semi_idx: usize,
    ) -> Option<Comment> {
        // After semicolon: optional WHITESPACE then COMMENT
        let mut idx = semi_idx + 1;
        let mut comment_leading_ws = String::new();

        while idx < children.len() {
            match children[idx].kind() {
                SyntaxKind::WHITESPACE => {
                    if let Some(tok) = children[idx].as_token() {
                        comment_leading_ws = tok.text().to_string();
                    }
                    idx += 1;
                }
                SyntaxKind::COMMENT => {
                    let token = children[idx].as_token().unwrap();
                    return Some(Comment {
                        text: token.text().to_string(),
                        span: self.span_of_token(token),
                        leading_whitespace: comment_leading_ws,
                        trailing_whitespace: String::new(),
                    });
                }
                _ => break,
            }
        }
        None
    }

    /// Find trailing comment after a block directive's closing brace.
    /// The comment would be in the parent's children, after the DIRECTIVE node.
    fn find_trailing_comment_after_block(
        &self,
        parent_children: &[SyntaxElement],
        dir_idx: usize,
    ) -> Option<Comment> {
        // After the DIRECTIVE node in parent, look for WHITESPACE + COMMENT
        // before a NEWLINE.
        let mut idx = dir_idx + 1;
        let mut comment_leading_ws = String::new();

        while idx < parent_children.len() {
            match parent_children[idx].kind() {
                SyntaxKind::WHITESPACE => {
                    if let Some(tok) = parent_children[idx].as_token() {
                        comment_leading_ws = tok.text().to_string();
                    }
                    idx += 1;
                }
                SyntaxKind::COMMENT => {
                    let token = parent_children[idx].as_token().unwrap();
                    return Some(Comment {
                        text: token.text().to_string(),
                        span: self.span_of_token(token),
                        leading_whitespace: comment_leading_ws,
                        trailing_whitespace: String::new(),
                    });
                }
                SyntaxKind::NEWLINE => break,
                _ => break,
            }
        }
        None
    }

    /// Collect trailing whitespace after a directive from the parent's children.
    ///
    /// In the original parser, for semicolon-terminated directives, this is the
    /// `leading_whitespace` of the Newline token that follows the semicolon.
    /// In rowan, this is the WHITESPACE token (if any) between the DIRECTIVE
    /// node (or its trailing comment) and the next NEWLINE in the parent.
    fn collect_directive_trailing_whitespace(
        &self,
        parent_children: &[SyntaxElement],
        dir_idx: usize,
    ) -> String {
        let idx = dir_idx + 1;
        if idx < parent_children.len() && parent_children[idx].kind() == SyntaxKind::WHITESPACE {
            // Check if this is followed by NEWLINE (making it trailing ws)
            // or by COMMENT (in which case directive trailing is empty)
            if idx + 1 < parent_children.len() {
                let next_kind = parent_children[idx + 1].kind();
                if (next_kind == SyntaxKind::NEWLINE
                    || next_kind == SyntaxKind::DIRECTIVE
                    || next_kind == SyntaxKind::BLANK_LINE)
                    && let Some(tok) = parent_children[idx].as_token()
                {
                    return tok.text().to_string();
                }
                // If followed by COMMENT, don't report as trailing_whitespace
            } else {
                // Whitespace at the end of siblings
                if let Some(tok) = parent_children[idx].as_token() {
                    return tok.text().to_string();
                }
            }
        }
        String::new()
    }

    /// Get trailing whitespace after the opening brace of a block.
    ///
    /// In the original parser this is `opening_brace_trailing` — the whitespace
    /// between `{` and the newline on the same line.
    fn opening_brace_trailing(&self, block_node: &SyntaxNode) -> String {
        // Inside the BLOCK node, after L_BRACE, look for WHITESPACE before NEWLINE.
        let mut found_lbrace = false;
        for child in block_node.children_with_tokens() {
            if child.kind() == SyntaxKind::L_BRACE {
                found_lbrace = true;
                continue;
            }
            if found_lbrace {
                if child.kind() == SyntaxKind::WHITESPACE
                    && let Some(tok) = child.as_token()
                {
                    return tok.text().to_string();
                }
                return String::new();
            }
        }
        String::new()
    }

    // ── block ────────────────────────────────────────────────────────

    fn convert_block(&self, block_node: &SyntaxNode, is_raw: bool) -> Block {
        let span = self.span_of(block_node);

        if is_raw {
            let raw_content = self.extract_raw_content(block_node);
            return Block {
                items: Vec::new(),
                span,
                raw_content: Some(raw_content),
                closing_brace_leading_whitespace: String::new(),
                trailing_whitespace: String::new(),
            };
        }

        let items = self.convert_items(block_node);
        let closing_ws = self.closing_brace_leading_whitespace(block_node);

        Block {
            items,
            span,
            raw_content: None,
            closing_brace_leading_whitespace: closing_ws,
            trailing_whitespace: String::new(), // Set by caller
        }
    }

    /// Extract raw content from a raw block, matching the original parser's
    /// token-by-token reconstruction.
    ///
    /// The original parser's `read_raw_block` joins token `raw` values with
    /// spaces (dropping indentation whitespace) and converts newlines to `\n`.
    /// We replicate this behaviour by walking the rowan tokens.
    fn extract_raw_content(&self, block_node: &SyntaxNode) -> String {
        let children: Vec<SyntaxElement> = block_node.children_with_tokens().collect();
        let mut content = String::new();
        let mut depth: u32 = 0;
        let mut i = 0;

        while i < children.len() {
            let kind = children[i].kind();
            match kind {
                SyntaxKind::L_BRACE => {
                    if depth > 0 {
                        content.push('{');
                    }
                    depth += 1;
                    i += 1;
                }
                SyntaxKind::R_BRACE => {
                    depth = depth.saturating_sub(1);
                    if depth > 0 {
                        content.push('}');
                    }
                    i += 1;
                }
                SyntaxKind::NEWLINE => {
                    // The original parser's read_raw_block pushes both the
                    // raw text ("\n") and an explicit '\n' for each Newline
                    // token, resulting in \n\n per newline. Replicate this.
                    content.push('\n');
                    content.push('\n');
                    i += 1;
                }
                SyntaxKind::WHITESPACE => {
                    // Skip whitespace (indentation) — the original parser
                    // reconstructs with single spaces between tokens instead.
                    i += 1;
                }
                _ => {
                    // Append token text
                    if let Some(tok) = children[i].as_token() {
                        content.push_str(tok.text());
                    }
                    i += 1;
                    // Look ahead: if the next meaningful token is not a
                    // newline/eof/closebrace/semicolon, add a space.
                    // Skip whitespace to find the next meaningful token.
                    let mut next_i = i;
                    while next_i < children.len()
                        && children[next_i].kind() == SyntaxKind::WHITESPACE
                    {
                        next_i += 1;
                    }
                    if next_i < children.len() {
                        let next_kind = children[next_i].kind();
                        if !matches!(
                            next_kind,
                            SyntaxKind::NEWLINE | SyntaxKind::R_BRACE | SyntaxKind::SEMICOLON
                        ) {
                            content.push(' ');
                        }
                    }
                    // Skip the whitespace we peeked past
                    i = next_i;
                }
            }
        }

        content.trim().to_string()
    }

    /// Get the leading whitespace before the closing brace `}`.
    fn closing_brace_leading_whitespace(&self, block_node: &SyntaxNode) -> String {
        let children: Vec<SyntaxElement> = block_node.children_with_tokens().collect();
        // Find R_BRACE and look at the preceding token
        for (idx, child) in children.iter().enumerate().rev() {
            if child.kind() == SyntaxKind::R_BRACE {
                if idx > 0
                    && children[idx - 1].kind() == SyntaxKind::WHITESPACE
                    && let Some(tok) = children[idx - 1].as_token()
                {
                    return tok.text().to_string();
                }
                break;
            }
        }
        String::new()
    }
}

/// Terminator information for a directive.
enum Terminator {
    Semicolon { idx: usize },
    Block { idx: usize },
    Missing,
}

/// Strip surrounding quotes from a string, matching the original lexer's
/// escape-sequence processing.
fn strip_quotes(s: &str) -> String {
    if s.len() < 2 {
        return s.to_string();
    }
    if s.starts_with('"') && s.ends_with('"') {
        unescape_double_quoted(&s[1..s.len() - 1])
    } else if s.starts_with('\'') && s.ends_with('\'') {
        unescape_single_quoted(&s[1..s.len() - 1])
    } else {
        s.to_string()
    }
}

/// Process escape sequences in a double-quoted string (matching the original lexer).
fn unescape_double_quoted(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some('$') => result.push('$'),
                Some(c) => {
                    result.push('\\');
                    result.push(c);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Process escape sequences in a single-quoted string (matching the original lexer).
fn unescape_single_quoted(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('\\') => result.push('\\'),
                Some('\'') => result.push('\''),
                Some(c) => {
                    result.push('\\');
                    result.push(c);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Check if a SyntaxKind represents an argument token.
fn is_argument_token(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IDENT
            | SyntaxKind::ARGUMENT
            | SyntaxKind::VARIABLE
            | SyntaxKind::DOUBLE_QUOTED_STRING
            | SyntaxKind::SINGLE_QUOTED_STRING
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_string_rowan;

    fn parse_and_convert(source: &str) -> Config {
        let (root, errors) = parse_string_rowan(source);
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        convert(&root, source)
    }

    #[test]
    fn simple_directive() {
        let config = parse_and_convert("listen 80;");
        let dirs: Vec<_> = config.directives().collect();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "listen");
        assert_eq!(dirs[0].args.len(), 1);
        assert_eq!(dirs[0].args[0].raw, "80");
        assert!(matches!(dirs[0].args[0].value, ArgumentValue::Literal(ref s) if s == "80"));
    }

    #[test]
    fn block_directive() {
        let config = parse_and_convert("server {\n    listen 80;\n}");
        let dirs: Vec<_> = config.directives().collect();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "server");
        assert!(dirs[0].block.is_some());

        let block = dirs[0].block.as_ref().unwrap();
        let inner: Vec<_> = block.directives().collect();
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0].name, "listen");
        assert_eq!(inner[0].leading_whitespace, "    ");
    }

    #[test]
    fn variable_argument() {
        let config = parse_and_convert("set $var value;");
        let d = config.directives().next().unwrap();
        assert_eq!(d.args.len(), 2);
        assert_eq!(d.args[0].raw, "$var");
        assert!(matches!(d.args[0].value, ArgumentValue::Variable(ref s) if s == "var"));
        assert_eq!(d.args[1].raw, "value");
    }

    #[test]
    fn quoted_string_argument() {
        let config = parse_and_convert(r#"return 200 "hello world";"#);
        let d = config.directives().next().unwrap();
        assert_eq!(d.args.len(), 2);
        assert_eq!(d.args[1].raw, "\"hello world\"");
        assert!(
            matches!(d.args[1].value, ArgumentValue::QuotedString(ref s) if s == "hello world")
        );
    }

    #[test]
    fn trailing_comment() {
        let config = parse_and_convert("listen 80; # port\n");
        let d = config.directives().next().unwrap();
        assert!(d.trailing_comment.is_some());
        assert_eq!(d.trailing_comment.as_ref().unwrap().text, "# port");
    }

    #[test]
    fn standalone_comment() {
        let config = parse_and_convert("# comment\nlisten 80;");
        assert_eq!(config.items.len(), 2);
        assert!(matches!(config.items[0], ConfigItem::Comment(_)));
        assert!(matches!(config.items[1], ConfigItem::Directive(_)));
    }

    #[test]
    fn span_positions() {
        let config = parse_and_convert("listen 80;");
        let d = config.directives().next().unwrap();
        // name_span should cover "listen" (0..6)
        assert_eq!(d.name_span.start.line, 1);
        assert_eq!(d.name_span.start.column, 1);
        assert_eq!(d.name_span.start.offset, 0);
        assert_eq!(d.name_span.end.offset, 6);
    }
}
