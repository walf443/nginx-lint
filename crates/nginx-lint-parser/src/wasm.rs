use crate::ast::{Argument, ArgumentValue, Comment, Config, ConfigItem, Directive};

// Generate guest-side bindings from the WIT file for the parser world
wit_bindgen::generate!({
    path: "../../wit/nginx-lint-plugin.wit",
    world: "parser",
    pub_export_macro: true,
});

struct ParserComponent;

impl Guest for ParserComponent {
    fn parse_config(
        source: String,
        include_context: Vec<String>,
    ) -> Result<nginx_lint::plugin::parser_types::ParseOutput, String> {
        let mut config = crate::parse_string(&source).map_err(|e| e.to_string())?;
        config.include_context = include_context;
        Ok(build_parse_output(&config))
    }
}

export!(ParserComponent);

// ── Type aliases for generated types ────────────────────────────────

use nginx_lint::plugin::data_types as dt;
use nginx_lint::plugin::parser_types as pt;

// ── Conversion functions ────────────────────────────────────────────

fn convert_argument(arg: &Argument) -> dt::ArgumentInfo {
    let arg_type = match &arg.value {
        ArgumentValue::Literal(_) => dt::ArgumentType::Literal,
        ArgumentValue::QuotedString(_) => dt::ArgumentType::QuotedString,
        ArgumentValue::SingleQuotedString(_) => dt::ArgumentType::SingleQuotedString,
        ArgumentValue::Variable(_) => dt::ArgumentType::Variable,
    };
    dt::ArgumentInfo {
        value: arg.as_str().to_string(),
        raw: arg.raw.clone(),
        arg_type,
        line: arg.span.start.line as u32,
        column: arg.span.start.column as u32,
        start_offset: arg.span.start.offset as u32,
        end_offset: arg.span.end.offset as u32,
    }
}

fn convert_directive(d: &Directive) -> dt::DirectiveData {
    dt::DirectiveData {
        name: d.name.clone(),
        args: d.args.iter().map(convert_argument).collect(),
        line: d.span.start.line as u32,
        column: d.span.start.column as u32,
        start_offset: d.span.start.offset as u32,
        end_offset: d.span.end.offset as u32,
        end_line: d.span.end.line as u32,
        end_column: d.span.end.column as u32,
        leading_whitespace: d.leading_whitespace.clone(),
        trailing_whitespace: d.trailing_whitespace.clone(),
        space_before_terminator: d.space_before_terminator.clone(),
        has_block: d.block.is_some(),
        block_is_raw: d.block.as_ref().is_some_and(|b| b.is_raw()),
        block_raw_content: d.block.as_ref().and_then(|b| b.raw_content.clone()),
        closing_brace_leading_whitespace: d
            .block
            .as_ref()
            .map(|b| b.closing_brace_leading_whitespace.clone()),
        block_trailing_whitespace: d.block.as_ref().map(|b| b.trailing_whitespace.clone()),
        trailing_comment_text: d.trailing_comment.as_ref().map(|c| c.text.clone()),
        name_end_column: d.name_span.end.column as u32,
        name_end_offset: d.name_span.end.offset as u32,
        block_start_line: d.block.as_ref().map(|b| b.span.start.line as u32),
        block_start_column: d.block.as_ref().map(|b| b.span.start.column as u32),
        block_start_offset: d.block.as_ref().map(|b| b.span.start.offset as u32),
    }
}

fn convert_comment(c: &Comment) -> dt::CommentInfo {
    dt::CommentInfo {
        text: c.text.clone(),
        line: c.span.start.line as u32,
        column: c.span.start.column as u32,
        leading_whitespace: c.leading_whitespace.clone(),
        trailing_whitespace: c.trailing_whitespace.clone(),
        start_offset: c.span.start.offset as u32,
        end_offset: c.span.end.offset as u32,
    }
}

/// Flatten all config items into a DFS-ordered array, recording child indices.
/// Returns (all_items, top_level_indices).
fn flatten_config_items(items: &[ConfigItem]) -> (Vec<pt::ConfigItem>, Vec<u32>) {
    let mut all_items: Vec<pt::ConfigItem> = Vec::new();
    let mut top_level_indices: Vec<u32> = Vec::new();

    for item in items {
        let idx = flatten_item(item, &mut all_items);
        top_level_indices.push(idx);
    }

    (all_items, top_level_indices)
}

/// Recursively flatten a single config item, returning its index in all_items.
fn flatten_item(item: &ConfigItem, all_items: &mut Vec<pt::ConfigItem>) -> u32 {
    match item {
        ConfigItem::Directive(d) => {
            // Reserve index for this directive
            let idx = all_items.len() as u32;
            // Push a placeholder (will be replaced after processing children)
            all_items.push(pt::ConfigItem {
                value: pt::ConfigItemValue::DirectiveItem(convert_directive(d)),
                child_indices: Vec::new(),
            });

            // Process block children if present
            let child_indices: Vec<u32> = if let Some(block) = &d.block {
                block
                    .items
                    .iter()
                    .map(|child| flatten_item(child, all_items))
                    .collect()
            } else {
                Vec::new()
            };

            // Update the placeholder with actual child indices
            all_items[idx as usize].child_indices = child_indices;
            idx
        }
        ConfigItem::Comment(c) => {
            let idx = all_items.len() as u32;
            all_items.push(pt::ConfigItem {
                value: pt::ConfigItemValue::CommentItem(convert_comment(c)),
                child_indices: Vec::new(),
            });
            idx
        }
        ConfigItem::BlankLine(b) => {
            let idx = all_items.len() as u32;
            all_items.push(pt::ConfigItem {
                value: pt::ConfigItemValue::BlankLineItem(dt::BlankLineInfo {
                    line: b.span.start.line as u32,
                    content: b.content.clone(),
                    start_offset: b.span.start.offset as u32,
                }),
                child_indices: Vec::new(),
            });
            idx
        }
    }
}

fn build_parse_output(config: &Config) -> pt::ParseOutput {
    // Flatten all items into a DFS-ordered array
    let (all_items, top_level_indices) = flatten_config_items(&config.items);

    // Build directives-with-context from the flat items
    let directives_with_context = build_directive_contexts(config, &all_items, &top_level_indices);

    pt::ParseOutput {
        directives_with_context,
        include_context: config.include_context.clone(),
        all_items,
        top_level_indices,
    }
}

/// Build the flat directives-with-context list by traversing the index-based tree.
fn build_directive_contexts(
    config: &Config,
    all_items: &[pt::ConfigItem],
    top_level_indices: &[u32],
) -> Vec<pt::DirectiveContext> {
    let mut results = Vec::new();
    collect_directive_contexts(
        all_items,
        top_level_indices,
        &config.include_context,
        &mut results,
    );
    results
}

fn collect_directive_contexts(
    all_items: &[pt::ConfigItem],
    indices: &[u32],
    parent_stack: &[String],
    results: &mut Vec<pt::DirectiveContext>,
) {
    for &idx in indices {
        let item = &all_items[idx as usize];
        if let pt::ConfigItemValue::DirectiveItem(ref data) = item.value {
            results.push(pt::DirectiveContext {
                data: data.clone(),
                block_item_indices: item.child_indices.clone(),
                parent_stack: parent_stack.to_vec(),
                depth: parent_stack.len() as u32,
            });

            // Recurse into block children
            if !item.child_indices.is_empty() {
                let mut child_stack = parent_stack.to_vec();
                child_stack.push(data.name.clone());
                collect_directive_contexts(all_items, &item.child_indices, &child_stack, results);
            }
        }
    }
}
