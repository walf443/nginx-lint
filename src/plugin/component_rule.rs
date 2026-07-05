//! Component model based lint rule implementation
//!
//! This module implements the LintRule trait for WIT component model plugins.
//! Plugins communicate with the host via WIT resource handles for config access,
//! eliminating the need for JSON serialization.

use super::error::PluginError;
use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::{self, Config};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wasmtime::component::{Resource, ResourceTable};
use wasmtime::{Engine, Store, StoreLimits, StoreLimitsBuilder, Trap};

/// Plugin spec returned by the plugin
#[derive(Debug, Clone)]
pub struct PluginSpec {
    pub name: String,
    pub category: String,
    pub description: String,
    #[allow(dead_code)]
    pub api_version: String,
    #[allow(dead_code)]
    pub severity: Option<String>,
    pub why: Option<String>,
    pub bad_example: Option<String>,
    pub good_example: Option<String>,
    pub references: Option<Vec<String>>,
    pub min_nginx_version: Option<String>,
    pub max_nginx_version: Option<String>,
}

/// Host-side config resource, holding the parsed Config.
pub struct ConfigResource {
    config: Arc<Config>,
}

/// Host-side directive resource, referencing a directive inside the shared
/// Config AST by its path.
///
/// Holding `(Arc<Config>, path)` instead of a cloned `Directive` keeps each
/// handle small: cloning the directive would deep-copy its whole block
/// subtree, which is O(n^2) host memory when a plugin requests handles for
/// every directive of a large config.
pub struct DirectiveResource {
    config: Arc<Config>,
    /// Indices locating the directive: each element is the position of a
    /// directive within the current `ConfigItem` list, descending into that
    /// directive's block items for the next element.
    path: Vec<usize>,
}

/// Generated bindings from WIT file, isolated in a submodule to avoid name conflicts
mod bindings {
    wasmtime::component::bindgen!({
        path: "wit/nginx-lint-plugin.wit",
        world: "plugin",
        with: {
            "nginx-lint:plugin/config-api.config": super::ConfigResource,
            "nginx-lint:plugin/config-api.directive": super::DirectiveResource,
        },
    });
}

use bindings::nginx_lint::plugin::config_api;
use bindings::{Plugin, PluginPre};

/// Store data for component model execution
struct ComponentStoreData {
    limits: StoreLimits,
    table: ResourceTable,
}

impl wasmtime::component::HasData for ComponentStoreData {
    type Data<'a> = &'a mut ComponentStoreData;
}

/// Helper methods for resource table access.
///
/// These methods panic on invalid handles or table exhaustion. In the wasmtime
/// runtime context, panics in host functions are caught and converted to traps,
/// so the host process will not crash. The descriptive panic messages appear in
/// the trap error for debugging.
impl ComponentStoreData {
    fn get_directive(&self, self_: &Resource<DirectiveResource>) -> &ast::Directive {
        let resource = self
            .table
            .get(self_)
            .expect("invalid directive resource handle");
        resolve_directive(&resource.config, &resource.path)
    }

    fn get_config(&self, self_: &Resource<ConfigResource>) -> &Arc<Config> {
        &self
            .table
            .get(self_)
            .expect("invalid config resource handle")
            .config
    }

    /// Push a directive path into the resource table.
    ///
    /// Panics (trapped by wasmtime) if the resource table is full. This can
    /// happen if an untrusted plugin requests an excessive number of handles.
    /// The execution timeout should prevent this in practice.
    fn push_directive(
        &mut self,
        config: Arc<Config>,
        path: Vec<usize>,
    ) -> Resource<DirectiveResource> {
        self.table
            .push(DirectiveResource { config, path })
            .expect("resource table full: too many directive handles allocated")
    }
}

/// Resolve a directive path (see [`DirectiveResource`]) to the directive it
/// points at. Panics (trapped by wasmtime) on a path that does not point at
/// a directive; paths are host-constructed, so this only happens on a host
/// bug, not on plugin input.
fn resolve_directive<'a>(config: &'a Config, path: &[usize]) -> &'a ast::Directive {
    let mut items: &[ast::ConfigItem] = &config.items;
    let mut found: Option<&'a ast::Directive> = None;
    for &index in path {
        let ast::ConfigItem::Directive(directive) =
            items.get(index).expect("invalid directive path index")
        else {
            panic!("directive path does not point at a directive");
        };
        items = directive
            .block
            .as_ref()
            .map(|block| block.items.as_slice())
            .unwrap_or(&[]);
        found = Some(directive);
    }
    found.expect("empty directive path")
}

/// Resolve a directive path to the directive's block items. An empty path
/// resolves to the config's top-level items; a directive without a block
/// resolves to an empty slice.
fn resolve_block_items<'a>(config: &'a Config, path: &[usize]) -> &'a [ast::ConfigItem] {
    if path.is_empty() {
        return &config.items;
    }
    resolve_directive(config, path)
        .block
        .as_ref()
        .map(|block| block.items.as_slice())
        .unwrap_or(&[])
}

// === Host trait implementations for config-api ===

impl bindings::nginx_lint::plugin::types::Host for ComponentStoreData {}

impl bindings::nginx_lint::plugin::data_types::Host for ComponentStoreData {}

impl bindings::nginx_lint::plugin::parser_types::Host for ComponentStoreData {}

impl config_api::Host for ComponentStoreData {}

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
                results.push(config_api::ConfigItem::CommentItem(
                    config_api::CommentInfo {
                        text: comment.text.clone(),
                        line: comment.span.start.line as u32,
                        column: comment.span.start.column as u32,
                        leading_whitespace: comment.leading_whitespace.clone(),
                        trailing_whitespace: comment.trailing_whitespace.clone(),
                        start_offset: comment.span.start.offset as u32,
                        end_offset: comment.span.end.offset as u32,
                    },
                ));
            }
            ast::ConfigItem::BlankLine(blank) => {
                results.push(config_api::ConfigItem::BlankLineItem(
                    config_api::BlankLineInfo {
                        line: blank.span.start.line as u32,
                        content: blank.content.clone(),
                        start_offset: blank.span.start.offset as u32,
                    },
                ));
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

// === WIT type conversion functions ===

/// Replace control characters (except `\n` and `\t`) and Unicode
/// Bidi_Control characters in plugin-provided text with U+FFFD (�).
///
/// Plugin output is untrusted and is printed to the user's terminal or
/// embedded in generated documentation; raw control characters such as ESC
/// would allow ANSI escape sequence injection (e.g. spoofing or hiding parts
/// of the lint report), and bidi overrides (Trojan Source) can visually
/// reorder displayed text. Characters are replaced rather than removed so
/// that tampering stays visible to the user.
fn sanitize_text(text: &str) -> String {
    text.chars()
        .map(|c| {
            let is_disallowed_control = c.is_control() && c != '\n' && c != '\t';
            // All characters with the Unicode Bidi_Control property
            let is_bidi_control = matches!(
                c,
                '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
            );
            if is_disallowed_control || is_bidi_control {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

/// Sanitize an optional plugin-provided string.
fn sanitize_opt(text: &Option<String>) -> Option<String> {
    text.as_deref().map(sanitize_text)
}

/// Convert WIT Severity to crate Severity
fn convert_severity(severity: &bindings::nginx_lint::plugin::types::Severity) -> Severity {
    match severity {
        bindings::nginx_lint::plugin::types::Severity::Error => Severity::Error,
        bindings::nginx_lint::plugin::types::Severity::Warning => Severity::Warning,
    }
}

/// Convert WIT Fix to crate Fix
fn convert_fix(fix: &bindings::nginx_lint::plugin::types::Fix) -> crate::linter::Fix {
    crate::linter::Fix {
        line: fix.line as usize,
        old_text: fix.old_text.clone(),
        new_text: fix.new_text.clone(),
        delete_line: fix.delete_line,
        insert_after: fix.insert_after,
        start_offset: fix.start_offset.map(|v| v as usize),
        end_offset: fix.end_offset.map(|v| v as usize),
    }
}

/// Convert WIT LintError to crate LintError
fn convert_lint_error(error: &bindings::nginx_lint::plugin::types::LintError) -> LintError {
    let severity = convert_severity(&error.severity);
    let mut lint_error = LintError::new(
        &sanitize_text(&error.rule),
        &sanitize_text(&error.category),
        &sanitize_text(&error.message),
        severity,
    );

    if let (Some(line), Some(column)) = (error.line, error.column) {
        lint_error = lint_error.with_location(line as usize, column as usize);
    } else if let Some(line) = error.line {
        lint_error = lint_error.with_location(line as usize, 1);
    }

    for fix in &error.fixes {
        lint_error = lint_error.with_fix(convert_fix(fix));
    }

    lint_error
}

/// Convert WIT PluginSpec to our PluginSpec format
fn convert_plugin_spec(spec: &bindings::nginx_lint::plugin::types::PluginSpec) -> PluginSpec {
    PluginSpec {
        name: sanitize_text(&spec.name),
        category: sanitize_text(&spec.category),
        description: sanitize_text(&spec.description),
        api_version: sanitize_text(&spec.api_version),
        severity: sanitize_opt(&spec.severity),
        why: sanitize_opt(&spec.why),
        bad_example: sanitize_opt(&spec.bad_example),
        good_example: sanitize_opt(&spec.good_example),
        references: spec
            .references
            .as_ref()
            .map(|refs| refs.iter().map(|r| sanitize_text(r)).collect()),
        min_nginx_version: sanitize_opt(&spec.min_nginx_version),
        max_nginx_version: sanitize_opt(&spec.max_nginx_version),
    }
}

// === ComponentLintRule ===

/// A lint rule implemented as a WIT component model plugin
#[derive(Clone)]
pub struct ComponentLintRule {
    /// Path to the component file (for error reporting)
    path: PathBuf,
    /// Plugin metadata
    spec: PluginSpec,
    /// Pre-instantiated bindings: import resolution and type checking are
    /// done once at load time, so each check call only pays for
    /// instantiation itself (shared across threads; also holds the engine)
    plugin_pre: PluginPre<ComponentStoreData>,
    /// Memory limit in bytes
    memory_limit: u64,
    /// Execution timeout per call in epoch ticks (None = no timeout, for
    /// trusted plugins)
    timeout_ticks: Option<u64>,
    /// Leaked static strings for LintRule trait
    name: &'static str,
    category: &'static str,
    description: &'static str,
}

impl ComponentLintRule {
    /// Create a new component lint rule from compiled bytes
    pub fn new(
        engine: &Engine,
        path: PathBuf,
        component_bytes: &[u8],
        memory_limit: u64,
        timeout_ticks: Option<u64>,
    ) -> Result<Self, PluginError> {
        // Compile the component
        let component = wasmtime::component::Component::new(engine, component_bytes)
            .map_err(|e| PluginError::compile_error(&path, e.to_string()))?;

        // Register all host functions (types + config-api) and resolve the
        // component's imports once; per-call work is instantiation only
        let mut linker = wasmtime::component::Linker::<ComponentStoreData>::new(engine);
        Plugin::add_to_linker::<ComponentStoreData, ComponentStoreData>(&mut linker, |data| data)
            .map_err(|e| {
            PluginError::instantiate_error(&path, format!("Failed to add imports to linker: {}", e))
        })?;
        let instance_pre = linker
            .instantiate_pre(&component)
            .map_err(|e| PluginError::instantiate_error(&path, e.to_string()))?;
        let plugin_pre = PluginPre::new(instance_pre)
            .map_err(|e| PluginError::instantiate_error(&path, e.to_string()))?;

        // Get plugin spec
        let spec_wit = Self::get_plugin_spec(&plugin_pre, &path, memory_limit, timeout_ticks)?;
        let spec = convert_plugin_spec(&spec_wit);

        // Leak strings for 'static lifetime required by the LintRule trait.
        // These live for the entire program duration. Since plugins are loaded once
        // at startup and never unloaded, this is acceptable.
        let name: &'static str = Box::leak(spec.name.clone().into_boxed_str());
        let category: &'static str = Box::leak(spec.category.clone().into_boxed_str());
        let description: &'static str = Box::leak(spec.description.clone().into_boxed_str());

        Ok(Self {
            path,
            spec,
            plugin_pre,
            memory_limit,
            timeout_ticks,
            name,
            category,
            description,
        })
    }

    /// Create a store with limits and the execution deadline
    fn create_store(
        engine: &Engine,
        memory_limit: u64,
        timeout_ticks: Option<u64>,
    ) -> Store<ComponentStoreData> {
        let limits = StoreLimitsBuilder::new()
            .memory_size(memory_limit as usize)
            .build();
        let mut store = Store::new(
            engine,
            ComponentStoreData {
                limits,
                table: ResourceTable::new(),
            },
        );
        store.limiter(|data| &mut data.limits);
        if let Some(ticks) = timeout_ticks {
            // The loader's epoch ticker advances the engine epoch at a fixed
            // interval; execution traps with Trap::Interrupt once the
            // deadline is reached (wasmtime's default deadline behavior).
            store.set_epoch_deadline(ticks);
        }
        store
    }

    /// Get plugin spec by instantiating the component and calling spec()
    fn get_plugin_spec(
        plugin_pre: &PluginPre<ComponentStoreData>,
        path: &Path,
        memory_limit: u64,
        timeout_ticks: Option<u64>,
    ) -> Result<bindings::nginx_lint::plugin::types::PluginSpec, PluginError> {
        let mut store = Self::create_store(plugin_pre.engine(), memory_limit, timeout_ticks);
        let plugin = plugin_pre
            .instantiate(&mut store)
            .map_err(|e| PluginError::instantiate_error(path, e.to_string()))?;

        plugin
            .call_spec(&mut store)
            .map_err(|e| PluginError::execution_error(path, format!("spec() call failed: {}", e)))
    }

    /// Execute the check function using resource-based config access
    fn execute_check(
        &self,
        config: Arc<Config>,
        file_path: &Path,
    ) -> Result<Vec<LintError>, PluginError> {
        let mut store = Self::create_store(
            self.plugin_pre.engine(),
            self.memory_limit,
            self.timeout_ticks,
        );
        let plugin = self
            .plugin_pre
            .instantiate(&mut store)
            .map_err(|e| PluginError::instantiate_error(&self.path, e.to_string()))?;

        // Create config resource handle
        let config_resource = store
            .data_mut()
            .table
            .push(ConfigResource { config })
            .map_err(|e| {
                PluginError::execution_error(
                    &self.path,
                    format!("Failed to create config resource: {}", e),
                )
            })?;

        let path_str = file_path.to_string_lossy().to_string();
        let wit_errors = plugin
            .call_check(&mut store, config_resource, &path_str)
            .map_err(|e| {
                // Epoch deadline expiry surfaces as Trap::Interrupt
                if e.downcast_ref::<Trap>() == Some(&Trap::Interrupt) {
                    PluginError::timeout(&self.path)
                } else {
                    PluginError::execution_error(&self.path, format!("check() failed: {}", e))
                }
            })?;

        // Note: The config resource and any directive resources created during
        // the check are cleaned up when `store` is dropped at function exit.
        // A fresh store is created for each execute_check call, so resources
        // do not leak across lint runs.

        Ok(wit_errors.iter().map(convert_lint_error).collect())
    }

    /// Run a check with a shared config handle, converting failures into a
    /// reported lint error
    fn run_check(&self, config: Arc<Config>, path: &Path) -> Vec<LintError> {
        match self.execute_check(config, path) {
            Ok(errors) => errors,
            Err(e) => {
                vec![LintError::new(
                    self.name,
                    self.category,
                    &format!("Plugin execution failed: {}", e),
                    Severity::Error,
                )]
            }
        }
    }
}

impl LintRule for ComponentLintRule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn category(&self) -> &'static str {
        self.category
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn check(&self, config: &Config, path: &Path) -> Vec<LintError> {
        // Direct callers only have a borrowed Config, so this pays a deep
        // clone. The linter passes a shared handle via check_shared instead.
        self.run_check(Arc::new(config.clone()), path)
    }

    fn wants_shared_config(&self) -> bool {
        true
    }

    fn check_shared(&self, config: &Arc<Config>, path: &Path) -> Vec<LintError> {
        self.run_check(config.clone(), path)
    }

    fn why(&self) -> Option<&str> {
        self.spec.why.as_deref()
    }

    fn bad_example(&self) -> Option<&str> {
        self.spec.bad_example.as_deref()
    }

    fn good_example(&self) -> Option<&str> {
        self.spec.good_example.as_deref()
    }

    fn references(&self) -> Option<Vec<String>> {
        self.spec.references.clone()
    }

    fn min_nginx_version(&self) -> Option<&str> {
        self.spec.min_nginx_version.as_deref()
    }

    fn max_nginx_version(&self) -> Option<&str> {
        self.spec.max_nginx_version.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Temporary measurement harness for the WIT-boundary cost investigation.
    /// Run manually with:
    /// cargo test --release --features plugins --lib phase_timing -- --ignored --nocapture
    #[test]
    #[ignore]
    fn phase_timing() {
        use crate::plugin::{CompilationCache, PluginLoader};
        use std::time::Instant;

        let wasm_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/builtin-plugins/server_tokens_enabled.wasm");
        if !wasm_path.exists() {
            eprintln!("SKIP: run `make build-plugins` first");
            return;
        }

        let loader = PluginLoader::new_with_cache(CompilationCache::Disabled).unwrap();
        let bytes = std::fs::read(&wasm_path).unwrap();
        let rule = loader
            .load_component_from_bytes(&wasm_path, &bytes)
            .unwrap();

        for n_servers in [30, 300] {
            let mut src = String::from("http {\n  gzip on;\n");
            for i in 0..n_servers {
                src.push_str(&format!(
                    "  server {{\n    listen 80;\n    server_name s{i}.example.com;\n    server_tokens off;\n    location / {{\n      proxy_pass http://127.0.0.1:8080;\n      proxy_set_header Host $host;\n    }}\n    error_log /var/log/nginx/error.log;\n  }}\n"
                ));
            }
            src.push_str("}\n");
            let config = crate::parser::parse_string(&src).unwrap();
            let n_directives = config.all_directives().count();
            let shared = Arc::new(config);
            let iters = 200;

            // Phase A: store + instantiate only
            let start = Instant::now();
            for _ in 0..iters {
                let mut store = ComponentLintRule::create_store(
                    rule.plugin_pre.engine(),
                    rule.memory_limit,
                    rule.timeout_ticks,
                );
                let _plugin = rule.plugin_pre.instantiate(&mut store).unwrap();
            }
            let phase_a = start.elapsed() / iters;

            // Phase B: full check (instantiate + guest reconstruct + rule logic)
            let start = Instant::now();
            for _ in 0..iters {
                let _ = rule.check_shared(&shared, Path::new("test.conf"));
            }
            let phase_b = start.elapsed() / iters;

            // Phase C: host-side conversion only (what the host does when the
            // guest walks items/data/block-items), no WIT lowering, no guest
            let start = Instant::now();
            for _ in 0..iters {
                let mut data = ComponentStoreData {
                    limits: StoreLimitsBuilder::new().build(),
                    table: ResourceTable::new(),
                };
                let cfg_res = data
                    .table
                    .push(ConfigResource {
                        config: shared.clone(),
                    })
                    .unwrap();
                let items = config_api::HostConfig::items(&mut data, cfg_res);
                walk_items(&mut data, items);
            }
            let phase_c = start.elapsed() / iters;

            println!(
                "directives={n_directives}: instantiate={phase_a:?} full_check={phase_b:?} host_conv={phase_c:?} guest+lowering={:?}",
                phase_b.saturating_sub(phase_a).saturating_sub(phase_c)
            );
        }

        fn walk_items(data: &mut ComponentStoreData, items: Vec<config_api::ConfigItem>) {
            for item in items {
                if let config_api::ConfigItem::DirectiveItem(handle) = item {
                    let alias = Resource::new_own(handle.rep());
                    let d = config_api::HostDirective::data(data, alias);
                    if d.has_block {
                        let alias = Resource::new_own(handle.rep());
                        let children = config_api::HostDirective::block_items(data, alias);
                        walk_items(data, children);
                    }
                }
            }
        }
    }

    #[test]
    fn test_sanitize_text_replaces_ansi_escape() {
        // ESC [ 31 m (red) + ESC [ 2 K (erase line) — typical injection payloads.
        // Replaced with U+FFFD so the tampering stays visible.
        let input = "\x1b[31mfake error\x1b[2K";
        assert_eq!(sanitize_text(input), "\u{FFFD}[31mfake error\u{FFFD}[2K");
    }

    #[test]
    fn test_sanitize_text_keeps_newline_and_tab() {
        let input = "line1\n\tline2";
        assert_eq!(sanitize_text(input), "line1\n\tline2");
    }

    #[test]
    fn test_sanitize_text_replaces_other_control_chars() {
        // \r (CR), \x07 (BEL), \x08 (BS) are replaced; unicode text is kept
        let input = "警告\r\x07\x08です";
        assert_eq!(sanitize_text(input), "警告\u{FFFD}\u{FFFD}\u{FFFD}です");
    }

    #[test]
    fn test_sanitize_text_replaces_c1_control_chars() {
        // 0x9B is a C1 control char acting as a standalone CSI
        let input = "a\u{9B}31mb";
        assert_eq!(sanitize_text(input), "a\u{FFFD}31mb");
    }

    #[test]
    fn test_sanitize_text_replaces_bidi_controls() {
        // U+202E (RTL override) and U+2066 (LTR isolate) enable Trojan
        // Source style display reordering
        let input = "safe\u{202E}gnirts live\u{2066}x";
        assert_eq!(sanitize_text(input), "safe\u{FFFD}gnirts live\u{FFFD}x");
    }

    #[test]
    fn test_sanitize_text_replaces_implicit_bidi_marks() {
        // U+200E (LRM), U+200F (RLM), U+061C (ALM) — the remaining
        // Bidi_Control characters
        let input = "a\u{200E}b\u{200F}c\u{061C}d";
        assert_eq!(sanitize_text(input), "a\u{FFFD}b\u{FFFD}c\u{FFFD}d");
    }

    #[test]
    fn test_convert_lint_error_sanitizes_strings() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "evil\x1brule".to_string(),
            category: "cat\x1begory".to_string(),
            message: "msg\x1b[31m with escape".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Warning,
            line: Some(1),
            column: Some(1),
            fixes: vec![],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.rule, "evil\u{FFFD}rule");
        assert_eq!(error.category, "cat\u{FFFD}egory");
        assert_eq!(error.message, "msg\u{FFFD}[31m with escape");
    }

    #[test]
    fn test_convert_plugin_spec_sanitizes_strings() {
        let wit_spec = bindings::nginx_lint::plugin::types::PluginSpec {
            name: "na\x1bme".to_string(),
            category: "cat\x1begory".to_string(),
            description: "desc\x1b[0m".to_string(),
            api_version: "1.0".to_string(),
            severity: Some("warn\x1bing".to_string()),
            why: Some("why\x07".to_string()),
            bad_example: Some("bad\nexample\x1b".to_string()),
            good_example: Some("good\texample\x08".to_string()),
            references: Some(vec!["https://example.com/\x1b[31m".to_string()]),
            min_nginx_version: Some("1.0\x1b".to_string()),
            max_nginx_version: None,
        };
        let spec = convert_plugin_spec(&wit_spec);
        assert_eq!(spec.name, "na\u{FFFD}me");
        assert_eq!(spec.category, "cat\u{FFFD}egory");
        assert_eq!(spec.description, "desc\u{FFFD}[0m");
        assert_eq!(spec.severity.as_deref(), Some("warn\u{FFFD}ing"));
        assert_eq!(spec.why.as_deref(), Some("why\u{FFFD}"));
        // Newlines and tabs in examples are preserved
        assert_eq!(spec.bad_example.as_deref(), Some("bad\nexample\u{FFFD}"));
        assert_eq!(spec.good_example.as_deref(), Some("good\texample\u{FFFD}"));
        assert_eq!(
            spec.references,
            Some(vec!["https://example.com/\u{FFFD}[31m".to_string()])
        );
        assert_eq!(spec.min_nginx_version.as_deref(), Some("1.0\u{FFFD}"));
    }

    #[test]
    fn test_convert_severity_error() {
        let wit_severity = bindings::nginx_lint::plugin::types::Severity::Error;
        assert!(matches!(convert_severity(&wit_severity), Severity::Error));
    }

    #[test]
    fn test_convert_severity_warning() {
        let wit_severity = bindings::nginx_lint::plugin::types::Severity::Warning;
        assert!(matches!(convert_severity(&wit_severity), Severity::Warning));
    }

    #[test]
    fn test_convert_fix_basic() {
        let wit_fix = bindings::nginx_lint::plugin::types::Fix {
            line: 10,
            old_text: Some("old".to_string()),
            new_text: "new".to_string(),
            delete_line: false,
            insert_after: true,
            start_offset: Some(5),
            end_offset: Some(8),
        };
        let fix = convert_fix(&wit_fix);
        assert_eq!(fix.line, 10);
        assert_eq!(fix.old_text.as_deref(), Some("old"));
        assert_eq!(fix.new_text, "new");
        assert!(!fix.delete_line);
        assert!(fix.insert_after);
        assert_eq!(fix.start_offset, Some(5));
        assert_eq!(fix.end_offset, Some(8));
    }

    #[test]
    fn test_convert_fix_optional_fields_none() {
        let wit_fix = bindings::nginx_lint::plugin::types::Fix {
            line: 1,
            old_text: None,
            new_text: "text".to_string(),
            delete_line: true,
            insert_after: false,
            start_offset: None,
            end_offset: None,
        };
        let fix = convert_fix(&wit_fix);
        assert!(fix.old_text.is_none());
        assert!(fix.delete_line);
        assert!(fix.start_offset.is_none());
        assert!(fix.end_offset.is_none());
    }

    #[test]
    fn test_convert_lint_error_with_location() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "test-rule".to_string(),
            category: "test-cat".to_string(),
            message: "test message".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Warning,
            line: Some(42),
            column: Some(10),
            fixes: vec![],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.rule, "test-rule");
        assert_eq!(error.category, "test-cat");
        assert_eq!(error.message, "test message");
        assert!(matches!(error.severity, Severity::Warning));
        assert_eq!(error.line, Some(42));
        assert_eq!(error.column, Some(10));
    }

    #[test]
    fn test_convert_lint_error_line_only() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "rule".to_string(),
            category: "cat".to_string(),
            message: "msg".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Error,
            line: Some(5),
            column: None,
            fixes: vec![],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.line, Some(5));
        assert_eq!(error.column, Some(1)); // defaults to column 1
    }

    #[test]
    fn test_convert_lint_error_no_location() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "rule".to_string(),
            category: "cat".to_string(),
            message: "msg".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Error,
            line: None,
            column: None,
            fixes: vec![],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.line, None);
        assert_eq!(error.column, None);
    }

    #[test]
    fn test_convert_lint_error_with_fixes() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "rule".to_string(),
            category: "cat".to_string(),
            message: "msg".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Warning,
            line: Some(1),
            column: Some(1),
            fixes: vec![bindings::nginx_lint::plugin::types::Fix {
                line: 1,
                old_text: Some("bad".to_string()),
                new_text: "good".to_string(),
                delete_line: false,
                insert_after: false,
                start_offset: None,
                end_offset: None,
            }],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.fixes.len(), 1);
        assert_eq!(error.fixes[0].new_text, "good");
    }

    #[test]
    fn test_convert_plugin_spec() {
        let wit_spec = bindings::nginx_lint::plugin::types::PluginSpec {
            name: "test-plugin".to_string(),
            category: "security".to_string(),
            description: "A test plugin".to_string(),
            api_version: "1.0".to_string(),
            severity: Some("warning".to_string()),
            why: Some("because".to_string()),
            bad_example: Some("bad".to_string()),
            good_example: Some("good".to_string()),
            references: Some(vec!["https://example.com".to_string()]),
            min_nginx_version: Some("0.6.27".to_string()),
            max_nginx_version: Some("1.30.0".to_string()),
        };
        let spec = convert_plugin_spec(&wit_spec);
        assert_eq!(spec.name, "test-plugin");
        assert_eq!(spec.category, "security");
        assert_eq!(spec.description, "A test plugin");
        assert_eq!(spec.api_version, "1.0");
        assert_eq!(spec.severity.as_deref(), Some("warning"));
        assert_eq!(spec.why.as_deref(), Some("because"));
        assert_eq!(spec.bad_example.as_deref(), Some("bad"));
        assert_eq!(spec.good_example.as_deref(), Some("good"));
        assert_eq!(
            spec.references,
            Some(vec!["https://example.com".to_string()])
        );
        assert_eq!(spec.min_nginx_version.as_deref(), Some("0.6.27"));
        assert_eq!(spec.max_nginx_version.as_deref(), Some("1.30.0"));
    }

    #[test]
    fn test_convert_plugin_spec_optional_none() {
        let wit_spec = bindings::nginx_lint::plugin::types::PluginSpec {
            name: "minimal".to_string(),
            category: "test".to_string(),
            description: "Minimal".to_string(),
            api_version: "1.0".to_string(),
            severity: None,
            why: None,
            bad_example: None,
            good_example: None,
            references: None,
            min_nginx_version: None,
            max_nginx_version: None,
        };
        let spec = convert_plugin_spec(&wit_spec);
        assert_eq!(spec.name, "minimal");
        assert!(spec.severity.is_none());
        assert!(spec.why.is_none());
        assert!(spec.references.is_none());
        assert!(spec.min_nginx_version.is_none());
        assert!(spec.max_nginx_version.is_none());
    }

    #[test]
    fn test_new_with_invalid_bytes() {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config).unwrap();
        let result = ComponentLintRule::new(
            &engine,
            PathBuf::from("test.wasm"),
            b"not a wasm component",
            256 * 1024 * 1024,
            Some(100),
        );
        assert!(matches!(result, Err(PluginError::CompileError { .. })));
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
