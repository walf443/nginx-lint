//! Types for plugin development
//!
//! These types mirror the nginx-lint AST and error types for use in WASM plugins.

use serde::{Deserialize, Serialize};

/// Current API version for the plugin SDK
pub const API_VERSION: &str = "1.0";

/// Plugin metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    /// Unique name for the rule (e.g., "my-custom-rule")
    pub name: String,
    /// Category (e.g., "security", "style", "best_practices", "custom")
    pub category: String,
    /// Human-readable description
    pub description: String,
    /// API version the plugin uses for input/output format
    pub api_version: String,
    /// Severity level (error, warning, info)
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

impl PluginInfo {
    /// Create a new PluginInfo with the current API version
    pub fn new(name: impl Into<String>, category: impl Into<String>, description: impl Into<String>) -> Self {
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
    ///     let info = self.info();
    ///     let error = info.error_builder();
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

    /// Create an error with Info severity
    pub fn info(&self, message: &str, line: usize, column: usize) -> LintError {
        LintError::info(&self.rule, &self.category, message, line, column)
    }

    /// Create an error from a directive's location
    pub fn error_at(&self, message: &str, directive: &Directive) -> LintError {
        self.error(message, directive.span.start.line, directive.span.start.column)
    }

    /// Create a warning from a directive's location
    pub fn warning_at(&self, message: &str, directive: &Directive) -> LintError {
        self.warning(message, directive.span.start.line, directive.span.start.column)
    }

    /// Create an info from a directive's location
    pub fn info_at(&self, message: &str, directive: &Directive) -> LintError {
        self.info(message, directive.span.start.line, directive.span.start.column)
    }
}

/// Severity level for lint errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
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
            fix: None,
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
            fix: None,
        }
    }

    /// Create a new error with Info severity
    pub fn info(rule: &str, category: &str, message: &str, line: usize, column: usize) -> Self {
        Self {
            rule: rule.to_string(),
            category: category.to_string(),
            message: message.to_string(),
            severity: Severity::Info,
            line: if line > 0 { Some(line) } else { None },
            column: if column > 0 { Some(column) } else { None },
            fix: None,
        }
    }

    /// Attach a fix to this error
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }
}

/// Trait that plugins must implement
pub trait Plugin: Default {
    /// Return plugin metadata
    fn info(&self) -> PluginInfo;

    /// Check the configuration and return any lint errors
    fn check(&self, config: &Config, path: &str) -> Vec<LintError>;
}

// Re-export AST types from the parser module
pub use crate::parser::ast::{
    Argument, ArgumentValue, Block, Comment, Config, ConfigItem, Directive, Position, Span,
};

/// Extension trait for Config to provide helper methods
pub trait ConfigExt {
    /// Iterate over all directives recursively
    fn all_directives(&self) -> AllDirectivesIter<'_>;

    /// Iterate over all directives recursively with parent context information
    ///
    /// This is useful for rules that need to know the parent context of each directive.
    /// The `include_context` from the Config is used as the initial parent stack.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for ctx in config.all_directives_with_context() {
    ///     if ctx.directive.is("proxy_pass") && !ctx.parent_stack.contains(&"location") {
    ///         // proxy_pass outside of location
    ///     }
    /// }
    /// ```
    fn all_directives_with_context(&self) -> AllDirectivesWithContextIter<'_>;

    /// Check if this config is included from within a specific context
    ///
    /// This is useful for rules that only apply within certain contexts (like http).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Check if this file was included from http context
    /// let in_http_include = config.is_included_from("http");
    /// ```
    fn is_included_from(&self, context: &str) -> bool;

    /// Check if this config is included from within http context
    ///
    /// Shorthand for `config.is_included_from("http")`
    fn is_included_from_http(&self) -> bool;

    /// Check if this config is included from within http > server context
    ///
    /// This checks that both "http" and "server" are in the include_context,
    /// with "http" appearing before "server".
    fn is_included_from_http_server(&self) -> bool;

    /// Check if this config is included from within http > server > location context
    ///
    /// This checks that "http", "server", and "location" are in the include_context
    /// in the correct order.
    fn is_included_from_http_location(&self) -> bool;

    /// Check if this config is included from within stream context
    ///
    /// Shorthand for `config.is_included_from("stream")`
    fn is_included_from_stream(&self) -> bool;

    /// Get the immediate parent context (last element in include_context)
    ///
    /// This is useful for rules that need to know the direct parent context
    /// of the included file, such as duplicate-directive checking.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // If include_context is ["http", "server"], returns Some("server")
    /// let parent = config.immediate_parent_context();
    /// ```
    fn immediate_parent_context(&self) -> Option<&str>;
}

impl ConfigExt for Config {
    fn all_directives(&self) -> AllDirectivesIter<'_> {
        AllDirectivesIter {
            stack: vec![self.items.iter()],
        }
    }

    fn all_directives_with_context(&self) -> AllDirectivesWithContextIter<'_> {
        // Use include_context as the initial parent stack
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
    /// Stack of parent directive names (e.g., ["http", "server", "location"])
    /// This includes the include_context from the Config if the file was included
    pub parent_stack: Vec<String>,
    /// Nesting depth (0 = root level, 1 = inside one block, etc.)
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

    /// Check if this directive is at root level (no parent directives)
    /// Note: This checks the actual parent_stack, which may include include_context
    pub fn is_at_root(&self) -> bool {
        self.parent_stack.is_empty()
    }
}

/// Iterator over all directives with their parent context
pub struct AllDirectivesWithContextIter<'a> {
    /// Stack of (iterator, parent_name) pairs
    /// parent_name is the name of the directive that contains this iterator's items
    stack: Vec<(std::slice::Iter<'a, ConfigItem>, Option<String>)>,
    /// Current parent stack (built up as we descend)
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
                    // Capture current context before potentially pushing
                    let context = DirectiveWithContext {
                        directive: directive.as_ref(),
                        parent_stack: self.current_parents.clone(),
                        depth: self.current_parents.len(),
                    };

                    // If this directive has a block, push it for later traversal
                    if let Some(block) = &directive.block {
                        self.current_parents.push(directive.name.clone());
                        self.stack
                            .push((block.items.iter(), Some(directive.name.clone())));
                    }

                    return Some(context);
                }
            } else {
                // Current iterator exhausted, pop it
                let (_, parent_name) = self.stack.pop().unwrap();
                // If we're leaving a block, pop the parent from current_parents
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
    /// Check if this directive has a specific name
    fn is(&self, name: &str) -> bool;

    /// Get the first argument value as a string
    fn first_arg(&self) -> Option<&str>;

    /// Check if the first argument equals a specific value
    fn first_arg_is(&self, value: &str) -> bool;

    /// Get the argument at a specific index (0-indexed)
    fn arg_at(&self, index: usize) -> Option<&str>;

    /// Get the last argument value as a string
    fn last_arg(&self) -> Option<&str>;

    /// Check if the directive has an argument with the given value
    fn has_arg(&self, value: &str) -> bool;

    /// Get the number of arguments
    fn arg_count(&self) -> usize;

    /// Get the start offset including leading whitespace (for Fix calculations)
    ///
    /// This is commonly used when creating range-based fixes:
    /// ```rust,ignore
    /// let start = directive.full_start_offset();
    /// let end = directive.span.end.offset;
    /// let fixed = format!("{}new_directive;", directive.leading_whitespace);
    /// Fix::replace_range(start, end, &fixed)
    /// ```
    fn full_start_offset(&self) -> usize;

    /// Create a range-based fix that replaces this entire directive (including leading whitespace)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Replace "server_tokens on;" with "server_tokens off;"
    /// let fix = directive.replace_with("server_tokens off;");
    /// ```
    fn replace_with(&self, new_text: &str) -> Fix;

    /// Create a fix that deletes this entire directive line
    fn delete_line(&self) -> Fix;

    /// Create a fix that inserts a new directive after this one
    ///
    /// The new directive will be inserted on a new line with the same indentation
    /// as this directive.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // After "proxy_http_version 1.1;", insert "proxy_set_header Connection "";"
    /// let fix = directive.insert_after("proxy_set_header Connection \"\";");
    /// ```
    fn insert_after(&self, new_text: &str) -> Fix;

    /// Create a fix that inserts multiple directives after this one
    ///
    /// Each directive will be inserted on its own line with the same indentation
    /// as this directive.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let fix = directive.insert_after_many(&[
    ///     "proxy_set_header Connection \"\";",
    ///     "proxy_set_header Upgrade $http_upgrade;",
    /// ]);
    /// ```
    fn insert_after_many(&self, lines: &[&str]) -> Fix;

    /// Create a fix that inserts a new directive before this one
    ///
    /// The new directive will be inserted on a new line before this directive,
    /// with the same indentation.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Before "add_header X-Custom value;", insert "add_header X-Frame-Options DENY;"
    /// let fix = directive.insert_before("add_header X-Frame-Options DENY;");
    /// ```
    fn insert_before(&self, new_text: &str) -> Fix;

    /// Create a fix that inserts multiple directives before this one
    ///
    /// Each directive will be inserted on its own line with the same indentation
    /// as this directive.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Insert multiple headers before the first add_header
    /// let fix = directive.insert_before_many(&[
    ///     "add_header X-Frame-Options DENY;",
    ///     "add_header X-Content-Type-Options nosniff;",
    /// ]);
    /// ```
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
        // Calculate the indentation from the directive's column position
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
        // Calculate the indentation from the directive's column position
        let indent = " ".repeat(self.span.start.column.saturating_sub(1));
        let fix_text: String = lines
            .iter()
            .map(|line| format!("{}{}\n", indent, line))
            .collect();
        // Insert at the beginning of the line (before the indentation)
        let line_start_offset = self.span.start.offset - (self.span.start.column - 1);
        Fix::replace_range(line_start_offset, line_start_offset, &fix_text)
    }
}

/// Extension trait for Argument
pub trait ArgumentExt {
    /// Get the string value (without quotes for quoted strings)
    fn as_str(&self) -> &str;

    /// Check if this is an "on" value
    fn is_on(&self) -> bool;

    /// Check if this is an "off" value
    fn is_off(&self) -> bool;

    /// Check if this is a variable (e.g., $host, $request_uri)
    fn is_variable(&self) -> bool;

    /// Check if this is a quoted string (double or single quotes)
    fn is_quoted(&self) -> bool;

    /// Check if this is a literal value (unquoted, non-variable)
    fn is_literal(&self) -> bool;
}

impl ArgumentExt for Argument {
    fn as_str(&self) -> &str {
        match &self.value {
            ArgumentValue::Literal(s) => s,
            ArgumentValue::QuotedString(s) => s,
            ArgumentValue::SingleQuotedString(s) => s,
            ArgumentValue::Variable(s) => s,
        }
    }

    fn is_on(&self) -> bool {
        self.as_str() == "on"
    }

    fn is_off(&self) -> bool {
        self.as_str() == "off"
    }

    fn is_variable(&self) -> bool {
        matches!(self.value, ArgumentValue::Variable(_))
    }

    fn is_quoted(&self) -> bool {
        matches!(
            self.value,
            ArgumentValue::QuotedString(_) | ArgumentValue::SingleQuotedString(_)
        )
    }

    fn is_literal(&self) -> bool {
        matches!(self.value, ArgumentValue::Literal(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_string;

    #[test]
    fn test_all_directives_with_context_basic() {
        let config = parse_string(
            r#"
http {
    server {
        listen 80;
        location / {
            root /var/www;
        }
    }
}
"#,
        )
        .unwrap();

        let contexts: Vec<_> = config.all_directives_with_context().collect();

        // http - no parent
        assert_eq!(contexts[0].directive.name, "http");
        assert!(contexts[0].parent_stack.is_empty());
        assert_eq!(contexts[0].depth, 0);
        assert!(contexts[0].is_at_root());

        // server - parent is http
        assert_eq!(contexts[1].directive.name, "server");
        assert_eq!(contexts[1].parent_stack, vec!["http"]);
        assert_eq!(contexts[1].depth, 1);
        assert!(contexts[1].is_inside("http"));
        assert!(contexts[1].parent_is("http"));

        // listen - parent is server
        assert_eq!(contexts[2].directive.name, "listen");
        assert_eq!(contexts[2].parent_stack, vec!["http", "server"]);
        assert_eq!(contexts[2].depth, 2);
        assert!(contexts[2].is_inside("http"));
        assert!(contexts[2].is_inside("server"));
        assert!(contexts[2].parent_is("server"));

        // location - parent is server
        assert_eq!(contexts[3].directive.name, "location");
        assert_eq!(contexts[3].parent_stack, vec!["http", "server"]);
        assert_eq!(contexts[3].depth, 2);

        // root - parent is location
        assert_eq!(contexts[4].directive.name, "root");
        assert_eq!(contexts[4].parent_stack, vec!["http", "server", "location"]);
        assert_eq!(contexts[4].depth, 3);
        assert!(contexts[4].is_inside("location"));
        assert!(contexts[4].parent_is("location"));
    }

    #[test]
    fn test_all_directives_with_context_include_context() {
        // Simulate a file included from server context
        let mut config = parse_string(
            r#"
location / {
    root /var/www;
}
"#,
        )
        .unwrap();

        config.include_context = vec!["http".to_string(), "server".to_string()];

        let contexts: Vec<_> = config.all_directives_with_context().collect();

        // location - parent is server (from include_context)
        assert_eq!(contexts[0].directive.name, "location");
        assert_eq!(contexts[0].parent_stack, vec!["http", "server"]);
        assert_eq!(contexts[0].depth, 2);
        assert!(contexts[0].is_inside("server"));
        assert!(contexts[0].parent_is("server"));

        // root - parent is location
        assert_eq!(contexts[1].directive.name, "root");
        assert_eq!(
            contexts[1].parent_stack,
            vec!["http", "server", "location"]
        );
        assert_eq!(contexts[1].depth, 3);
    }

    #[test]
    fn test_all_directives_with_context_helper_methods() {
        let config = parse_string(
            r#"
http {
    upstream backend {
        server 127.0.0.1:8080;
    }
    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let contexts: Vec<_> = config.all_directives_with_context().collect();

        // Find proxy_pass
        let proxy_pass = contexts
            .iter()
            .find(|c| c.directive.name == "proxy_pass")
            .unwrap();

        assert!(proxy_pass.is_inside("location"));
        assert!(proxy_pass.is_inside("server"));
        assert!(proxy_pass.is_inside("http"));
        assert!(!proxy_pass.is_inside("upstream"));
        assert!(proxy_pass.parent_is("location"));
        assert!(!proxy_pass.parent_is("server"));

        // Find server inside upstream
        let upstream_server = contexts
            .iter()
            .find(|c| c.directive.name == "server" && c.parent_is("upstream"))
            .unwrap();

        assert!(upstream_server.is_inside("upstream"));
        assert!(!upstream_server.is_inside("server"));
    }

    #[test]
    fn test_all_directives_with_context_empty_config() {
        let config = parse_string("").unwrap();
        let contexts: Vec<_> = config.all_directives_with_context().collect();
        assert!(contexts.is_empty());
    }

    #[test]
    fn test_all_directives_with_context_flat_config() {
        let config = parse_string(
            r#"
worker_processes auto;
error_log /var/log/nginx/error.log;
"#,
        )
        .unwrap();

        let contexts: Vec<_> = config.all_directives_with_context().collect();

        assert_eq!(contexts.len(), 2);
        assert!(contexts[0].is_at_root());
        assert!(contexts[1].is_at_root());
        assert_eq!(contexts[0].depth, 0);
        assert_eq!(contexts[1].depth, 0);
    }

    #[test]
    fn test_config_is_included_from() {
        let mut config = parse_string("server { listen 80; }").unwrap();
        config.include_context = vec!["http".to_string()];

        assert!(config.is_included_from("http"));
        assert!(config.is_included_from_http());
        assert!(!config.is_included_from("server"));
        assert!(!config.is_included_from_http_server());
    }

    #[test]
    fn test_config_is_included_from_http_server() {
        let mut config = parse_string("location / { root /var/www; }").unwrap();
        config.include_context = vec!["http".to_string(), "server".to_string()];

        assert!(config.is_included_from_http());
        assert!(config.is_included_from_http_server());
        assert!(!config.is_included_from_http_location());
    }

    #[test]
    fn test_config_is_included_from_http_location() {
        let mut config = parse_string("proxy_pass http://backend;").unwrap();
        config.include_context = vec!["http".to_string(), "server".to_string(), "location".to_string()];

        assert!(config.is_included_from_http());
        assert!(config.is_included_from_http_server());
        assert!(config.is_included_from_http_location());
    }

    #[test]
    fn test_config_is_included_from_stream() {
        let mut config = parse_string("server { listen 12345; }").unwrap();
        config.include_context = vec!["stream".to_string()];

        assert!(config.is_included_from_stream());
        assert!(!config.is_included_from_http());
        assert!(!config.is_included_from_http_server());
    }

    #[test]
    fn test_directive_ext_helpers() {
        let config = parse_string("proxy_pass http://backend last;").unwrap();
        let directive = config.all_directives().next().unwrap();

        assert!(directive.is("proxy_pass"));
        assert!(!directive.is("server"));

        assert_eq!(directive.first_arg(), Some("http://backend"));
        assert_eq!(directive.arg_at(0), Some("http://backend"));
        assert_eq!(directive.arg_at(1), Some("last"));
        assert_eq!(directive.arg_at(2), None);
        assert_eq!(directive.last_arg(), Some("last"));

        assert!(directive.has_arg("last"));
        assert!(!directive.has_arg("break"));

        assert_eq!(directive.arg_count(), 2);
    }

    #[test]
    fn test_directive_replace_with() {
        let config = parse_string("    server_tokens on;").unwrap();
        let directive = config.all_directives().next().unwrap();

        let fix = directive.replace_with("server_tokens off;");
        assert!(fix.is_range_based());
        assert_eq!(fix.new_text, "    server_tokens off;");
    }

    #[test]
    fn test_error_builder() {
        let info = PluginInfo::new("test-rule", "security", "Test rule");
        let builder = info.error_builder();

        let error = builder.warning("test message", 10, 5);
        assert_eq!(error.rule, "test-rule");
        assert_eq!(error.category, "security");
        assert_eq!(error.message, "test message");
        assert_eq!(error.line, Some(10));
        assert_eq!(error.column, Some(5));
    }

    #[test]
    fn test_error_builder_at_directive() {
        let config = parse_string("server_tokens on;").unwrap();
        let directive = config.all_directives().next().unwrap();

        let info = PluginInfo::new("test-rule", "security", "Test rule");
        let builder = info.error_builder();

        let error = builder.warning_at("test message", directive);
        assert_eq!(error.rule, "test-rule");
        assert_eq!(error.line, Some(1));
        assert_eq!(error.column, Some(1));
    }

    #[test]
    fn test_argument_is_variable() {
        let config = parse_string("proxy_set_header Host $host;").unwrap();
        let directive = config.all_directives().next().unwrap();

        // First arg "Host" is literal
        assert!(directive.args[0].is_literal());
        assert!(!directive.args[0].is_variable());
        assert!(!directive.args[0].is_quoted());

        // Second arg "$host" is variable
        assert!(directive.args[1].is_variable());
        assert!(!directive.args[1].is_literal());
        assert!(!directive.args[1].is_quoted());
    }

    #[test]
    fn test_argument_is_quoted() {
        let config = parse_string(r#"add_header X-Custom "value with spaces";"#).unwrap();
        let directive = config.all_directives().next().unwrap();

        // First arg "X-Custom" is literal
        assert!(directive.args[0].is_literal());
        assert!(!directive.args[0].is_quoted());

        // Second arg is quoted
        assert!(directive.args[1].is_quoted());
        assert!(!directive.args[1].is_literal());
        assert!(!directive.args[1].is_variable());
        assert_eq!(directive.args[1].as_str(), "value with spaces");
    }

    #[test]
    fn test_argument_single_quoted() {
        let config = parse_string("add_header X-Custom 'single quoted';").unwrap();
        let directive = config.all_directives().next().unwrap();

        assert!(directive.args[1].is_quoted());
        assert_eq!(directive.args[1].as_str(), "single quoted");
    }
}
