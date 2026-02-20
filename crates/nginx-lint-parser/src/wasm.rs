use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::ast::{Argument, ArgumentValue, Comment, Config, ConfigItem, Directive};

/// Parse an nginx configuration string and return JSON AST.
///
/// Returns JSON string of the parsed Config AST on success,
/// or a JSON object `{"error": "message"}` on parse failure.
#[wasm_bindgen]
pub fn parse_string_to_json(source: &str) -> String {
    match crate::parse_string(source) {
        Ok(config) => serde_json::to_string(&config)
            .unwrap_or_else(|e| format!(r#"{{"error":"serialization error: {}"}}"#, e)),
        Err(e) => {
            let msg = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
            format!(r#"{{"error":"{}"}}"#, msg)
        }
    }
}

/// Parse an nginx configuration string and return WIT-compatible JSON.
///
/// Unlike `parse_string_to_json` which returns the raw AST, this function
/// returns pre-computed data matching the WIT interface types:
/// - `directivesWithContext`: Flat list with parent stack (from DFS traversal)
/// - `includeContext`: The include context used for traversal
/// - `items`: Top-level config items
///
/// The `include_context_json` parameter is a JSON array of parent block names
/// (e.g., `["http", "server"]`), or an empty string for root files.
#[wasm_bindgen]
pub fn parse_to_wit_json(source: &str, include_context_json: &str) -> String {
    match crate::parse_string(source) {
        Ok(mut config) => {
            if !include_context_json.is_empty() {
                if let Ok(ctx) = serde_json::from_str::<Vec<String>>(include_context_json) {
                    config.include_context = ctx;
                }
            }
            let output = build_wit_output(&config);
            serde_json::to_string(&output)
                .unwrap_or_else(|e| format!(r#"{{"error":"serialization error: {}"}}"#, e))
        }
        Err(e) => {
            let msg = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
            format!(r#"{{"error":"{}"}}"#, msg)
        }
    }
}

// ── WIT-compatible serializable types ──────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WitOutput {
    directives_with_context: Vec<WitDirectiveContext>,
    include_context: Vec<String>,
    items: Vec<WitConfigItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WitDirectiveContext {
    data: WitDirectiveData,
    block_items: Vec<WitConfigItem>,
    parent_stack: Vec<String>,
    depth: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WitDirectiveData {
    name: String,
    args: Vec<WitArgumentInfo>,
    line: usize,
    column: usize,
    start_offset: usize,
    end_offset: usize,
    end_line: usize,
    end_column: usize,
    leading_whitespace: String,
    trailing_whitespace: String,
    space_before_terminator: String,
    has_block: bool,
    block_is_raw: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    block_raw_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    closing_brace_leading_whitespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    block_trailing_whitespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trailing_comment_text: Option<String>,
    name_end_column: usize,
    name_end_offset: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    block_start_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    block_start_column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    block_start_offset: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WitArgumentInfo {
    value: String,
    raw: String,
    arg_type: String,
    line: usize,
    column: usize,
    start_offset: usize,
    end_offset: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WitCommentInfo {
    text: String,
    line: usize,
    column: usize,
    leading_whitespace: String,
    trailing_whitespace: String,
    start_offset: usize,
    end_offset: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WitBlankLineInfo {
    line: usize,
    content: String,
    start_offset: usize,
}

#[derive(Serialize)]
#[serde(tag = "tag", rename_all = "camelCase")]
enum WitConfigItem {
    #[serde(rename = "directive-item")]
    DirectiveItem {
        data: WitDirectiveData,
        #[serde(rename = "blockItems")]
        block_items: Vec<WitConfigItem>,
    },
    #[serde(rename = "comment-item")]
    CommentItem { val: WitCommentInfo },
    #[serde(rename = "blank-line-item")]
    BlankLineItem { val: WitBlankLineInfo },
}

// ── Conversion functions ──────────────────────────────────────────

fn convert_argument(arg: &Argument) -> WitArgumentInfo {
    let (value, arg_type) = match &arg.value {
        ArgumentValue::Literal(s) => (s.clone(), "literal"),
        ArgumentValue::QuotedString(s) => (s.clone(), "quoted-string"),
        ArgumentValue::SingleQuotedString(s) => (s.clone(), "single-quoted-string"),
        ArgumentValue::Variable(s) => (s.clone(), "variable"),
    };
    WitArgumentInfo {
        value,
        raw: arg.raw.clone(),
        arg_type: arg_type.to_string(),
        line: arg.span.start.line,
        column: arg.span.start.column,
        start_offset: arg.span.start.offset,
        end_offset: arg.span.end.offset,
    }
}

fn convert_directive(d: &Directive) -> WitDirectiveData {
    WitDirectiveData {
        name: d.name.clone(),
        args: d.args.iter().map(convert_argument).collect(),
        line: d.span.start.line,
        column: d.span.start.column,
        start_offset: d.span.start.offset,
        end_offset: d.span.end.offset,
        end_line: d.span.end.line,
        end_column: d.span.end.column,
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
        name_end_column: d.name_span.end.column,
        name_end_offset: d.name_span.end.offset,
        block_start_line: d.block.as_ref().map(|b| b.span.start.line),
        block_start_column: d.block.as_ref().map(|b| b.span.start.column),
        block_start_offset: d.block.as_ref().map(|b| b.span.start.offset),
    }
}

fn convert_comment(c: &Comment) -> WitCommentInfo {
    WitCommentInfo {
        text: c.text.clone(),
        line: c.span.start.line,
        column: c.span.start.column,
        leading_whitespace: c.leading_whitespace.clone(),
        trailing_whitespace: c.trailing_whitespace.clone(),
        start_offset: c.span.start.offset,
        end_offset: c.span.end.offset,
    }
}

fn convert_config_item(item: &ConfigItem) -> WitConfigItem {
    match item {
        ConfigItem::Directive(d) => WitConfigItem::DirectiveItem {
            data: convert_directive(d),
            block_items: d
                .block
                .as_ref()
                .map(|b| b.items.iter().map(convert_config_item).collect())
                .unwrap_or_default(),
        },
        ConfigItem::Comment(c) => WitConfigItem::CommentItem {
            val: convert_comment(c),
        },
        ConfigItem::BlankLine(b) => WitConfigItem::BlankLineItem {
            val: WitBlankLineInfo {
                line: b.span.start.line,
                content: b.content.clone(),
                start_offset: b.span.start.offset,
            },
        },
    }
}

fn build_wit_output(config: &Config) -> WitOutput {
    let directives_with_context = config
        .all_directives_with_context()
        .map(|ctx| {
            let d = ctx.directive;
            WitDirectiveContext {
                data: convert_directive(d),
                block_items: d
                    .block
                    .as_ref()
                    .map(|b| b.items.iter().map(convert_config_item).collect())
                    .unwrap_or_default(),
                parent_stack: ctx.parent_stack,
                depth: ctx.depth,
            }
        })
        .collect();

    let items = config.items.iter().map(convert_config_item).collect();

    WitOutput {
        directives_with_context,
        include_context: config.include_context.clone(),
        items,
    }
}
