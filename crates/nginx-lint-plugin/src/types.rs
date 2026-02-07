//! Types for plugin development
//!
//! These types mirror the nginx-lint AST and error types for use in WASM plugins.

use serde::{Deserialize, Serialize};

/// Current API version for the plugin SDK
pub const API_VERSION: &str = "1.0";

/// Plugin metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSpec {
    /// Unique name for the rule (e.g., "my-custom-rule")
    pub name: String,
    /// Category (e.g., "security", "style", "best_practices", "custom")
    pub category: String,
    /// Human-readable description
    pub description: String,
    /// API version the plugin uses for input/output format
    pub api_version: String,
    /// Severity level (error, warning)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    /// Why this rule exists (detailed explanation)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// Example of bad configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bad_example: Option<String>,
    /// Example of good configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub good_example: Option<String>,
    /// References (URLs, documentation links)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<Vec<String>>,
}

impl PluginSpec {
    /// Create a new PluginSpec with the current API version
    pub fn new(
        name: impl Into<String>,
        category: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            category: category.into(),
            description: description.into(),
            api_version: API_VERSION.to_string(),
            severity: None,
            why: None,
            bad_example: None,
            good_example: None,
            references: None,
        }
    }

    /// Set the severity level
    pub fn with_severity(mut self, severity: impl Into<String>) -> Self {
        self.severity = Some(severity.into());
        self
    }

    /// Set the why documentation
    pub fn with_why(mut self, why: impl Into<String>) -> Self {
        self.why = Some(why.into());
        self
    }

    /// Set the bad example
    pub fn with_bad_example(mut self, example: impl Into<String>) -> Self {
        self.bad_example = Some(example.into());
        self
    }

    /// Set the good example
    pub fn with_good_example(mut self, example: impl Into<String>) -> Self {
        self.good_example = Some(example.into());
        self
    }

    /// Set references
    pub fn with_references(mut self, refs: Vec<String>) -> Self {
        self.references = Some(refs);
        self
    }

    /// Create an error builder that uses this plugin's name and category
    ///
    /// This reduces boilerplate when creating errors in the check method.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
    ///     let spec = self.spec();
    ///     let error = spec.error_builder();
    ///
    ///     // Instead of:
    ///     // LintError::warning("my-rule", "security", "message", line, col)
    ///     // Use:
    ///     error.warning("message", line, col)
    /// }
    /// ```
    pub fn error_builder(&self) -> ErrorBuilder {
        ErrorBuilder {
            rule: self.name.clone(),
            category: self.category.clone(),
        }
    }
}

/// Builder for creating LintError with pre-filled rule and category
#[derive(Debug, Clone)]
pub struct ErrorBuilder {
    rule: String,
    category: String,
}

impl ErrorBuilder {
    /// Create an error with Error severity
    pub fn error(&self, message: &str, line: usize, column: usize) -> LintError {
        LintError::error(&self.rule, &self.category, message, line, column)
    }

    /// Create an error with Warning severity
    pub fn warning(&self, message: &str, line: usize, column: usize) -> LintError {
        LintError::warning(&self.rule, &self.category, message, line, column)
    }

    /// Create an error from a directive's location
    pub fn error_at(&self, message: &str, directive: &Directive) -> LintError {
        self.error(
            message,
            directive.span.start.line,
            directive.span.start.column,
        )
    }

    /// Create a warning from a directive's location
    pub fn warning_at(&self, message: &str, directive: &Directive) -> LintError {
        self.warning(
            message,
            directive.span.start.line,
            directive.span.start.column,
        )
    }
}

/// Severity level for lint errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// Represents a fix that can be applied to resolve a lint error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fix {
    /// Line number where the fix should be applied (1-indexed)
    pub line: usize,
    /// The original text to replace (if None and new_text is empty, delete the line)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_text: Option<String>,
    /// The new text to insert (empty string with old_text=None means delete)
    pub new_text: String,
    /// Whether to delete the entire line
    #[serde(default)]
    pub delete_line: bool,
    /// Whether to insert new_text as a new line after the specified line
    #[serde(default)]
    pub insert_after: bool,
    /// Start byte offset for range-based fix (0-indexed, inclusive)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<usize>,
    /// End byte offset for range-based fix (0-indexed, exclusive)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_offset: Option<usize>,
}

impl Fix {
    /// Create a fix that deletes an entire line
    pub fn delete(line: usize) -> Self {
        Self {
            line,
            old_text: None,
            new_text: String::new(),
            delete_line: true,
            insert_after: false,
            start_offset: None,
            end_offset: None,
        }
    }

    /// Create a fix that inserts a new line after the specified line
    pub fn insert_after(line: usize, new_text: &str) -> Self {
        Self {
            line,
            old_text: None,
            new_text: new_text.to_string(),
            delete_line: false,
            insert_after: true,
            start_offset: None,
            end_offset: None,
        }
    }

    /// Create a range-based fix that replaces bytes from start to end offset
    ///
    /// This allows multiple fixes on the same line as long as their ranges don't overlap.
    pub fn replace_range(start_offset: usize, end_offset: usize, new_text: &str) -> Self {
        Self {
            line: 0, // Not used for range-based fixes
            old_text: None,
            new_text: new_text.to_string(),
            delete_line: false,
            insert_after: false,
            start_offset: Some(start_offset),
            end_offset: Some(end_offset),
        }
    }

    /// Check if this is a range-based fix
    pub fn is_range_based(&self) -> bool {
        self.start_offset.is_some() && self.end_offset.is_some()
    }
}

/// A lint error reported by a plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintError {
    pub rule: String,
    pub category: String,
    pub message: String,
    pub severity: Severity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<Fix>,
}

impl LintError {
    /// Create a new error with Error severity
    pub fn error(rule: &str, category: &str, message: &str, line: usize, column: usize) -> Self {
        Self {
            rule: rule.to_string(),
            category: category.to_string(),
            message: message.to_string(),
            severity: Severity::Error,
            line: if line > 0 { Some(line) } else { None },
            column: if column > 0 { Some(column) } else { None },
            fixes: Vec::new(),
        }
    }

    /// Create a new error with Warning severity
    pub fn warning(rule: &str, category: &str, message: &str, line: usize, column: usize) -> Self {
        Self {
            rule: rule.to_string(),
            category: category.to_string(),
            message: message.to_string(),
            severity: Severity::Warning,
            line: if line > 0 { Some(line) } else { None },
            column: if column > 0 { Some(column) } else { None },
            fixes: Vec::new(),
        }
    }

    /// Attach a fix to this error
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fixes.push(fix);
        self
    }

    /// Attach multiple fixes to this error
    pub fn with_fixes(mut self, fixes: Vec<Fix>) -> Self {
        self.fixes.extend(fixes);
        self
    }
}

/// Trait that plugins must implement
pub trait Plugin: Default {
    /// Return plugin metadata
    fn spec(&self) -> PluginSpec;

    /// Check the configuration and return any lint errors
    fn check(&self, config: &Config, path: &str) -> Vec<LintError>;
}

// Re-export AST types from nginx-lint-common
pub use nginx_lint_common::parser::ast::{
    Argument, ArgumentValue, Block, Comment, Config, ConfigItem, Directive, Position, Span,
};

/// Extension trait for Config to provide helper methods
pub trait ConfigExt {
    /// Iterate over all directives recursively
    fn all_directives(&self) -> AllDirectivesIter<'_>;

    /// Iterate over all directives recursively with parent context information
    fn all_directives_with_context(&self) -> AllDirectivesWithContextIter<'_>;

    /// Check if this config is included from within a specific context
    fn is_included_from(&self, context: &str) -> bool;

    /// Check if this config is included from within http context
    fn is_included_from_http(&self) -> bool;

    /// Check if this config is included from within http > server context
    fn is_included_from_http_server(&self) -> bool;

    /// Check if this config is included from within http > server > location context
    fn is_included_from_http_location(&self) -> bool;

    /// Check if this config is included from within stream context
    fn is_included_from_stream(&self) -> bool;

    /// Get the immediate parent context (last element in include_context)
    fn immediate_parent_context(&self) -> Option<&str>;
}

impl ConfigExt for Config {
    fn all_directives(&self) -> AllDirectivesIter<'_> {
        AllDirectivesIter {
            stack: vec![self.items.iter()],
        }
    }

    fn all_directives_with_context(&self) -> AllDirectivesWithContextIter<'_> {
        let initial_context: Vec<String> = self.include_context.clone();
        AllDirectivesWithContextIter::new(&self.items, initial_context)
    }

    fn is_included_from(&self, context: &str) -> bool {
        self.include_context.iter().any(|c| c == context)
    }

    fn is_included_from_http(&self) -> bool {
        self.is_included_from("http")
    }

    fn is_included_from_http_server(&self) -> bool {
        let ctx = &self.include_context;
        ctx.iter().any(|c| c == "http")
            && ctx.iter().any(|c| c == "server")
            && ctx.iter().position(|c| c == "http") < ctx.iter().position(|c| c == "server")
    }

    fn is_included_from_http_location(&self) -> bool {
        let ctx = &self.include_context;
        ctx.iter().any(|c| c == "http")
            && ctx.iter().any(|c| c == "location")
            && ctx.iter().position(|c| c == "http") < ctx.iter().position(|c| c == "location")
    }

    fn is_included_from_stream(&self) -> bool {
        self.is_included_from("stream")
    }

    fn immediate_parent_context(&self) -> Option<&str> {
        self.include_context.last().map(|s| s.as_str())
    }
}

/// Iterator over all directives in a config (recursively)
pub struct AllDirectivesIter<'a> {
    stack: Vec<std::slice::Iter<'a, ConfigItem>>,
}

impl<'a> Iterator for AllDirectivesIter<'a> {
    type Item = &'a Directive;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(iter) = self.stack.last_mut() {
            if let Some(item) = iter.next() {
                if let ConfigItem::Directive(directive) = item {
                    if let Some(block) = &directive.block {
                        self.stack.push(block.items.iter());
                    }
                    return Some(directive.as_ref());
                }
            } else {
                self.stack.pop();
            }
        }
        None
    }
}

/// A directive with its parent context information
#[derive(Debug, Clone)]
pub struct DirectiveWithContext<'a> {
    /// The directive itself
    pub directive: &'a Directive,
    /// Stack of parent directive names
    pub parent_stack: Vec<String>,
    /// Nesting depth
    pub depth: usize,
}

impl<'a> DirectiveWithContext<'a> {
    /// Get the immediate parent directive name, if any
    pub fn parent(&self) -> Option<&str> {
        self.parent_stack.last().map(|s| s.as_str())
    }

    /// Check if this directive is inside a specific parent context
    pub fn is_inside(&self, parent_name: &str) -> bool {
        self.parent_stack.iter().any(|p| p == parent_name)
    }

    /// Check if the immediate parent is a specific directive
    pub fn parent_is(&self, parent_name: &str) -> bool {
        self.parent() == Some(parent_name)
    }

    /// Check if this directive is at root level
    pub fn is_at_root(&self) -> bool {
        self.parent_stack.is_empty()
    }
}

/// Iterator over all directives with their parent context
pub struct AllDirectivesWithContextIter<'a> {
    stack: Vec<(std::slice::Iter<'a, ConfigItem>, Option<String>)>,
    current_parents: Vec<String>,
}

impl<'a> AllDirectivesWithContextIter<'a> {
    fn new(items: &'a [ConfigItem], initial_context: Vec<String>) -> Self {
        Self {
            stack: vec![(items.iter(), None)],
            current_parents: initial_context,
        }
    }
}

impl<'a> Iterator for AllDirectivesWithContextIter<'a> {
    type Item = DirectiveWithContext<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((iter, _)) = self.stack.last_mut() {
            if let Some(item) = iter.next() {
                if let ConfigItem::Directive(directive) = item {
                    let context = DirectiveWithContext {
                        directive: directive.as_ref(),
                        parent_stack: self.current_parents.clone(),
                        depth: self.current_parents.len(),
                    };

                    if let Some(block) = &directive.block {
                        self.current_parents.push(directive.name.clone());
                        self.stack
                            .push((block.items.iter(), Some(directive.name.clone())));
                    }

                    return Some(context);
                }
            } else {
                let (_, parent_name) = self.stack.pop().unwrap();
                if parent_name.is_some() {
                    self.current_parents.pop();
                }
            }
        }
        None
    }
}

/// Extension trait for Directive to provide helper methods
pub trait DirectiveExt {
    fn is(&self, name: &str) -> bool;
    fn first_arg(&self) -> Option<&str>;
    fn first_arg_is(&self, value: &str) -> bool;
    fn arg_at(&self, index: usize) -> Option<&str>;
    fn last_arg(&self) -> Option<&str>;
    fn has_arg(&self, value: &str) -> bool;
    fn arg_count(&self) -> usize;
    fn full_start_offset(&self) -> usize;
    fn replace_with(&self, new_text: &str) -> Fix;
    fn delete_line(&self) -> Fix;
    fn insert_after(&self, new_text: &str) -> Fix;
    fn insert_after_many(&self, lines: &[&str]) -> Fix;
    fn insert_before(&self, new_text: &str) -> Fix;
    fn insert_before_many(&self, lines: &[&str]) -> Fix;
}

impl DirectiveExt for Directive {
    fn is(&self, name: &str) -> bool {
        self.name == name
    }

    fn first_arg(&self) -> Option<&str> {
        self.args.first().map(|a| a.as_str())
    }

    fn first_arg_is(&self, value: &str) -> bool {
        self.first_arg() == Some(value)
    }

    fn arg_at(&self, index: usize) -> Option<&str> {
        self.args.get(index).map(|a| a.as_str())
    }

    fn last_arg(&self) -> Option<&str> {
        self.args.last().map(|a| a.as_str())
    }

    fn has_arg(&self, value: &str) -> bool {
        self.args.iter().any(|a| a.as_str() == value)
    }

    fn arg_count(&self) -> usize {
        self.args.len()
    }

    fn full_start_offset(&self) -> usize {
        self.span.start.offset - self.leading_whitespace.len()
    }

    fn replace_with(&self, new_text: &str) -> Fix {
        let start = self.full_start_offset();
        let end = self.span.end.offset;
        let fixed = format!("{}{}", self.leading_whitespace, new_text);
        Fix::replace_range(start, end, &fixed)
    }

    fn delete_line(&self) -> Fix {
        Fix::delete(self.span.start.line)
    }

    fn insert_after(&self, new_text: &str) -> Fix {
        self.insert_after_many(&[new_text])
    }

    fn insert_after_many(&self, lines: &[&str]) -> Fix {
        let indent = " ".repeat(self.span.start.column.saturating_sub(1));
        let fix_text: String = lines
            .iter()
            .map(|line| format!("\n{}{}", indent, line))
            .collect();
        let insert_offset = self.span.end.offset;
        Fix::replace_range(insert_offset, insert_offset, &fix_text)
    }

    fn insert_before(&self, new_text: &str) -> Fix {
        self.insert_before_many(&[new_text])
    }

    fn insert_before_many(&self, lines: &[&str]) -> Fix {
        let indent = " ".repeat(self.span.start.column.saturating_sub(1));
        let fix_text: String = lines
            .iter()
            .map(|line| format!("{}{}\n", indent, line))
            .collect();
        let line_start_offset = self.span.start.offset - (self.span.start.column - 1);
        Fix::replace_range(line_start_offset, line_start_offset, &fix_text)
    }
}

/// Extension trait for Argument to add source reconstruction
pub trait ArgumentExt {
    /// Reconstruct the source text for this argument
    fn to_source(&self) -> String;
}

impl ArgumentExt for Argument {
    fn to_source(&self) -> String {
        match &self.value {
            ArgumentValue::Literal(s) => s.clone(),
            ArgumentValue::QuotedString(s) => format!("\"{}\"", s),
            ArgumentValue::SingleQuotedString(s) => format!("'{}'", s),
            ArgumentValue::Variable(s) => format!("${}", s),
        }
    }
}
