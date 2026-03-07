use crate::docs::RuleDoc;
use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use crate::parser::line_index::LineIndex;
use crate::parser::parse_string_rowan;
use crate::parser::syntax_kind::{SyntaxElement, SyntaxKind, SyntaxNode};
use std::fs;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "missing-semicolon",
    category: "syntax",
    description: "Detects missing semicolons at the end of directives",
    severity: "error",
    why: r#"In nginx configuration, each directive must end with a semicolon.
Without it, nginx cannot parse the configuration correctly.

Block directives (server, location, etc.) don't need semicolons,
but regular directives always require them."#,
    bad_example: include_str!("missing_semicolon/bad.conf"),
    good_example: include_str!("missing_semicolon/good.conf"),
    references: &[],
};

/// Check for missing semicolons at the end of directives
pub struct MissingSemicolon;

impl MissingSemicolon {
    /// Check content directly (used by WASM)
    pub fn check_content(&self, content: &str) -> Vec<LintError> {
        self.check_cst(content)
    }

    /// Check content with additional block directives
    ///
    /// The CST-based approach handles block structures via the parser,
    /// so `additional_block_directives` is accepted for API compatibility
    /// but not used.
    pub fn check_content_with_extras(
        &self,
        content: &str,
        _additional_block_directives: &[String],
    ) -> Vec<LintError> {
        self.check_cst(content)
    }

    /// CST-based missing semicolon detection.
    ///
    /// Walks the CST to find directives that lack a semicolon terminator.
    /// Detects two patterns:
    /// 1. Directives without SEMICOLON or BLOCK (e.g., at EOF or before `}`)
    /// 2. Merged directives where a NEWLINE separates what should be
    ///    independent directives (the parser treats newlines as whitespace)
    fn check_cst(&self, source: &str) -> Vec<LintError> {
        let (root, _) = parse_string_rowan(source);
        let line_index = LineIndex::new(source);
        let mut errors = Vec::new();

        Self::walk_node(&root, &line_index, &mut errors);

        errors
    }

    /// Recursively walk CST nodes, checking directives for missing semicolons.
    ///
    /// BLOCK nodes belonging to raw block directives (lua etc.) are skipped.
    fn walk_node(node: &SyntaxNode, line_index: &LineIndex, errors: &mut Vec<LintError>) {
        for child in node.children_with_tokens() {
            if let SyntaxElement::Node(child_node) = child {
                match child_node.kind() {
                    SyntaxKind::DIRECTIVE => {
                        Self::check_directive(&child_node, line_index, errors);
                    }
                    SyntaxKind::BLOCK => {
                        if !crate::parser::is_raw_block_cst_node(&child_node) {
                            Self::walk_node(&child_node, line_index, errors);
                        }
                    }
                    _ => {
                        Self::walk_node(&child_node, line_index, errors);
                    }
                }
            }
        }
    }

    /// Check a single DIRECTIVE node for missing semicolons.
    fn check_directive(
        directive: &SyntaxNode,
        line_index: &LineIndex,
        errors: &mut Vec<LintError>,
    ) {
        let has_semicolon = directive
            .children_with_tokens()
            .any(|c| c.kind() == SyntaxKind::SEMICOLON);
        let has_block = directive.children().any(|c| c.kind() == SyntaxKind::BLOCK);

        // Check for merged directives (NEWLINE within argument list)
        Self::check_merged_directives(directive, line_index, errors);

        // Recurse into non-raw blocks
        for child in directive.children() {
            if child.kind() == SyntaxKind::BLOCK && !crate::parser::is_raw_block_cst_node(&child) {
                Self::walk_node(&child, line_index, errors);
            }
        }

        // Directive with no terminator at all (EOF or before `}`)
        if !has_semicolon && !has_block {
            Self::report_at_end(directive, line_index, errors);
        }
    }

    /// Report a missing semicolon at the end of a directive (the last
    /// non-trivia token).
    fn report_at_end(directive: &SyntaxNode, line_index: &LineIndex, errors: &mut Vec<LintError>) {
        let last_meaningful = directive
            .children_with_tokens()
            .filter(|c| !c.kind().is_trivia())
            .last();

        if let Some(last) = last_meaningful {
            // text_range().end() is exclusive; subtract 1 to get the last character's position
            let end_offset: usize = last.text_range().end().into();
            let pos = line_index.position(end_offset.saturating_sub(1));
            let fix = Fix::replace_range(end_offset, end_offset, ";");
            errors.push(
                LintError::new(
                    "missing-semicolon",
                    "syntax",
                    "Missing semicolon at end of directive",
                    Severity::Error,
                )
                .with_location(pos.line, pos.column)
                .with_fix(fix),
            );
        }
    }

    /// Detect directives that were merged by the parser due to a missing
    /// semicolon between them.
    ///
    /// When a semicolon is missing between two directives on adjacent lines,
    /// the parser (which treats newlines as whitespace) merges them into a
    /// single DIRECTIVE node. We detect this by looking for NEWLINE tokens
    /// within a directive's children where:
    /// - Arguments have already been seen after the directive name
    /// - The next non-trivia token after the NEWLINE is an IDENT
    ///   (suggesting a new directive name)
    fn check_merged_directives(
        directive: &SyntaxNode,
        line_index: &LineIndex,
        errors: &mut Vec<LintError>,
    ) {
        let children: Vec<SyntaxElement> = directive.children_with_tokens().collect();

        let mut seen_name = false;
        let mut seen_args = false;

        for (i, child) in children.iter().enumerate() {
            let kind = child.kind();

            // Wait for the directive name (first non-trivia token)
            if !seen_name {
                if is_value_kind(kind) {
                    seen_name = true;
                }
                continue;
            }

            // Track whether we've seen arguments after the name
            if is_value_kind(kind) {
                seen_args = true;
                continue;
            }

            // Look for NEWLINE after we've seen arguments
            if kind == SyntaxKind::NEWLINE && seen_args {
                // Find the next non-whitespace/newline token
                let next_ident = children[i + 1..].iter().find(|c| {
                    c.kind() != SyntaxKind::WHITESPACE && c.kind() != SyntaxKind::NEWLINE
                });

                if let Some(next) = next_ident
                    && next.kind() == SyntaxKind::IDENT
                {
                    // NEWLINE followed by IDENT after args → likely merged directive
                    // Find the last non-trivia token before this NEWLINE
                    let last_before = children[..i].iter().rev().find(|c| !c.kind().is_trivia());

                    if let Some(last) = last_before {
                        // text_range().end() is exclusive; subtract 1 to get the last character's position
                        let end_offset: usize = last.text_range().end().into();
                        let pos = line_index.position(end_offset.saturating_sub(1));
                        let fix = Fix::replace_range(end_offset, end_offset, ";");
                        errors.push(
                            LintError::new(
                                "missing-semicolon",
                                "syntax",
                                "Missing semicolon at end of directive",
                                Severity::Error,
                            )
                            .with_location(pos.line, pos.column)
                            .with_fix(fix),
                        );
                    }

                    // Reset for next potential merged directive
                    seen_args = false;
                }
            }
        }
    }
}

/// Returns `true` for token kinds that represent directive names or arguments.
fn is_value_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IDENT
            | SyntaxKind::ARGUMENT
            | SyntaxKind::VARIABLE
            | SyntaxKind::DOUBLE_QUOTED_STRING
            | SyntaxKind::SINGLE_QUOTED_STRING
    )
}

impl LintRule for MissingSemicolon {
    fn name(&self) -> &'static str {
        "missing-semicolon"
    }

    fn category(&self) -> &'static str {
        "syntax"
    }

    fn description(&self) -> &'static str {
        "Detects missing semicolons at the end of directives"
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
    use crate::parser::ast::Config;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn check_content(content: &str) -> Vec<LintError> {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        let path = file.path().to_path_buf();

        let rule = MissingSemicolon;
        let config = Config::new();
        rule.check(&config, &path)
    }

    #[test]
    fn test_correct_semicolons() {
        let content = r#"worker_processes auto;

http {
    server {
        listen 80;
        server_name example.com;
    }
}
"#;
        let errors = check_content(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_missing_semicolon() {
        let content = r#"worker_processes auto

http {
    server {
        listen 80;
    }
}
"#;
        let errors = check_content(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("Missing semicolon"));
        assert_eq!(errors[0].line, Some(1));
    }

    #[test]
    fn test_missing_semicolon_in_block() {
        let content = r#"http {
    server {
        listen 80
        server_name example.com;
    }
}
"#;
        let errors = check_content(content);
        assert_eq!(errors.len(), 1, "Expected 1 error");
        assert_eq!(errors[0].line, Some(3));
    }

    #[test]
    fn test_block_directive_no_semicolon_needed() {
        let content = r#"http {
    server {
        listen 80;
    }
}
"#;
        let errors = check_content(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_comment_lines_ignored() {
        let content = r#"# This is a comment without semicolon
worker_processes auto;
"#;
        let errors = check_content(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_semicolon_in_string() {
        // Semicolon inside string, but line ends with proper semicolon
        let content = r#"http {
    server {
        return 200 "hello; world";
    }
}
"#;
        let errors = check_content(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_semicolon_in_string_no_trailing() {
        // Semicolon inside string but no trailing semicolon - should error
        let content = r#"http {
    server {
        return 200 "hello; world"
    }
}
"#;
        let errors = check_content(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_string_ending_with_semicolon() {
        // String content ends with semicolon but line doesn't
        let content = r#"http {
    server {
        return 200 "test;"
    }
}
"#;
        let errors = check_content(content);
        assert_eq!(
            errors.len(),
            1,
            "Expected 1 error - string ending with ; is not a real semicolon"
        );
    }

    #[test]
    fn test_comment_ending_with_semicolon() {
        // Comment ends with semicolon but directive doesn't have one
        let content = r#"http {
    server {
        listen 80  # port number;
    }
}
"#;
        let errors = check_content(content);
        assert_eq!(
            errors.len(),
            1,
            "Expected 1 error - comment ending with ; should not count"
        );
    }

    #[test]
    fn test_lua_block_ignored() {
        // Lua code inside lua_block should not trigger missing semicolon errors
        let content = r#"http {
    server {
        listen 80;
        content_by_lua_block {
            local cjson = require "cjson"
            ngx.say(cjson.encode({status = "ok"}))
        }
    }
}
"#;
        let errors = check_content(content);
        assert!(
            errors.is_empty(),
            "Expected no errors in lua_block, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_multiple_lua_blocks_ignored() {
        let content = r#"http {
    init_by_lua_block {
        require "resty.core"
        cjson = require "cjson"
    }

    server {
        listen 80;
        access_by_lua_block {
            local token = ngx.var.http_authorization
            if not token then
                ngx.exit(ngx.HTTP_UNAUTHORIZED)
            end
        }
    }
}
"#;
        let errors = check_content(content);
        assert!(
            errors.is_empty(),
            "Expected no errors in lua_blocks, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_semicolon_with_trailing_comment() {
        // Semicolon followed by comment should work correctly
        let content = r#"http {
    upstream backend {
        server 10.0.0.1:8080; # backend-a
        server 10.0.0.2:8080; # backend-b
    }
}
"#;
        let errors = check_content(content);
        assert!(
            errors.is_empty(),
            "Expected no errors for semicolon with trailing comment, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_multiline_string_continuation_not_flagged() {
        // Multi-line directive with quoted string continuation across lines
        // should not be flagged (next token after NEWLINE is a string, not IDENT)
        let content = r#"http {
    log_format main '$remote_addr - $remote_user'
        '$request $status';
}
"#;
        let errors = check_content(content);
        assert!(
            errors.is_empty(),
            "Expected no errors for multi-line string continuation, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_multiline_variable_continuation_not_flagged() {
        // Multi-line directive where continuation starts with a variable
        // should not be flagged (next token after NEWLINE is VARIABLE, not IDENT)
        let content = r#"http {
    server {
        proxy_set_header Host
            $host;
    }
}
"#;
        let errors = check_content(content);
        assert!(
            errors.is_empty(),
            "Expected no errors for variable continuation, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_multiline_ident_continuation_flagged() {
        // When an IDENT token follows a NEWLINE after arguments, the heuristic
        // treats it as a merged directive. This is a known limitation: legitimate
        // multi-line directives where a continuation arg is an IDENT (e.g.
        // `add_header X-H\n    value;`) will be flagged as missing semicolon.
        // In practice, such line-splitting is uncommon in nginx configs.
        let content = r#"http {
    server {
        add_header X-Header
            value;
    }
}
"#;
        let errors = check_content(content);
        assert_eq!(
            errors.len(),
            1,
            "IDENT after NEWLINE is flagged as merged directive (known limitation)"
        );
    }

    #[test]
    fn test_hash_in_regex_not_treated_as_comment() {
        // Hash inside regex pattern should not be treated as comment start
        let content = r#"http {
    server {
        location ~* foo#bar {
            deny all;
        }
    }
}
"#;
        let errors = check_content(content);
        assert!(
            errors.is_empty(),
            "Expected no errors for hash in regex, got: {:?}",
            errors
        );
    }
}
