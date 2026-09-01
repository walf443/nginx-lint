use crate::ast::{Argument, ArgumentValue, Comment, Config, ConfigItem, Directive};

// Generate guest-side bindings from the WIT file for the parser world
wit_bindgen::generate!({
    path: "wit/nginx-lint-plugin.wit",
    world: "parser",
    pub_export_macro: true,
    // So the same parse-output the component returns can also be handed out
    // as JSON by the `wasm-json` entry point below. The derives cost the
    // component nothing: with `wasm` alone nothing calls them and the build
    // is byte-identical.
    additional_derives: [serde::Serialize],
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

// ── A core-module JSON entry point (feature `wasm-json`) ────────────────
//
// The component above is only reachable from a component-model runtime, which
// Go does not have. These exports make the same parse output reachable from
// any plain wasm runtime: no imports, no canonical ABI, one JSON string out.
// The Go SDK's test helper runs this build under wazero, which is what lets a
// plugin's `go test` use the real parser instead of hand-built fixtures.
#[cfg(feature = "wasm-json")]
mod json {
    use super::{build_parse_output, pt};
    use serde::Serialize;

    /// The result shape, so a parse error is distinguishable from an output
    /// that happens to be empty.
    #[derive(Serialize)]
    struct JsonResult<'a> {
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<&'a pt::ParseOutput>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    }

    /// Reserve `len` bytes for the caller to write an argument into. The
    /// caller does not free it; the module is instantiated per parse and
    /// thrown away.
    #[unsafe(no_mangle)]
    pub extern "C" fn alloc(len: usize) -> *mut u8 {
        let mut buffer = Vec::with_capacity(len);
        let ptr = buffer.as_mut_ptr();
        std::mem::forget(buffer);
        ptr
    }

    /// Parse `source`, with `include_context` given as a JSON array of block
    /// names, and return the result as JSON.
    ///
    /// Returns the pointer and length packed into one u64 — `(ptr << 32) |
    /// len` — because a wasm export returns a single value and this avoids
    /// making the caller pass an out-parameter.
    #[unsafe(no_mangle)]
    pub extern "C" fn parse_config_json(
        source_ptr: *const u8,
        source_len: usize,
        context_ptr: *const u8,
        context_len: usize,
    ) -> u64 {
        let json = match (read(source_ptr, source_len), read(context_ptr, context_len)) {
            (Some(source), Some(context)) => parse(source, context),
            // Parsing an empty configuration instead would report a file the
            // CLI refuses to read as one with nothing in it, so a test
            // asserting no findings would pass for the wrong reason.
            _ => error("the argument is not valid UTF-8".to_string()),
        };
        let bytes = json.into_bytes();
        let (ptr, len) = (bytes.as_ptr() as u64, bytes.len() as u64);
        std::mem::forget(bytes);
        (ptr << 32) | len
    }

    fn read<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
        if ptr.is_null() || len == 0 {
            return Some("");
        }
        std::str::from_utf8(unsafe { std::slice::from_raw_parts(ptr, len) }).ok()
    }

    fn parse(source: &str, context: &str) -> String {
        let include_context: Vec<String> = if context.is_empty() {
            Vec::new()
        } else {
            match serde_json::from_str(context) {
                Ok(context) => context,
                Err(e) => return error(format!("invalid include context: {e}")),
            }
        };

        let mut config = match crate::parse_string(source) {
            Ok(config) => config,
            Err(e) => return error(e.to_string()),
        };
        config.include_context = include_context;

        let output = build_parse_output(&config);
        let result = JsonResult {
            output: Some(&output),
            error: None,
        };
        serde_json::to_string(&result).unwrap_or_else(|e| error(e.to_string()))
    }

    fn error(message: String) -> String {
        let result = JsonResult {
            output: None,
            error: Some(message),
        };
        // A serde_json failure on a struct of two options cannot happen, but
        // a panic here would surface to the caller as an unhelpful trap.
        serde_json::to_string(&result)
            .unwrap_or_else(|_| r#"{"error":"could not encode the error"}"#.to_string())
    }
}
