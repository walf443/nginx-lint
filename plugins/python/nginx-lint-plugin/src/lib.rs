//! Native half of the nginx-lint-plugin Python SDK, exposed as
//! `nginx_lint_plugin._native`.
//!
//! Exposes `parse_config_json(source, include_context) -> str`, which returns
//! the same `parse-output` structure the parser WASM component produces (see
//! `world parser` in `wit/nginx-lint-plugin.wit`), serialized as JSON with
//! snake_case keys matching the componentize-py generated dataclasses.
//!
//! NOTE: the conversion below mirrors `build_parse_output` in
//! `crates/nginx-lint-parser/src/wasm.rs` (which targets wit-bindgen types
//! and is not reusable here). If the WIT `parser-types` records change, both
//! places must be updated.

use nginx_lint_parser::ast::{Argument, ArgumentValue, Comment, Config, ConfigItem, Directive};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde::Serialize;

// ── JSON mirror of the WIT parser-types records ─────────────────────

#[derive(Serialize, Clone)]
struct ArgumentInfoJson {
    value: String,
    raw: String,
    arg_type: &'static str,
    line: u32,
    column: u32,
    start_offset: u32,
    end_offset: u32,
}

#[derive(Serialize, Clone)]
struct DirectiveDataJson {
    name: String,
    args: Vec<ArgumentInfoJson>,
    line: u32,
    column: u32,
    start_offset: u32,
    end_offset: u32,
    end_line: u32,
    end_column: u32,
    leading_whitespace: String,
    trailing_whitespace: String,
    space_before_terminator: String,
    has_block: bool,
    block_is_raw: bool,
    block_raw_content: Option<String>,
    closing_brace_leading_whitespace: Option<String>,
    block_trailing_whitespace: Option<String>,
    trailing_comment_text: Option<String>,
    name_end_column: u32,
    name_end_offset: u32,
    block_start_line: Option<u32>,
    block_start_column: Option<u32>,
    block_start_offset: Option<u32>,
}

#[derive(Serialize)]
struct CommentInfoJson {
    text: String,
    line: u32,
    column: u32,
    leading_whitespace: String,
    trailing_whitespace: String,
    start_offset: u32,
    end_offset: u32,
}

#[derive(Serialize)]
struct BlankLineInfoJson {
    line: u32,
    content: String,
    start_offset: u32,
}

#[derive(Serialize)]
#[serde(tag = "tag", content = "val", rename_all = "kebab-case")]
// The shared `Item` suffix mirrors the WIT `config-item-value` variants, and
// the kebab-case rename turns it into the tag the Python side matches on
#[allow(clippy::enum_variant_names)]
enum ConfigItemValueJson {
    DirectiveItem(DirectiveDataJson),
    CommentItem(CommentInfoJson),
    BlankLineItem(BlankLineInfoJson),
}

#[derive(Serialize)]
struct ConfigItemJson {
    value: ConfigItemValueJson,
    child_indices: Vec<u32>,
}

#[derive(Serialize)]
struct DirectiveContextJson {
    data: DirectiveDataJson,
    block_item_indices: Vec<u32>,
    parent_stack: Vec<String>,
    depth: u32,
}

#[derive(Serialize)]
struct ParseOutputJson {
    directives_with_context: Vec<DirectiveContextJson>,
    include_context: Vec<String>,
    all_items: Vec<ConfigItemJson>,
    top_level_indices: Vec<u32>,
}

// ── Conversion functions (mirroring wasm.rs) ────────────────────────

fn convert_argument(arg: &Argument) -> ArgumentInfoJson {
    let arg_type = match &arg.value {
        ArgumentValue::Literal(_) => "literal",
        ArgumentValue::QuotedString(_) => "quoted-string",
        ArgumentValue::SingleQuotedString(_) => "single-quoted-string",
        ArgumentValue::Variable(_) => "variable",
    };
    ArgumentInfoJson {
        value: arg.as_str().to_string(),
        raw: arg.raw.clone(),
        arg_type,
        line: arg.span.start.line as u32,
        column: arg.span.start.column as u32,
        start_offset: arg.span.start.offset as u32,
        end_offset: arg.span.end.offset as u32,
    }
}

fn convert_directive(d: &Directive) -> DirectiveDataJson {
    DirectiveDataJson {
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

fn convert_comment(c: &Comment) -> CommentInfoJson {
    CommentInfoJson {
        text: c.text.clone(),
        line: c.span.start.line as u32,
        column: c.span.start.column as u32,
        leading_whitespace: c.leading_whitespace.clone(),
        trailing_whitespace: c.trailing_whitespace.clone(),
        start_offset: c.span.start.offset as u32,
        end_offset: c.span.end.offset as u32,
    }
}

/// Recursively flatten a single config item, returning its index in all_items.
fn flatten_item(item: &ConfigItem, all_items: &mut Vec<ConfigItemJson>) -> u32 {
    match item {
        ConfigItem::Directive(d) => {
            let idx = all_items.len() as u32;
            all_items.push(ConfigItemJson {
                value: ConfigItemValueJson::DirectiveItem(convert_directive(d)),
                child_indices: Vec::new(),
            });

            let child_indices: Vec<u32> = if let Some(block) = &d.block {
                block
                    .items
                    .iter()
                    .map(|child| flatten_item(child, all_items))
                    .collect()
            } else {
                Vec::new()
            };

            all_items[idx as usize].child_indices = child_indices;
            idx
        }
        ConfigItem::Comment(c) => {
            let idx = all_items.len() as u32;
            all_items.push(ConfigItemJson {
                value: ConfigItemValueJson::CommentItem(convert_comment(c)),
                child_indices: Vec::new(),
            });
            idx
        }
        ConfigItem::BlankLine(b) => {
            let idx = all_items.len() as u32;
            all_items.push(ConfigItemJson {
                value: ConfigItemValueJson::BlankLineItem(BlankLineInfoJson {
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

fn collect_directive_contexts(
    all_items: &[ConfigItemJson],
    indices: &[u32],
    parent_stack: &[String],
    results: &mut Vec<DirectiveContextJson>,
) {
    for &idx in indices {
        let item = &all_items[idx as usize];
        if let ConfigItemValueJson::DirectiveItem(ref data) = item.value {
            results.push(DirectiveContextJson {
                data: data.clone(),
                block_item_indices: item.child_indices.clone(),
                parent_stack: parent_stack.to_vec(),
                depth: parent_stack.len() as u32,
            });

            if !item.child_indices.is_empty() {
                let mut child_stack = parent_stack.to_vec();
                child_stack.push(data.name.clone());
                collect_directive_contexts(all_items, &item.child_indices, &child_stack, results);
            }
        }
    }
}

fn build_parse_output(config: &Config) -> ParseOutputJson {
    let mut all_items: Vec<ConfigItemJson> = Vec::new();
    let mut top_level_indices: Vec<u32> = Vec::new();
    for item in &config.items {
        let idx = flatten_item(item, &mut all_items);
        top_level_indices.push(idx);
    }

    let mut directives_with_context = Vec::new();
    collect_directive_contexts(
        &all_items,
        &top_level_indices,
        &config.include_context,
        &mut directives_with_context,
    );

    ParseOutputJson {
        directives_with_context,
        include_context: config.include_context.clone(),
        all_items,
        top_level_indices,
    }
}

// ── Python API ──────────────────────────────────────────────────────

/// Parse an nginx config string and return the parse-output as a JSON string.
#[pyfunction]
#[pyo3(signature = (source, include_context = Vec::new()))]
fn parse_config_json(source: &str, include_context: Vec<String>) -> PyResult<String> {
    let mut config = nginx_lint_parser::parse_string(source)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    config.include_context = include_context;
    let output = build_parse_output(&config);
    serde_json::to_string(&output).map_err(|e| PyValueError::new_err(e.to_string()))
}

/// Apply fixes to a config, returning the result as a JSON string.
///
/// `fixes_json` is a list of the WIT `fix` records, whose field names match
/// `nginx_lint_common::Fix`, so they deserialize into the very struct the
/// linter applies — the point being that a plugin's fixes are tested
/// through the production applier rather than a reimplementation of it.
///
/// The returned object carries `content`, `applied` and `skipped_invalid`;
/// a caller that ignores the last one cannot tell a fix that did nothing
/// from a fix that was silently dropped.
#[pyfunction]
fn apply_fixes_json(content: &str, fixes_json: &str) -> PyResult<String> {
    let fixes: Vec<nginx_lint_common::Fix> =
        serde_json::from_str(fixes_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let refs: Vec<&nginx_lint_common::Fix> = fixes.iter().collect();
    let result = nginx_lint_common::apply_fixes_to_content_detailed(content, &refs);

    let out = serde_json::json!({
        "content": result.content,
        "applied": result.applied,
        "skipped_invalid": result.skipped_invalid,
    });
    serde_json::to_string(&out).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse_config_json, m)?)?;
    m.add_function(wrap_pyfunction!(apply_fixes_json, m)?)?;
    Ok(())
}
