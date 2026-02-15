//! WASM-mode config types backed by host resource handles.
//!
//! These types provide the same API surface as the parser AST types
//! but delegate to host functions via WIT resource handles instead of
//! holding the data directly. This eliminates the need for JSON
//! parsing libraries in the WASM plugin binary.

use crate::types::{DirectiveExt, Fix};
use crate::wit_guest::nginx_lint::plugin::config_api;
use crate::wit_guest::nginx_lint::plugin::types as wit_types;

/// Parsed nginx configuration (backed by host resource handle).
pub struct Config {
    handle: config_api::Config,
}

impl Config {
    /// Create from a WIT resource handle (used by the export macro).
    pub fn from_handle(handle: config_api::Config) -> Self {
        Self { handle }
    }

    /// Iterate over all directives recursively with parent context.
    pub fn all_directives_with_context(&self) -> Vec<DirectiveWithContext> {
        self.handle
            .all_directives_with_context()
            .into_iter()
            .map(|ctx| DirectiveWithContext {
                directive: Directive::from_handle(ctx.directive),
                parent_stack: ctx.parent_stack,
                depth: ctx.depth as usize,
            })
            .collect()
    }

    /// Iterate over all directives recursively.
    pub fn all_directives(&self) -> Vec<Directive> {
        self.handle
            .all_directives()
            .into_iter()
            .map(Directive::from_handle)
            .collect()
    }

    /// Get the top-level config items.
    pub fn items(&self) -> Vec<ConfigItem> {
        self.handle
            .items()
            .into_iter()
            .map(ConfigItem::from_wit)
            .collect()
    }

    /// Get the include context (parent block names from include directive).
    pub fn include_context(&self) -> Vec<String> {
        self.handle.include_context()
    }

    /// Check if this config is included from within a specific context.
    pub fn is_included_from(&self, context: &str) -> bool {
        self.handle.is_included_from(context)
    }

    /// Check if included from http context.
    pub fn is_included_from_http(&self) -> bool {
        self.handle.is_included_from_http()
    }

    /// Check if included from http > server context.
    pub fn is_included_from_http_server(&self) -> bool {
        self.handle.is_included_from_http_server()
    }

    /// Check if included from http > ... > location context.
    pub fn is_included_from_http_location(&self) -> bool {
        self.handle.is_included_from_http_location()
    }

    /// Check if included from stream context.
    pub fn is_included_from_stream(&self) -> bool {
        self.handle.is_included_from_stream()
    }

    /// Get the immediate parent context.
    pub fn immediate_parent_context(&self) -> Option<String> {
        self.handle.immediate_parent_context()
    }
}

/// An nginx directive (backed by host resource handle).
pub struct Directive {
    handle: config_api::Directive,
}

impl Directive {
    /// Create from a WIT resource handle.
    pub(crate) fn from_handle(handle: config_api::Directive) -> Self {
        Self { handle }
    }

    /// Get the directive name.
    pub fn name(&self) -> String {
        self.handle.name()
    }

    /// Get all arguments as records.
    pub fn args(&self) -> Vec<ArgumentInfo> {
        self.handle
            .args()
            .into_iter()
            .map(ArgumentInfo::from_wit)
            .collect()
    }

    /// Check if the directive has a block.
    pub fn has_block(&self) -> bool {
        self.handle.has_block()
    }

    /// Get the items inside the directive's block.
    pub fn block_items(&self) -> Vec<ConfigItem> {
        self.handle
            .block_items()
            .into_iter()
            .map(ConfigItem::from_wit)
            .collect()
    }

    /// Check if the block contains raw content (e.g., lua blocks).
    pub fn block_is_raw(&self) -> bool {
        self.handle.block_is_raw()
    }

    /// Get the start byte offset (0-based).
    pub fn start_offset(&self) -> usize {
        self.handle.start_offset() as usize
    }

    /// Get the end byte offset (0-based).
    pub fn end_offset(&self) -> usize {
        self.handle.end_offset() as usize
    }

    /// Get the leading whitespace before the directive.
    pub fn leading_whitespace(&self) -> String {
        self.handle.leading_whitespace()
    }

    /// Get the trailing whitespace after the directive.
    pub fn trailing_whitespace(&self) -> String {
        self.handle.trailing_whitespace()
    }

    /// Get the space before the terminator (; or {).
    pub fn space_before_terminator(&self) -> String {
        self.handle.space_before_terminator()
    }
}

impl DirectiveExt for Directive {
    fn is(&self, name: &str) -> bool {
        self.handle.is(name)
    }

    fn first_arg(&self) -> Option<&str> {
        // NOTE: We can't return &str from a host call because the string
        // is returned by value. Plugin code that calls first_arg() in WASM mode
        // would need to use first_arg_owned() or we handle this differently.
        // For now, we'll use a workaround - see first_arg_owned().
        None // This won't be called - inherent method below takes precedence
    }

    fn first_arg_is(&self, value: &str) -> bool {
        self.handle.first_arg_is(value)
    }

    fn arg_at(&self, _index: usize) -> Option<&str> {
        None // Same issue as first_arg - use arg_at_owned()
    }

    fn last_arg(&self) -> Option<&str> {
        None // Same issue - use last_arg_owned()
    }

    fn has_arg(&self, value: &str) -> bool {
        self.handle.has_arg(value)
    }

    fn arg_count(&self) -> usize {
        self.handle.arg_count() as usize
    }

    fn line(&self) -> usize {
        self.handle.line() as usize
    }

    fn column(&self) -> usize {
        self.handle.column() as usize
    }

    fn full_start_offset(&self) -> usize {
        self.start_offset() - self.leading_whitespace().len()
    }

    fn replace_with(&self, new_text: &str) -> Fix {
        convert_wit_fix(self.handle.replace_with(new_text))
    }

    fn delete_line(&self) -> Fix {
        convert_wit_fix(self.handle.delete_line_fix())
    }

    fn insert_after(&self, new_text: &str) -> Fix {
        convert_wit_fix(self.handle.insert_after(new_text))
    }

    fn insert_after_many(&self, lines: &[&str]) -> Fix {
        let owned: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        convert_wit_fix(self.handle.insert_after_many(&owned))
    }

    fn insert_before(&self, new_text: &str) -> Fix {
        convert_wit_fix(self.handle.insert_before(new_text))
    }

    fn insert_before_many(&self, lines: &[&str]) -> Fix {
        let owned: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        convert_wit_fix(self.handle.insert_before_many(&owned))
    }
}

/// Additional methods for WASM-mode Directive that return owned strings.
impl Directive {
    /// Get the first argument value (owned string).
    pub fn first_arg_owned(&self) -> Option<String> {
        self.handle.first_arg()
    }

    /// Get the argument at the given index (owned string).
    pub fn arg_at_owned(&self, index: usize) -> Option<String> {
        self.handle.arg_at(index as u32)
    }

    /// Get the last argument value (owned string).
    pub fn last_arg_owned(&self) -> Option<String> {
        self.handle.last_arg()
    }
}

/// A directive paired with its parent block context.
pub struct DirectiveWithContext {
    /// The directive itself.
    pub directive: Directive,
    /// Stack of parent directive names (e.g., `["http", "server"]`).
    pub parent_stack: Vec<String>,
    /// Nesting depth (0 = root level).
    pub depth: usize,
}

impl DirectiveWithContext {
    /// Get the immediate parent directive name, if any.
    pub fn parent(&self) -> Option<&str> {
        self.parent_stack.last().map(|s| s.as_str())
    }

    /// Check if this directive is inside a specific parent context.
    pub fn is_inside(&self, parent_name: &str) -> bool {
        self.parent_stack.iter().any(|p| p == parent_name)
    }

    /// Check if the immediate parent is a specific directive.
    pub fn parent_is(&self, parent_name: &str) -> bool {
        self.parent() == Some(parent_name)
    }

    /// Check if this directive is at root level.
    pub fn is_at_root(&self) -> bool {
        self.parent_stack.is_empty()
    }
}

/// A config item (directive, comment, or blank line).
pub enum ConfigItem {
    Directive(Directive),
    Comment(CommentInfo),
    BlankLine(BlankLineInfo),
}

impl ConfigItem {
    fn from_wit(item: config_api::ConfigItem) -> Self {
        match item {
            config_api::ConfigItem::DirectiveItem(d) => {
                ConfigItem::Directive(Directive::from_handle(d))
            }
            config_api::ConfigItem::CommentItem(c) => ConfigItem::Comment(CommentInfo {
                text: c.text,
                line: c.line as usize,
                column: c.column as usize,
                leading_whitespace: c.leading_whitespace,
                trailing_whitespace: c.trailing_whitespace,
                start_offset: c.start_offset as usize,
                end_offset: c.end_offset as usize,
            }),
            config_api::ConfigItem::BlankLineItem(b) => ConfigItem::BlankLine(BlankLineInfo {
                line: b.line as usize,
                content: b.content,
                start_offset: b.start_offset as usize,
            }),
        }
    }
}

/// Comment data.
pub struct CommentInfo {
    pub text: String,
    pub line: usize,
    pub column: usize,
    pub leading_whitespace: String,
    pub trailing_whitespace: String,
    pub start_offset: usize,
    pub end_offset: usize,
}

/// Blank line data.
pub struct BlankLineInfo {
    pub line: usize,
    pub content: String,
    pub start_offset: usize,
}

/// Argument data.
pub struct ArgumentInfo {
    pub value: String,
    pub raw: String,
    pub arg_type: ArgumentType,
    pub line: usize,
    pub column: usize,
    pub start_offset: usize,
    pub end_offset: usize,
}

impl ArgumentInfo {
    fn from_wit(arg: config_api::ArgumentInfo) -> Self {
        Self {
            value: arg.value,
            raw: arg.raw,
            arg_type: match arg.arg_type {
                config_api::ArgumentType::Literal => ArgumentType::Literal,
                config_api::ArgumentType::QuotedString => ArgumentType::QuotedString,
                config_api::ArgumentType::SingleQuotedString => ArgumentType::SingleQuotedString,
                config_api::ArgumentType::Variable => ArgumentType::Variable,
            },
            line: arg.line as usize,
            column: arg.column as usize,
            start_offset: arg.start_offset as usize,
            end_offset: arg.end_offset as usize,
        }
    }

    /// Get the argument's string value.
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Check if this is a variable argument.
    pub fn is_variable(&self) -> bool {
        matches!(self.arg_type, ArgumentType::Variable)
    }

    /// Check if this is a quoted string.
    pub fn is_quoted(&self) -> bool {
        matches!(self.arg_type, ArgumentType::QuotedString)
    }

    /// Check if this is a single-quoted string.
    pub fn is_single_quoted(&self) -> bool {
        matches!(self.arg_type, ArgumentType::SingleQuotedString)
    }

    /// Check if this is a literal value.
    pub fn is_literal(&self) -> bool {
        matches!(self.arg_type, ArgumentType::Literal)
    }
}

/// Argument value type.
pub enum ArgumentType {
    Literal,
    QuotedString,
    SingleQuotedString,
    Variable,
}

/// Convert a WIT fix to an SDK fix.
fn convert_wit_fix(fix: wit_types::Fix) -> Fix {
    Fix {
        line: fix.line as usize,
        old_text: fix.old_text,
        new_text: fix.new_text,
        delete_line: fix.delete_line,
        insert_after: fix.insert_after,
        start_offset: fix.start_offset.map(|v| v as usize),
        end_offset: fix.end_offset.map(|v| v as usize),
    }
}
