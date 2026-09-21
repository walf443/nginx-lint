//! The `config-api` interface as the host implements it: the config and
//! directive resources a component walks, and the snapshots it takes of
//! them, built from the AST.

use super::{
    ComponentStoreData, ConfigResource, DirectiveResource, bindings, config_api,
    resolve_block_items,
};
use crate::parser::ast::{self, Config};
use std::sync::Arc;
use wasmtime::component::{Resource, ResourceTable};

impl config_api::HostConfig for ComponentStoreData {
    fn snapshot(&mut self, self_: Resource<ConfigResource>) -> config_api::ConfigSnapshot {
        let config = self.get_config(&self_).clone();
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

    fn snapshot_filtered(
        &mut self,
        self_: Resource<ConfigResource>,
        names: Vec<String>,
    ) -> config_api::ConfigSnapshot {
        let config = self.get_config(&self_).clone();
        let mut all_items = Vec::new();
        let top_level_indices = config
            .items
            .iter()
            .filter_map(|item| flatten_item_to_wit_filtered(item, &names, &mut all_items))
            .collect();
        config_api::ConfigSnapshot {
            all_items,
            top_level_indices,
            include_context: config.include_context.clone(),
        }
    }

    fn all_directives_with_context(
        &mut self,
        self_: Resource<ConfigResource>,
    ) -> Vec<config_api::DirectiveContext> {
        let config = self.get_config(&self_).clone();
        let mut collected = Vec::new();
        collect_directive_paths_with_context(
            &config.items,
            &config.include_context,
            &mut Vec::new(),
            &mut collected,
        );

        let mut results = Vec::new();
        for (path, parent_stack, depth) in collected {
            let dir_resource = self.push_directive(config.clone(), path);
            results.push(config_api::DirectiveContext {
                directive: dir_resource,
                parent_stack,
                depth,
            });
        }
        results
    }

    fn all_directives(
        &mut self,
        self_: Resource<ConfigResource>,
    ) -> Vec<Resource<DirectiveResource>> {
        let config = self.get_config(&self_).clone();
        let mut collected = Vec::new();
        collect_directive_paths(&config.items, &mut Vec::new(), &mut collected);

        let mut results = Vec::new();
        for path in collected {
            let dir_resource = self.push_directive(config.clone(), path);
            results.push(dir_resource);
        }
        results
    }

    fn items(&mut self, self_: Resource<ConfigResource>) -> Vec<config_api::ConfigItem> {
        // Clone the Arc (cheap) to release the immutable borrow on self.table,
        // allowing convert_config_items_to_wit to borrow self.table mutably.
        let config = self.get_config(&self_).clone();
        convert_config_items_to_wit(&config, &[], &mut self.table)
    }

    fn include_context(&mut self, self_: Resource<ConfigResource>) -> Vec<String> {
        self.get_config(&self_).include_context.clone()
    }

    fn is_included_from(&mut self, self_: Resource<ConfigResource>, context: String) -> bool {
        let ctx = &self.get_config(&self_).include_context;
        ctx.iter().any(|c| c == &context)
    }

    fn is_included_from_http(&mut self, self_: Resource<ConfigResource>) -> bool {
        let ctx = &self.get_config(&self_).include_context;
        ctx.iter().any(|c| c == "http")
    }

    fn is_included_from_http_server(&mut self, self_: Resource<ConfigResource>) -> bool {
        let ctx = &self.get_config(&self_).include_context;
        if let (Some(http_pos), Some(server_pos)) = (
            ctx.iter().position(|c| c == "http"),
            ctx.iter().position(|c| c == "server"),
        ) {
            http_pos < server_pos
        } else {
            false
        }
    }

    fn is_included_from_http_location(&mut self, self_: Resource<ConfigResource>) -> bool {
        let ctx = &self.get_config(&self_).include_context;
        if let (Some(http_pos), Some(location_pos)) = (
            ctx.iter().position(|c| c == "http"),
            ctx.iter().position(|c| c == "location"),
        ) {
            http_pos < location_pos
        } else {
            false
        }
    }

    fn is_included_from_stream(&mut self, self_: Resource<ConfigResource>) -> bool {
        let ctx = &self.get_config(&self_).include_context;
        ctx.iter().any(|c| c == "stream")
    }

    fn immediate_parent_context(&mut self, self_: Resource<ConfigResource>) -> Option<String> {
        self.get_config(&self_).include_context.last().cloned()
    }

    fn drop(&mut self, rep: Resource<ConfigResource>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep)?;
        Ok(())
    }
}

impl config_api::HostDirective for ComponentStoreData {
    fn data(&mut self, self_: Resource<DirectiveResource>) -> config_api::DirectiveData {
        make_directive_data(self.get_directive(&self_))
    }

    fn name(&mut self, self_: Resource<DirectiveResource>) -> String {
        self.get_directive(&self_).name.clone()
    }

    fn is(&mut self, self_: Resource<DirectiveResource>, name: String) -> bool {
        self.get_directive(&self_).name == name
    }

    fn first_arg(&mut self, self_: Resource<DirectiveResource>) -> Option<String> {
        self.get_directive(&self_)
            .first_arg()
            .map(|s| s.to_string())
    }

    fn first_arg_is(&mut self, self_: Resource<DirectiveResource>, value: String) -> bool {
        self.get_directive(&self_).first_arg_is(&value)
    }

    fn arg_at(&mut self, self_: Resource<DirectiveResource>, index: u32) -> Option<String> {
        self.get_directive(&self_)
            .args
            .get(index as usize)
            .map(|a| a.as_str().to_string())
    }

    fn last_arg(&mut self, self_: Resource<DirectiveResource>) -> Option<String> {
        self.get_directive(&self_)
            .args
            .last()
            .map(|a| a.as_str().to_string())
    }

    fn has_arg(&mut self, self_: Resource<DirectiveResource>, value: String) -> bool {
        self.get_directive(&self_)
            .args
            .iter()
            .any(|a| a.as_str() == value)
    }

    fn arg_count(&mut self, self_: Resource<DirectiveResource>) -> u32 {
        self.get_directive(&self_).args.len() as u32
    }

    fn args(&mut self, self_: Resource<DirectiveResource>) -> Vec<config_api::ArgumentInfo> {
        self.get_directive(&self_)
            .args
            .iter()
            .map(convert_argument_to_wit)
            .collect()
    }

    fn line(&mut self, self_: Resource<DirectiveResource>) -> u32 {
        self.get_directive(&self_).span.start.line as u32
    }

    fn column(&mut self, self_: Resource<DirectiveResource>) -> u32 {
        self.get_directive(&self_).span.start.column as u32
    }

    fn start_offset(&mut self, self_: Resource<DirectiveResource>) -> u32 {
        self.get_directive(&self_).span.start.offset as u32
    }

    fn end_offset(&mut self, self_: Resource<DirectiveResource>) -> u32 {
        self.get_directive(&self_).span.end.offset as u32
    }

    fn leading_whitespace(&mut self, self_: Resource<DirectiveResource>) -> String {
        self.get_directive(&self_).leading_whitespace.clone()
    }

    fn trailing_whitespace(&mut self, self_: Resource<DirectiveResource>) -> String {
        self.get_directive(&self_).trailing_whitespace.clone()
    }

    fn space_before_terminator(&mut self, self_: Resource<DirectiveResource>) -> String {
        self.get_directive(&self_).space_before_terminator.clone()
    }

    fn has_block(&mut self, self_: Resource<DirectiveResource>) -> bool {
        self.get_directive(&self_).block.is_some()
    }

    fn block_items(&mut self, self_: Resource<DirectiveResource>) -> Vec<config_api::ConfigItem> {
        // Clone the Arc and path (cheap) to release the immutable borrow on
        // self.table, allowing convert_config_items_to_wit to borrow it mutably.
        let (config, path) = {
            let resource = self
                .table
                .get(&self_)
                .expect("invalid directive resource handle");
            (resource.config.clone(), resource.path.clone())
        };
        convert_config_items_to_wit(&config, &path, &mut self.table)
    }

    fn block_is_raw(&mut self, self_: Resource<DirectiveResource>) -> bool {
        self.get_directive(&self_)
            .block
            .as_ref()
            .is_some_and(|b| b.is_raw())
    }

    fn replace_with(
        &mut self,
        self_: Resource<DirectiveResource>,
        new_text: String,
    ) -> config_api::Fix {
        let d = self.get_directive(&self_);
        let start = d.span.start.offset - d.leading_whitespace.len();
        let end = d.span.end.offset;
        let fixed = format!("{}{}", d.leading_whitespace, new_text);
        make_range_fix(start, end, fixed)
    }

    fn delete_line_fix(&mut self, self_: Resource<DirectiveResource>) -> config_api::Fix {
        let line = self.get_directive(&self_).span.start.line;
        config_api::Fix {
            line: line as u32,
            old_text: None,
            new_text: String::new(),
            delete_line: true,
            insert_after: false,
            start_offset: None,
            end_offset: None,
        }
    }

    fn insert_after(
        &mut self,
        self_: Resource<DirectiveResource>,
        new_text: String,
    ) -> config_api::Fix {
        let d = self.get_directive(&self_);
        let indent = " ".repeat(d.span.start.column.saturating_sub(1));
        let fix_text = format!("\n{}{}", indent, new_text);
        let offset = d.span.end.offset;
        make_range_fix(offset, offset, fix_text)
    }

    fn insert_before(
        &mut self,
        self_: Resource<DirectiveResource>,
        new_text: String,
    ) -> config_api::Fix {
        let d = self.get_directive(&self_);
        let indent = " ".repeat(d.span.start.column.saturating_sub(1));
        let fix_text = format!("{}{}\n", indent, new_text);
        let offset = d
            .span
            .start
            .offset
            .saturating_sub(d.span.start.column.saturating_sub(1));
        make_range_fix(offset, offset, fix_text)
    }

    fn insert_after_many(
        &mut self,
        self_: Resource<DirectiveResource>,
        lines: Vec<String>,
    ) -> config_api::Fix {
        let d = self.get_directive(&self_);
        let indent = " ".repeat(d.span.start.column.saturating_sub(1));
        let fix_text: String = lines
            .iter()
            .map(|line| format!("\n{}{}", indent, line))
            .collect();
        let offset = d.span.end.offset;
        make_range_fix(offset, offset, fix_text)
    }

    fn insert_before_many(
        &mut self,
        self_: Resource<DirectiveResource>,
        lines: Vec<String>,
    ) -> config_api::Fix {
        let d = self.get_directive(&self_);
        let indent = " ".repeat(d.span.start.column.saturating_sub(1));
        let fix_text: String = lines
            .iter()
            .map(|line| format!("{}{}\n", indent, line))
            .collect();
        let offset = d
            .span
            .start
            .offset
            .saturating_sub(d.span.start.column.saturating_sub(1));
        make_range_fix(offset, offset, fix_text)
    }

    fn drop(&mut self, rep: Resource<DirectiveResource>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep)?;
        Ok(())
    }
}

// === Helper functions ===

/// Create a range-based WIT fix.
fn make_range_fix(start: usize, end: usize, new_text: String) -> config_api::Fix {
    config_api::Fix {
        line: 0,
        old_text: None,
        new_text,
        delete_line: false,
        insert_after: false,
        start_offset: Some(start as u32),
        end_offset: Some(end as u32),
    }
}

/// Recursively collect directive paths with parent context (depth-first).
fn collect_directive_paths_with_context(
    items: &[ast::ConfigItem],
    parent_stack: &[String],
    path_prefix: &mut Vec<usize>,
    results: &mut Vec<(Vec<usize>, Vec<String>, u32)>,
) {
    for (index, item) in items.iter().enumerate() {
        if let ast::ConfigItem::Directive(directive) = item {
            path_prefix.push(index);
            results.push((
                path_prefix.clone(),
                parent_stack.to_vec(),
                parent_stack.len() as u32,
            ));
            if let Some(block) = &directive.block {
                let mut child_stack = parent_stack.to_vec();
                child_stack.push(directive.name.clone());
                collect_directive_paths_with_context(
                    &block.items,
                    &child_stack,
                    path_prefix,
                    results,
                );
            }
            path_prefix.pop();
        }
    }
}

/// Recursively collect all directive paths (depth-first).
fn collect_directive_paths(
    items: &[ast::ConfigItem],
    path_prefix: &mut Vec<usize>,
    results: &mut Vec<Vec<usize>>,
) {
    for (index, item) in items.iter().enumerate() {
        if let ast::ConfigItem::Directive(directive) = item {
            path_prefix.push(index);
            results.push(path_prefix.clone());
            if let Some(block) = &directive.block {
                collect_directive_paths(&block.items, path_prefix, results);
            }
            path_prefix.pop();
        }
    }
}

/// Convert the ConfigItems at `base_path` (see [`resolve_block_items`]) to
/// WIT ConfigItems, creating path-based directive resources for directives.
fn convert_config_items_to_wit(
    config: &Arc<Config>,
    base_path: &[usize],
    table: &mut ResourceTable,
) -> Vec<config_api::ConfigItem> {
    let items = resolve_block_items(config, base_path);
    let mut results = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match item {
            ast::ConfigItem::Directive(_) => {
                let mut path = base_path.to_vec();
                path.push(index);
                let dir_resource = table
                    .push(DirectiveResource {
                        config: config.clone(),
                        path,
                    })
                    .expect("resource table full: too many directive handles allocated");
                results.push(config_api::ConfigItem::DirectiveItem(dir_resource));
            }
            ast::ConfigItem::Comment(comment) => {
                results.push(config_api::ConfigItem::CommentItem(make_comment_info(
                    comment,
                )));
            }
            ast::ConfigItem::BlankLine(blank) => {
                results.push(config_api::ConfigItem::BlankLineItem(make_blank_line_info(
                    blank,
                )));
            }
        }
    }
    results
}

/// Build the WIT DirectiveData record for a directive.
fn make_directive_data(dir: &ast::Directive) -> config_api::DirectiveData {
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
fn make_comment_info(comment: &ast::Comment) -> config_api::CommentInfo {
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
fn make_blank_line_info(blank: &ast::BlankLine) -> config_api::BlankLineInfo {
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
fn convert_argument_to_wit(arg: &ast::Argument) -> config_api::ArgumentInfo {
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
    use wasmtime::StoreLimitsBuilder;

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
        let config = crate::parser::parse_string(source).unwrap();
        let (mut data, resource) =
            setup_store_with_config(vec!["http".to_string()], config.items.clone());

        let snapshot = config_api::HostConfig::snapshot(&mut data, resource);

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

        let mut original = Config {
            items: config.items,
            include_context: vec!["http".to_string()],
        };
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

    // === Host trait method tests ===

    /// Create a ComponentStoreData with a config resource for testing host methods.
    fn setup_store_with_config(
        include_context: Vec<String>,
        items: Vec<ast::ConfigItem>,
    ) -> (ComponentStoreData, Resource<ConfigResource>) {
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
            wasi: None,
        };
        let config = Arc::new(Config {
            items,
            include_context,
        });
        let resource = data
            .table
            .push(ConfigResource { config })
            .expect("push config");
        (data, resource)
    }

    /// Create a directive with the given name and span.
    fn make_directive(
        name: &str,
        line: usize,
        column: usize,
        start_offset: usize,
        end_offset: usize,
    ) -> ast::Directive {
        ast::Directive {
            name: name.to_string(),
            name_span: ast::Span::new(
                ast::Position::new(line, column, start_offset),
                ast::Position::new(line, column + name.len(), start_offset + name.len()),
            ),
            args: vec![],
            block: None,
            span: ast::Span::new(
                ast::Position::new(line, column, start_offset),
                ast::Position::new(line, column, end_offset),
            ),
            trailing_comment: None,
            leading_whitespace: " ".repeat(column.saturating_sub(1)),
            space_before_terminator: String::new(),
            trailing_whitespace: "\n".to_string(),
        }
    }

    /// Create a second handle aliasing the same table entry, for calling
    /// host trait methods that take the `Resource` by value while the
    /// original handle stays owned elsewhere (e.g. by a DirectiveContext).
    /// Tests must not drop/delete these aliases: deleting both the alias
    /// and the original would double-delete the table entry.
    fn alias_directive_handle(
        resource: &Resource<DirectiveResource>,
    ) -> Resource<DirectiveResource> {
        Resource::new_own(resource.rep())
    }

    /// Wrap a directive in a single-item Config and push a path-based
    /// directive resource for it.
    fn push_test_directive(
        data: &mut ComponentStoreData,
        directive: ast::Directive,
    ) -> Resource<DirectiveResource> {
        let config = Arc::new(Config {
            items: vec![ast::ConfigItem::Directive(Box::new(directive))],
            include_context: vec![],
        });
        data.table
            .push(DirectiveResource {
                config,
                path: vec![0],
            })
            .unwrap()
    }

    #[test]
    fn test_is_included_from_http_server_correct_order() {
        let (mut data, resource) =
            setup_store_with_config(vec!["http".to_string(), "server".to_string()], vec![]);
        assert!(config_api::HostConfig::is_included_from_http_server(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_server_reversed_order() {
        let (mut data, resource) =
            setup_store_with_config(vec!["server".to_string(), "http".to_string()], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_server(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_server_missing_server() {
        let (mut data, resource) = setup_store_with_config(vec!["http".to_string()], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_server(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_server_missing_http() {
        let (mut data, resource) = setup_store_with_config(vec!["server".to_string()], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_server(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_server_empty_context() {
        let (mut data, resource) = setup_store_with_config(vec![], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_server(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_location_correct_order() {
        let (mut data, resource) =
            setup_store_with_config(vec!["http".to_string(), "location".to_string()], vec![]);
        assert!(config_api::HostConfig::is_included_from_http_location(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_location_reversed_order() {
        let (mut data, resource) =
            setup_store_with_config(vec!["location".to_string(), "http".to_string()], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_location(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_location_missing_location() {
        let (mut data, resource) = setup_store_with_config(vec!["http".to_string()], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_location(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_location_empty_context() {
        let (mut data, resource) = setup_store_with_config(vec![], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_location(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_location_with_server_in_between() {
        let (mut data, resource) = setup_store_with_config(
            vec![
                "http".to_string(),
                "server".to_string(),
                "location".to_string(),
            ],
            vec![],
        );
        assert!(config_api::HostConfig::is_included_from_http_location(
            &mut data, resource
        ));
    }

    #[test]
    fn test_insert_before_column_1() {
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
            wasi: None,
        };
        let dir = make_directive("listen", 2, 1, 10, 20);
        let resource = push_test_directive(&mut data, dir);

        let fix =
            config_api::HostDirective::insert_before(&mut data, resource, "new_line;".to_string());
        // Column 1: offset should be offset - 0 = 10
        assert_eq!(fix.start_offset, Some(10));
        assert_eq!(fix.end_offset, Some(10));
        assert!(fix.new_text.contains("new_line;"));
    }

    #[test]
    fn test_insert_before_indented() {
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
            wasi: None,
        };
        let dir = make_directive("listen", 2, 5, 15, 25);
        let resource = push_test_directive(&mut data, dir);

        let fix =
            config_api::HostDirective::insert_before(&mut data, resource, "new_line;".to_string());
        // Column 5: offset should be 15 - 4 = 11
        assert_eq!(fix.start_offset, Some(11));
        assert_eq!(fix.end_offset, Some(11));
        // Should include indentation
        assert!(fix.new_text.starts_with("    "));
    }

    #[test]
    fn test_insert_before_many_multiple_lines() {
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
            wasi: None,
        };
        let dir = make_directive("listen", 2, 5, 15, 25);
        let resource = push_test_directive(&mut data, dir);

        let fix = config_api::HostDirective::insert_before_many(
            &mut data,
            resource,
            vec!["line1;".to_string(), "line2;".to_string()],
        );
        assert_eq!(fix.start_offset, Some(11));
        assert!(fix.new_text.contains("line1;"));
        assert!(fix.new_text.contains("line2;"));
    }

    #[test]
    fn test_items_with_mixed_content() {
        let items = vec![
            ast::ConfigItem::Directive(Box::new(make_directive("http", 1, 1, 0, 10))),
            ast::ConfigItem::Comment(ast::Comment {
                text: "# comment".to_string(),
                span: ast::Span::new(ast::Position::new(2, 1, 11), ast::Position::new(2, 10, 20)),
                leading_whitespace: String::new(),
                trailing_whitespace: "\n".to_string(),
            }),
            ast::ConfigItem::BlankLine(ast::BlankLine {
                span: ast::Span::new(ast::Position::new(3, 1, 21), ast::Position::new(3, 1, 22)),
                content: "\n".to_string(),
            }),
        ];
        let (mut data, resource) = setup_store_with_config(vec![], items);
        let wit_items = config_api::HostConfig::items(&mut data, resource);
        assert_eq!(wit_items.len(), 3);
        // Check types: directive, comment, blank line
        assert!(matches!(
            wit_items[0],
            config_api::ConfigItem::DirectiveItem(_)
        ));
        assert!(matches!(
            wit_items[1],
            config_api::ConfigItem::CommentItem(_)
        ));
        assert!(matches!(
            wit_items[2],
            config_api::ConfigItem::BlankLineItem(_)
        ));
    }

    #[test]
    fn test_block_items_no_block() {
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
            wasi: None,
        };
        let dir = make_directive("listen", 1, 1, 0, 10);
        let resource = push_test_directive(&mut data, dir);

        let items = config_api::HostDirective::block_items(&mut data, resource);
        assert!(items.is_empty());
    }

    #[test]
    fn test_block_items_with_block() {
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
            wasi: None,
        };
        let mut dir = make_directive("http", 1, 1, 0, 30);
        dir.block = Some(ast::Block {
            items: vec![ast::ConfigItem::Directive(Box::new(make_directive(
                "server", 2, 5, 10, 25,
            )))],
            span: ast::Span::new(ast::Position::new(1, 6, 5), ast::Position::new(3, 1, 30)),
            raw_content: None,
            closing_brace_leading_whitespace: String::new(),
            trailing_whitespace: "\n".to_string(),
        });
        let resource = push_test_directive(&mut data, dir);

        let items = config_api::HostDirective::block_items(&mut data, resource);
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], config_api::ConfigItem::DirectiveItem(_)));
    }

    /// Create a directive with a block containing the given items.
    fn make_block_directive(
        name: &str,
        line: usize,
        items: Vec<ast::ConfigItem>,
    ) -> ast::Directive {
        let mut dir = make_directive(name, line, 1, line * 100, line * 100 + 50);
        dir.block = Some(ast::Block {
            items,
            span: ast::Span::new(
                ast::Position::new(line, 10, line * 100 + 9),
                ast::Position::new(line + 2, 1, line * 100 + 49),
            ),
            raw_content: None,
            closing_brace_leading_whitespace: String::new(),
            trailing_whitespace: "\n".to_string(),
        });
        dir
    }

    #[test]
    fn test_nested_directive_paths_resolve() {
        // http { server { listen; } }  plus a comment before `server` so that
        // directive indices differ from "directive number"
        let listen = make_directive("listen", 3, 9, 320, 330);
        let comment = ast::ConfigItem::Comment(ast::Comment {
            text: "# c".to_string(),
            span: ast::Span::new(ast::Position::new(2, 5, 210), ast::Position::new(2, 8, 213)),
            leading_whitespace: String::new(),
            trailing_whitespace: "\n".to_string(),
        });
        let server = make_block_directive(
            "server",
            2,
            vec![ast::ConfigItem::Directive(Box::new(listen))],
        );
        let http = make_block_directive(
            "http",
            1,
            vec![comment, ast::ConfigItem::Directive(Box::new(server))],
        );
        let (mut data, config_resource) =
            setup_store_with_config(vec![], vec![ast::ConfigItem::Directive(Box::new(http))]);

        let contexts =
            config_api::HostConfig::all_directives_with_context(&mut data, config_resource);
        assert_eq!(contexts.len(), 3);

        let names: Vec<String> = contexts
            .iter()
            .map(|ctx| {
                config_api::HostDirective::name(&mut data, alias_directive_handle(&ctx.directive))
            })
            .collect();
        assert_eq!(names, vec!["http", "server", "listen"]);

        // The deepest directive (listen, behind a comment sibling) resolves
        // with correct location data through the path
        let listen_ctx = &contexts[2];
        assert_eq!(listen_ctx.parent_stack, vec!["http", "server"]);
        assert_eq!(listen_ctx.depth, 2);
        let data_wit = config_api::HostDirective::data(
            &mut data,
            alias_directive_handle(&listen_ctx.directive),
        );
        assert_eq!(data_wit.name, "listen");
        assert_eq!(data_wit.line, 3);
        assert_eq!(data_wit.start_offset, 320);
    }

    #[test]
    fn test_nested_block_items_create_resolvable_handles() {
        // block_items on `http` must return a handle for `server` that
        // resolves through the extended path
        let listen = make_directive("listen", 3, 9, 320, 330);
        let server = make_block_directive(
            "server",
            2,
            vec![ast::ConfigItem::Directive(Box::new(listen))],
        );
        let http = make_block_directive(
            "http",
            1,
            vec![ast::ConfigItem::Directive(Box::new(server))],
        );
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
            wasi: None,
        };
        let http_resource = push_test_directive(&mut data, http);

        let items = config_api::HostDirective::block_items(&mut data, http_resource);
        assert_eq!(items.len(), 1);
        let config_api::ConfigItem::DirectiveItem(server_resource) = &items[0] else {
            panic!("expected directive item");
        };
        assert_eq!(
            config_api::HostDirective::name(&mut data, alias_directive_handle(server_resource)),
            "server"
        );
    }
}
