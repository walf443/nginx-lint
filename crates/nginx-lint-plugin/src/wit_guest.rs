//! WIT component model guest bindings
//!
//! This module provides the bridge between the existing Plugin trait
//! and the WIT-generated Guest trait for component model plugins.

// Generate guest-side bindings from the WIT file
wit_bindgen::generate!({
    path: "../../wit/nginx-lint-plugin.wit",
    world: "plugin",
    pub_export_macro: true,
});

/// Convert SDK PluginSpec to WIT PluginSpec
pub fn convert_spec(sdk_spec: super::PluginSpec) -> nginx_lint::plugin::types::PluginSpec {
    nginx_lint::plugin::types::PluginSpec {
        name: sdk_spec.name,
        category: sdk_spec.category,
        description: sdk_spec.description,
        api_version: sdk_spec.api_version,
        severity: sdk_spec.severity,
        why: sdk_spec.why,
        bad_example: sdk_spec.bad_example,
        good_example: sdk_spec.good_example,
        references: sdk_spec.references,
    }
}

/// Convert SDK Severity to WIT Severity
pub fn convert_severity(severity: super::Severity) -> nginx_lint::plugin::types::Severity {
    match severity {
        super::Severity::Error => nginx_lint::plugin::types::Severity::Error,
        super::Severity::Warning => nginx_lint::plugin::types::Severity::Warning,
    }
}

/// Convert SDK Fix to WIT Fix
pub fn convert_fix(fix: super::Fix) -> nginx_lint::plugin::types::Fix {
    nginx_lint::plugin::types::Fix {
        line: fix.line as u32,
        old_text: fix.old_text,
        new_text: fix.new_text,
        delete_line: fix.delete_line,
        insert_after: fix.insert_after,
        start_offset: fix.start_offset.map(|v| v as u32),
        end_offset: fix.end_offset.map(|v| v as u32),
    }
}

/// Convert SDK LintError to WIT LintError
pub fn convert_lint_error(error: super::LintError) -> nginx_lint::plugin::types::LintError {
    nginx_lint::plugin::types::LintError {
        rule: error.rule,
        category: error.category,
        message: error.message,
        severity: convert_severity(error.severity),
        line: error.line.map(|v| v as u32),
        column: error.column.map(|v| v as u32),
        fixes: error.fixes.into_iter().map(convert_fix).collect(),
    }
}

/// Reconstruct a parser Config from a WIT config resource handle.
///
/// This calls host functions to retrieve the config data and builds
/// a parser::Config that plugins can use unchanged.
pub fn reconstruct_config(
    config: &nginx_lint::plugin::config_api::Config,
) -> crate::parser::ast::Config {
    use crate::parser::ast;

    let items = reconstruct_config_items(&config.items());
    let include_context = config.include_context();

    ast::Config {
        items,
        include_context,
    }
}

/// Reconstruct parser ConfigItems from WIT ConfigItems.
fn reconstruct_config_items(
    items: &[nginx_lint::plugin::config_api::ConfigItem],
) -> Vec<crate::parser::ast::ConfigItem> {
    use crate::parser::ast;
    use nginx_lint::plugin::config_api::ConfigItem as WitConfigItem;

    items
        .iter()
        .map(|item| match item {
            WitConfigItem::DirectiveItem(dir_handle) => {
                ast::ConfigItem::Directive(Box::new(reconstruct_directive(dir_handle)))
            }
            WitConfigItem::CommentItem(c) => ast::ConfigItem::Comment(ast::Comment {
                text: c.text.clone(),
                span: ast::Span::new(
                    ast::Position::new(c.line as usize, c.column as usize, c.start_offset as usize),
                    ast::Position::new(
                        c.line as usize,
                        c.column as usize + c.text.len(),
                        c.end_offset as usize,
                    ),
                ),
                leading_whitespace: c.leading_whitespace.clone(),
                trailing_whitespace: c.trailing_whitespace.clone(),
            }),
            WitConfigItem::BlankLineItem(b) => ast::ConfigItem::BlankLine(ast::BlankLine {
                span: ast::Span::new(
                    ast::Position::new(b.line as usize, 1, b.start_offset as usize),
                    ast::Position::new(
                        b.line as usize,
                        1 + b.content.len(),
                        b.start_offset as usize + b.content.len(),
                    ),
                ),
                content: b.content.clone(),
            }),
        })
        .collect()
}

/// Reconstruct a parser Directive from a WIT directive resource handle.
fn reconstruct_directive(
    handle: &nginx_lint::plugin::config_api::Directive,
) -> crate::parser::ast::Directive {
    use crate::parser::ast;

    let name = handle.name();
    let args: Vec<ast::Argument> = handle
        .args()
        .iter()
        .map(|a| {
            let value = match a.arg_type {
                nginx_lint::plugin::config_api::ArgumentType::Literal => {
                    ast::ArgumentValue::Literal(a.value.clone())
                }
                nginx_lint::plugin::config_api::ArgumentType::QuotedString => {
                    ast::ArgumentValue::QuotedString(a.value.clone())
                }
                nginx_lint::plugin::config_api::ArgumentType::SingleQuotedString => {
                    ast::ArgumentValue::SingleQuotedString(a.value.clone())
                }
                nginx_lint::plugin::config_api::ArgumentType::Variable => {
                    ast::ArgumentValue::Variable(a.value.clone())
                }
            };
            ast::Argument {
                value,
                span: ast::Span::new(
                    ast::Position::new(a.line as usize, a.column as usize, a.start_offset as usize),
                    ast::Position::new(
                        a.line as usize,
                        a.column as usize + a.raw.len(),
                        a.end_offset as usize,
                    ),
                ),
                raw: a.raw.clone(),
            }
        })
        .collect();

    let line = handle.line() as usize;
    let column = handle.column() as usize;
    let start_offset = handle.start_offset() as usize;
    let end_offset = handle.end_offset() as usize;
    let leading_whitespace = handle.leading_whitespace();
    let trailing_whitespace = handle.trailing_whitespace();
    let space_before_terminator = handle.space_before_terminator();

    let block = if handle.has_block() {
        let block_items = reconstruct_config_items(&handle.block_items());
        Some(ast::Block {
            items: block_items,
            span: ast::Span::new(
                ast::Position::new(line, column + name.len() + 1, start_offset + name.len() + 1),
                ast::Position::new(line, column, end_offset),
            ),
            raw_content: if handle.block_is_raw() {
                Some(String::new()) // marker for raw block
            } else {
                None
            },
            closing_brace_leading_whitespace: String::new(),
            trailing_whitespace: String::new(),
        })
    } else {
        None
    };

    let name_len = name.len();
    ast::Directive {
        name,
        name_span: ast::Span::new(
            ast::Position::new(line, column, start_offset),
            ast::Position::new(line, column + name_len, start_offset + name_len),
        ),
        args,
        block,
        span: ast::Span::new(
            ast::Position::new(line, column, start_offset),
            ast::Position::new(line, column, end_offset),
        ),
        trailing_comment: None,
        leading_whitespace,
        space_before_terminator,
        trailing_whitespace,
    }
}
