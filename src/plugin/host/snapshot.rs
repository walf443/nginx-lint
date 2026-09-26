//! A config as a component takes a snapshot of it: the AST flattened into
//! one array of items that index each other, which the guest SDKs rebuild
//! into their own tree with no host calls per node. Pruned to a set of
//! directive names on request, which keeps the ancestors a rule needs for
//! context and drops everything else.

use super::{bindings, config_api};
use nginx_lint_common::parser::ast::{self, Config};

/// The whole config, flattened.
pub(super) fn snapshot(config: &Config) -> config_api::ConfigSnapshot {
    let mut all_items = Vec::new();
    let top_level_indices = config
        .items
        .iter()
        .map(|item| flatten_item_to_wit(item, &mut all_items))
        .collect();
    config_api::ConfigSnapshot {
        all_items,
        top_level_indices,
        include_context: config.include_context.clone(),
    }
}

/// The config pruned to the directives named in `names` and the blocks
/// that lead to them (see [`flatten_item_to_wit_filtered`]), flattened.
pub(super) fn snapshot_filtered(config: &Config, names: &[String]) -> config_api::ConfigSnapshot {
    let mut all_items = Vec::new();
    let top_level_indices = config
        .items
        .iter()
        .filter_map(|item| flatten_item_to_wit_filtered(item, names, &mut all_items))
        .collect();
    config_api::ConfigSnapshot {
        all_items,
        top_level_indices,
        include_context: config.include_context.clone(),
    }
}

/// Build the WIT DirectiveData record for a directive.
pub(super) fn make_directive_data(dir: &ast::Directive) -> config_api::DirectiveData {
    config_api::DirectiveData {
        name: dir.name.clone(),
        args: dir.args.iter().map(convert_argument_to_wit).collect(),
        line: dir.span.start.line as u32,
        column: dir.span.start.column as u32,
        start_offset: dir.span.start.offset as u32,
        end_offset: dir.span.end.offset as u32,
        end_line: dir.span.end.line as u32,
        end_column: dir.span.end.column as u32,
        leading_whitespace: dir.leading_whitespace.clone(),
        trailing_whitespace: dir.trailing_whitespace.clone(),
        space_before_terminator: dir.space_before_terminator.clone(),
        has_block: dir.block.is_some(),
        block_is_raw: dir.block.as_ref().is_some_and(|b| b.raw_content.is_some()),
        block_raw_content: dir.block.as_ref().and_then(|b| b.raw_content.clone()),
        closing_brace_leading_whitespace: dir
            .block
            .as_ref()
            .map(|b| b.closing_brace_leading_whitespace.clone()),
        block_trailing_whitespace: dir.block.as_ref().map(|b| b.trailing_whitespace.clone()),
        trailing_comment_text: dir.trailing_comment.as_ref().map(|c| c.text.clone()),
        name_end_column: dir.name_span.end.column as u32,
        name_end_offset: dir.name_span.end.offset as u32,
        block_start_line: dir.block.as_ref().map(|b| b.span.start.line as u32),
        block_start_column: dir.block.as_ref().map(|b| b.span.start.column as u32),
        block_start_offset: dir.block.as_ref().map(|b| b.span.start.offset as u32),
    }
}

/// Build the WIT CommentInfo record for a comment.
pub(super) fn make_comment_info(comment: &ast::Comment) -> config_api::CommentInfo {
    config_api::CommentInfo {
        text: comment.text.clone(),
        line: comment.span.start.line as u32,
        column: comment.span.start.column as u32,
        leading_whitespace: comment.leading_whitespace.clone(),
        trailing_whitespace: comment.trailing_whitespace.clone(),
        start_offset: comment.span.start.offset as u32,
        end_offset: comment.span.end.offset as u32,
    }
}

/// Build the WIT BlankLineInfo record for a blank line.
pub(super) fn make_blank_line_info(blank: &ast::BlankLine) -> config_api::BlankLineInfo {
    config_api::BlankLineInfo {
        line: blank.span.start.line as u32,
        content: blank.content.clone(),
        start_offset: blank.span.start.offset as u32,
    }
}

/// Recursively flatten a config item into the snapshot's DFS-ordered array,
/// returning its index. Directive items record their block children as
/// indices into the same array (see the `parser-types` WIT interface, which
/// uses the same layout).
fn flatten_item_to_wit(item: &ast::ConfigItem, all_items: &mut Vec<config_api::FlatItem>) -> u32 {
    use bindings::nginx_lint::plugin::parser_types::ConfigItemValue;

    match item {
        ast::ConfigItem::Directive(directive) => {
            let index = all_items.len() as u32;
            all_items.push(config_api::FlatItem {
                value: ConfigItemValue::DirectiveItem(make_directive_data(directive)),
                child_indices: Vec::new(),
            });
            let child_indices = directive
                .block
                .as_ref()
                .map(|block| {
                    block
                        .items
                        .iter()
                        .map(|child| flatten_item_to_wit(child, all_items))
                        .collect()
                })
                .unwrap_or_default();
            all_items[index as usize].child_indices = child_indices;
            index
        }
        ast::ConfigItem::Comment(comment) => {
            let index = all_items.len() as u32;
            all_items.push(config_api::FlatItem {
                value: ConfigItemValue::CommentItem(make_comment_info(comment)),
                child_indices: Vec::new(),
            });
            index
        }
        ast::ConfigItem::BlankLine(blank) => {
            let index = all_items.len() as u32;
            all_items.push(config_api::FlatItem {
                value: ConfigItemValue::BlankLineItem(make_blank_line_info(blank)),
                child_indices: Vec::new(),
            });
            index
        }
    }
}

/// Recursively flatten a config item for [`HostConfig::snapshot_filtered`],
/// keeping only directives whose name is in `names` and the ancestor
/// directives needed to reach them (so guest-side `is_inside` context stays
/// correct). Returns `None` when this item is neither a match nor an
/// ancestor of one, so the caller can drop it from the parent's child list
/// entirely. Comments and blank lines are never kept: no builtin rule reads
/// them through the filtered path, and they cannot be ancestors of a match.
fn flatten_item_to_wit_filtered(
    item: &ast::ConfigItem,
    names: &[String],
    all_items: &mut Vec<config_api::FlatItem>,
) -> Option<u32> {
    use bindings::nginx_lint::plugin::parser_types::ConfigItemValue;

    let ast::ConfigItem::Directive(directive) = item else {
        return None;
    };

    let child_indices: Vec<u32> = directive
        .block
        .as_ref()
        .map(|block| {
            block
                .items
                .iter()
                .filter_map(|child| flatten_item_to_wit_filtered(child, names, all_items))
                .collect()
        })
        .unwrap_or_default();

    let is_match = names.iter().any(|name| name == &directive.name);
    if !is_match && child_indices.is_empty() {
        return None;
    }

    let index = all_items.len() as u32;
    all_items.push(config_api::FlatItem {
        value: ConfigItemValue::DirectiveItem(make_directive_data(directive)),
        child_indices,
    });
    Some(index)
}

/// Convert a parser Argument to WIT ArgumentInfo.
pub(super) fn convert_argument_to_wit(arg: &ast::Argument) -> config_api::ArgumentInfo {
    config_api::ArgumentInfo {
        value: arg.as_str().to_string(),
        raw: arg.raw.clone(),
        arg_type: match &arg.value {
            ast::ArgumentValue::Literal(_) => config_api::ArgumentType::Literal,
            ast::ArgumentValue::QuotedString(_) => config_api::ArgumentType::QuotedString,
            ast::ArgumentValue::SingleQuotedString(_) => {
                config_api::ArgumentType::SingleQuotedString
            }
            ast::ArgumentValue::Variable(_) => config_api::ArgumentType::Variable,
        },
        line: arg.span.start.line as u32,
        column: arg.span.start.column as u32,
        start_offset: arg.span.start.offset as u32,
        end_offset: arg.span.end.offset as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The snapshot flattening must be lossless: rebuilding the AST from the
    /// flat array (the way the guest SDK does) must reproduce the original
    /// config exactly. Guards against field omissions in make_directive_data
    /// and ordering/index bugs in flatten_item_to_wit, which would otherwise
    /// silently drift from what plugins observe.
    #[test]
    fn test_snapshot_round_trip_is_lossless() {
        let source = r#"# leading comment
user nginx;

http {
    gzip on; # trailing comment
    upstream backend {
        server 127.0.0.1:8080;
    }

    server {
        listen 80;
        server_name "example.com" 'alt.example.com';
        set $custom_var value;
        location / {
            proxy_pass http://backend;
            proxy_set_header Host $host;
        }
    }
}
"#;
        let config = Config {
            include_context: vec!["http".to_string()],
            ..nginx_lint_common::parser::parse_string(source).unwrap()
        };

        let snapshot = snapshot(&config);

        let mut slots: Vec<Option<config_api::FlatItem>> =
            snapshot.all_items.into_iter().map(Some).collect();
        let rebuilt = Config {
            items: snapshot
                .top_level_indices
                .iter()
                .map(|&index| rebuild_item(&mut slots, index))
                .collect(),
            include_context: snapshot.include_context,
        };
        assert!(
            slots.iter().all(Option::is_none),
            "snapshot contains items unreachable from the index tree"
        );

        let mut original = config;
        normalize_known_lossy_fields(&mut original.items);
        assert_eq!(
            serde_json::to_value(&original).unwrap(),
            serde_json::to_value(&rebuilt).unwrap(),
            "snapshot round trip must reproduce the original AST exactly \
             (modulo the known-lossy fields normalized above)"
        );
    }

    /// The WIT boundary intentionally does not carry two pieces of data, and
    /// has not since the original per-directive reconstruction path; plugins
    /// have never observed them. They cannot be "fixed" either: adding
    /// fields to the existing WIT records would be a breaking change that
    /// fails instantiation of plugins built against the old SDK (see the
    /// API_VERSION note in plugin/mod.rs). Normalize the original AST to
    /// the guest-visible form so the round-trip comparison checks
    /// everything else exactly:
    /// - a trailing comment transfers only its text (span and whitespace are
    ///   zeroed guest-side)
    /// - a blank line's span end is recomputed from its content, which
    ///   excludes the newline the parser includes
    fn normalize_known_lossy_fields(items: &mut [ast::ConfigItem]) {
        for item in items {
            match item {
                ast::ConfigItem::Directive(directive) => {
                    if let Some(comment) = &mut directive.trailing_comment {
                        let line = directive.span.start.line;
                        comment.span = ast::Span::new(
                            ast::Position::new(line, 0, 0),
                            ast::Position::new(line, 0, 0),
                        );
                        comment.leading_whitespace = String::new();
                        comment.trailing_whitespace = String::new();
                    }
                    if let Some(block) = &mut directive.block {
                        normalize_known_lossy_fields(&mut block.items);
                    }
                }
                ast::ConfigItem::BlankLine(blank) => {
                    let start = blank.span.start;
                    blank.span = ast::Span::new(
                        start,
                        ast::Position::new(
                            start.line,
                            1 + blank.content.chars().count(),
                            start.offset + blank.content.len(),
                        ),
                    );
                }
                ast::ConfigItem::Comment(_) => {}
            }
        }
    }

    /// Test-local mirror of the guest SDK's build_item: rebuild the AST item
    /// at `index` from the snapshot's flat array.
    fn rebuild_item(slots: &mut [Option<config_api::FlatItem>], index: u32) -> ast::ConfigItem {
        use bindings::nginx_lint::plugin::parser_types::ConfigItemValue;

        let item = slots[index as usize].take().expect("index visited twice");
        let children: Vec<ast::ConfigItem> = item
            .child_indices
            .iter()
            .map(|&child| rebuild_item(slots, child))
            .collect();

        match item.value {
            ConfigItemValue::DirectiveItem(d) => {
                let line = d.line as usize;
                let column = d.column as usize;
                let start_offset = d.start_offset as usize;
                let block = d.has_block.then(|| ast::Block {
                    items: children,
                    span: ast::Span::new(
                        ast::Position::new(
                            d.block_start_line.unwrap_or(d.line) as usize,
                            d.block_start_column.unwrap_or(d.column) as usize,
                            d.block_start_offset.unwrap_or(d.start_offset) as usize,
                        ),
                        ast::Position::new(
                            d.end_line as usize,
                            d.end_column as usize,
                            d.end_offset as usize,
                        ),
                    ),
                    raw_content: d.block_raw_content.clone(),
                    closing_brace_leading_whitespace: d
                        .closing_brace_leading_whitespace
                        .clone()
                        .unwrap_or_default(),
                    trailing_whitespace: d.block_trailing_whitespace.clone().unwrap_or_default(),
                });
                ast::ConfigItem::Directive(Box::new(ast::Directive {
                    name_span: ast::Span::new(
                        ast::Position::new(line, column, start_offset),
                        ast::Position::new(
                            line,
                            d.name_end_column as usize,
                            d.name_end_offset as usize,
                        ),
                    ),
                    args: d.args.into_iter().map(rebuild_argument).collect(),
                    block,
                    span: ast::Span::new(
                        ast::Position::new(line, column, start_offset),
                        ast::Position::new(
                            d.end_line as usize,
                            d.end_column as usize,
                            d.end_offset as usize,
                        ),
                    ),
                    trailing_comment: d.trailing_comment_text.map(|text| ast::Comment {
                        span: ast::Span::new(
                            ast::Position::new(line, 0, 0),
                            ast::Position::new(line, 0, 0),
                        ),
                        leading_whitespace: String::new(),
                        trailing_whitespace: String::new(),
                        text,
                    }),
                    name: d.name,
                    leading_whitespace: d.leading_whitespace,
                    space_before_terminator: d.space_before_terminator,
                    trailing_whitespace: d.trailing_whitespace,
                }))
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

    fn rebuild_argument(a: config_api::ArgumentInfo) -> ast::Argument {
        let value = match a.arg_type {
            config_api::ArgumentType::Literal => ast::ArgumentValue::Literal(a.value),
            config_api::ArgumentType::QuotedString => ast::ArgumentValue::QuotedString(a.value),
            config_api::ArgumentType::SingleQuotedString => {
                ast::ArgumentValue::SingleQuotedString(a.value)
            }
            config_api::ArgumentType::Variable => ast::ArgumentValue::Variable(a.value),
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
}
