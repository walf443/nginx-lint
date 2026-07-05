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
        min_nginx_version: sdk_spec.min_nginx_version,
        max_nginx_version: sdk_spec.max_nginx_version,
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
/// Fetches the entire config in a single `snapshot()` host call (a flat
/// DFS-ordered array with index-based child references) and rebuilds the
/// tree guest-side. One WIT boundary crossing regardless of config size,
/// instead of two calls (`data` + `block-items`) per directive.
pub fn reconstruct_config(
    config: &nginx_lint::plugin::config_api::Config,
) -> crate::parser::ast::Config {
    use crate::parser::ast;

    let snapshot = config.snapshot();
    // Slots let each flat item be moved out exactly once while children are
    // resolved by index, avoiding a clone of every string in the config
    let mut slots: Vec<Option<nginx_lint::plugin::config_api::FlatItem>> =
        snapshot.all_items.into_iter().map(Some).collect();
    let items = snapshot
        .top_level_indices
        .iter()
        .map(|&index| build_item(&mut slots, index))
        .collect();

    ast::Config {
        items,
        include_context: snapshot.include_context,
    }
}

/// Rebuild the config item at `index` (and, recursively, its block children)
/// from the snapshot's flat array.
fn build_item(
    slots: &mut [Option<nginx_lint::plugin::config_api::FlatItem>],
    index: u32,
) -> crate::parser::ast::ConfigItem {
    use crate::parser::ast;
    use nginx_lint::plugin::parser_types::ConfigItemValue;

    let item = slots[index as usize]
        .take()
        .expect("snapshot child index out of range or visited twice");
    let children: Vec<ast::ConfigItem> = item
        .child_indices
        .iter()
        .map(|&child| build_item(slots, child))
        .collect();

    match item.value {
        ConfigItemValue::DirectiveItem(d) => {
            ast::ConfigItem::Directive(Box::new(directive_from_data(d, children)))
        }
        ConfigItemValue::CommentItem(c) => ast::ConfigItem::Comment(ast::Comment {
            span: ast::Span::new(
                ast::Position::new(c.line as usize, c.column as usize, c.start_offset as usize),
                ast::Position::new(
                    c.line as usize,
                    c.column as usize + c.text.chars().count(),
                    c.end_offset as usize,
                ),
            ),
            leading_whitespace: c.leading_whitespace,
            trailing_whitespace: c.trailing_whitespace,
            text: c.text,
        }),
        ConfigItemValue::BlankLineItem(b) => ast::ConfigItem::BlankLine(ast::BlankLine {
            span: ast::Span::new(
                ast::Position::new(b.line as usize, 1, b.start_offset as usize),
                ast::Position::new(
                    b.line as usize,
                    1 + b.content.chars().count(),
                    b.start_offset as usize + b.content.len(),
                ),
            ),
            content: b.content,
        }),
    }
}

/// Convert a WIT argument-info to a parser Argument (by value, no clones).
fn argument_from_info(
    a: nginx_lint::plugin::config_api::ArgumentInfo,
) -> crate::parser::ast::Argument {
    use crate::parser::ast;

    let value = match a.arg_type {
        nginx_lint::plugin::config_api::ArgumentType::Literal => {
            ast::ArgumentValue::Literal(a.value)
        }
        nginx_lint::plugin::config_api::ArgumentType::QuotedString => {
            ast::ArgumentValue::QuotedString(a.value)
        }
        nginx_lint::plugin::config_api::ArgumentType::SingleQuotedString => {
            ast::ArgumentValue::SingleQuotedString(a.value)
        }
        nginx_lint::plugin::config_api::ArgumentType::Variable => {
            ast::ArgumentValue::Variable(a.value)
        }
    };
    ast::Argument {
        value,
        span: ast::Span::new(
            ast::Position::new(a.line as usize, a.column as usize, a.start_offset as usize),
            ast::Position::new(
                a.line as usize,
                a.column as usize + a.raw.chars().count(),
                a.end_offset as usize,
            ),
        ),
        raw: a.raw,
    }
}

/// Build a parser Directive from an owned WIT directive-data record and the
/// already-reconstructed block children (empty when there is no block).
fn directive_from_data(
    d: nginx_lint::plugin::config_api::DirectiveData,
    block_items: Vec<crate::parser::ast::ConfigItem>,
) -> crate::parser::ast::Directive {
    use crate::parser::ast;

    let args: Vec<ast::Argument> = d.args.into_iter().map(argument_from_info).collect();

    let line = d.line as usize;
    let column = d.column as usize;
    let start_offset = d.start_offset as usize;
    let end_offset = d.end_offset as usize;
    let end_line = d.end_line as usize;
    let end_column = d.end_column as usize;

    let block = if d.has_block {
        // Use the actual block span start (position of '{') when available,
        // falling back to directive start for backwards compatibility.
        let block_start_line = d.block_start_line.unwrap_or(line as u32) as usize;
        let block_start_column = d.block_start_column.unwrap_or(column as u32) as usize;
        let block_start_offset = d.block_start_offset.unwrap_or(start_offset as u32) as usize;
        Some(ast::Block {
            items: block_items,
            span: ast::Span::new(
                ast::Position::new(block_start_line, block_start_column, block_start_offset),
                ast::Position::new(end_line, end_column, end_offset),
            ),
            raw_content: d.block_raw_content,
            closing_brace_leading_whitespace: d
                .closing_brace_leading_whitespace
                .unwrap_or_default(),
            trailing_whitespace: d.block_trailing_whitespace.unwrap_or_default(),
        })
    } else {
        None
    };

    let name_end_column = d.name_end_column as usize;
    let name_end_offset = d.name_end_offset as usize;

    let trailing_comment = d.trailing_comment_text.map(|text| ast::Comment {
        span: ast::Span::new(
            ast::Position::new(line, 0, 0),
            ast::Position::new(line, 0, 0),
        ),
        leading_whitespace: String::new(),
        trailing_whitespace: String::new(),
        text,
    });

    ast::Directive {
        name: d.name,
        name_span: ast::Span::new(
            ast::Position::new(line, column, start_offset),
            ast::Position::new(line, name_end_column, name_end_offset),
        ),
        args,
        block,
        span: ast::Span::new(
            ast::Position::new(line, column, start_offset),
            ast::Position::new(end_line, end_column, end_offset),
        ),
        trailing_comment,
        leading_whitespace: d.leading_whitespace,
        space_before_terminator: d.space_before_terminator,
        trailing_whitespace: d.trailing_whitespace,
    }
}
