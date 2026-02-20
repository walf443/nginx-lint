//! Core types for plugin development.
//!
//! This module provides the fundamental types needed to build nginx-lint plugins:
//!
//! - [`Plugin`] - The trait every plugin must implement
//! - [`PluginSpec`] - Plugin metadata (name, category, description, examples)
//! - [`LintError`] - Lint errors reported by plugins
//! - [`ErrorBuilder`] - Helper for creating errors with pre-filled rule/category
//! - [`Fix`] - Autofix actions (replace, delete, insert)
//! - [`ConfigExt`] - Extension trait for traversing the nginx config AST
//! - [`DirectiveExt`] - Extension trait for inspecting and modifying directives
//! - [`DirectiveWithContext`] - A directive paired with its parent block context
//!
//! These types mirror the nginx-lint AST and error types for use in WASM plugins.

use serde::{Deserialize, Serialize};

/// Current API version for the plugin SDK
pub const API_VERSION: &str = "1.0";

/// Plugin metadata describing a lint rule.
///
/// Created via [`PluginSpec::new()`] and configured with builder methods.
///
/// # Example
///
/// ```
/// use nginx_lint_plugin::PluginSpec;
///
/// let spec = PluginSpec::new("my-rule", "security", "Short description")
///     .with_severity("warning")
///     .with_why("Detailed explanation of why this rule exists.")
///     .with_bad_example("server {\n    bad_directive on;\n}")
///     .with_good_example("server {\n    bad_directive off;\n}")
///     .with_references(vec![
///         "https://nginx.org/en/docs/...".to_string(),
///     ]);
///
/// assert_eq!(spec.name, "my-rule");
/// assert_eq!(spec.category, "security");
/// assert_eq!(spec.severity, Some("warning".to_string()));
/// ```
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
    /// ```
    /// use nginx_lint_plugin::{PluginSpec, Severity};
    ///
    /// let spec = PluginSpec::new("my-rule", "security", "Check something");
    /// let err = spec.error_builder();
    ///
    /// // Instead of:
    /// //   LintError::warning("my-rule", "security", "message", 10, 5)
    /// // Use:
    /// let warning = err.warning("message", 10, 5);
    /// assert_eq!(warning.rule, "my-rule");
    /// assert_eq!(warning.category, "security");
    /// assert_eq!(warning.severity, Severity::Warning);
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
    pub fn error_at(&self, message: &str, directive: &(impl DirectiveExt + ?Sized)) -> LintError {
        self.error(message, directive.line(), directive.column())
    }

    /// Create a warning from a directive's location
    pub fn warning_at(&self, message: &str, directive: &(impl DirectiveExt + ?Sized)) -> LintError {
        self.warning(message, directive.line(), directive.column())
    }
}

/// Severity level for lint errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// Represents a fix that can be applied to automatically resolve a lint error.
///
/// Fixes can operate in two modes:
///
/// - **Line-based**: Operate on entire lines (delete, insert after, replace text on a line)
/// - **Range-based**: Operate on byte offsets for precise edits (multiple fixes per line)
///
/// Use the convenience methods on [`DirectiveExt`] to create fixes from directives:
///
/// ```
/// use nginx_lint_plugin::prelude::*;
///
/// let config = nginx_lint_plugin::parse_string("server_tokens on;").unwrap();
/// let directive = config.all_directives().next().unwrap();
///
/// // Replace the entire directive
/// let fix = directive.replace_with("server_tokens off;");
/// assert!(fix.is_range_based());
///
/// // Delete the directive's line
/// let fix = directive.delete_line();
/// assert!(fix.delete_line);
///
/// // Insert a new line after the directive
/// let fix = directive.insert_after("add_header X-Frame-Options DENY;");
/// assert!(fix.is_range_based());
///
/// // Insert before the directive
/// let fix = directive.insert_before("# Security headers");
/// assert!(fix.is_range_based());
/// ```
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

/// A lint error reported by a plugin.
///
/// Create errors using [`LintError::error()`] / [`LintError::warning()`] directly,
/// or more conveniently via [`ErrorBuilder`] (obtained from [`PluginSpec::error_builder()`]):
///
/// ```
/// use nginx_lint_plugin::prelude::*;
///
/// let spec = PluginSpec::new("my-rule", "security", "Check something");
/// let err = spec.error_builder();
///
/// // Warning at a specific line/column
/// let warning = err.warning("message", 10, 5);
/// assert_eq!(warning.line, Some(10));
///
/// // Warning at a directive's location (most common pattern)
/// let config = nginx_lint_plugin::parse_string("autoindex on;").unwrap();
/// let directive = config.all_directives().next().unwrap();
/// let error = err.warning_at("use 'off'", directive)
///     .with_fix(directive.replace_with("autoindex off;"));
/// assert_eq!(error.fixes.len(), 1);
/// ```
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

/// Trait that all plugins must implement.
///
/// A plugin consists of two parts:
/// - **Metadata** ([`spec()`](Plugin::spec)) describing the rule name, category, severity, and documentation
/// - **Logic** ([`check()`](Plugin::check)) that inspects the parsed nginx config and reports errors
///
/// Plugins must also derive [`Default`], which is used by [`export_plugin!`](crate::export_plugin)
/// to instantiate the plugin.
///
/// # Example
///
/// ```
/// use nginx_lint_plugin::prelude::*;
///
/// #[derive(Default)]
/// pub struct MyPlugin;
///
/// impl Plugin for MyPlugin {
///     fn spec(&self) -> PluginSpec {
///         PluginSpec::new("my-rule", "security", "Check for something")
///             .with_severity("warning")
///     }
///
///     fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
///         let mut errors = Vec::new();
///         let err = self.spec().error_builder();
///
///         for ctx in config.all_directives_with_context() {
///             if ctx.is_inside("http") && ctx.directive.is("bad_directive") {
///                 errors.push(err.warning_at("Avoid bad_directive", ctx.directive));
///             }
///         }
///         errors
///     }
/// }
///
/// // export_plugin!(MyPlugin);  // Required for WASM build
///
/// // Verify it works
/// let plugin = MyPlugin;
/// let config = nginx_lint_plugin::parse_string("http { bad_directive on; }").unwrap();
/// let errors = plugin.check(&config, "test.conf");
/// assert_eq!(errors.len(), 1);
/// ```
pub trait Plugin: Default {
    /// Return plugin metadata.
    ///
    /// This is called once at plugin load time. Use [`PluginSpec::new()`] to create
    /// the spec, then chain builder methods like [`with_severity()`](PluginSpec::with_severity),
    /// [`with_why()`](PluginSpec::with_why), [`with_bad_example()`](PluginSpec::with_bad_example), etc.
    fn spec(&self) -> PluginSpec;

    /// Check the configuration and return any lint errors.
    ///
    /// Called once per file being linted. The `config` parameter contains the parsed
    /// AST of the nginx configuration file. The `path` parameter is the file path
    /// being checked (useful for error messages).
    ///
    /// Use [`config.all_directives()`](ConfigExt::all_directives) for simple iteration
    /// or [`config.all_directives_with_context()`](ConfigExt::all_directives_with_context)
    /// when you need to know the parent block context.
    fn check(&self, config: &Config, path: &str) -> Vec<LintError>;
}

// Re-export AST types from nginx-lint-common
pub use nginx_lint_common::parser::ast::{
    Argument, ArgumentValue, Block, Comment, Config, ConfigItem, Directive, Position, Span,
};
pub use nginx_lint_common::parser::context::{AllDirectivesWithContextIter, DirectiveWithContext};

/// Extension trait for [`Config`] providing iteration and include-context helpers.
///
/// This trait is automatically available when using `use nginx_lint_plugin::prelude::*`.
///
/// # Traversal
///
/// Two traversal methods are provided:
///
/// - [`all_directives()`](ConfigExt::all_directives) - Simple recursive iteration over all directives
/// - [`all_directives_with_context()`](ConfigExt::all_directives_with_context) - Iteration with
///   parent block context (e.g., know if a directive is inside `http`, `server`, `location`)
///
/// # Include Context
///
/// When nginx-lint processes `include` directives, the included file's [`Config`] receives
/// an `include_context` field recording the parent block names. For example, a file included
/// from `http { server { include conf.d/*.conf; } }` would have
/// `include_context = ["http", "server"]`.
///
/// The `is_included_from_*` methods check this context:
///
/// ```
/// use nginx_lint_plugin::prelude::*;
///
/// let mut config = nginx_lint_plugin::parse_string("server { listen 80; }").unwrap();
/// assert!(!config.is_included_from_http());
///
/// // Simulate being included from http context
/// config.include_context = vec!["http".to_string()];
/// assert!(config.is_included_from_http());
/// ```
pub trait ConfigExt {
    /// Iterate over all directives recursively.
    ///
    /// Traverses the entire config tree depth-first, yielding each [`Directive`].
    fn all_directives(&self) -> nginx_lint_common::parser::ast::AllDirectives<'_>;

    /// Iterate over all directives with parent context information.
    ///
    /// Each item is a [`DirectiveWithContext`] that includes the parent block stack.
    /// This is the recommended traversal method for most plugins, as it allows
    /// checking whether a directive is inside a specific block (e.g., `http`, `server`).
    fn all_directives_with_context(&self) -> AllDirectivesWithContextIter<'_>;

    /// Check if this config is included from within a specific context.
    fn is_included_from(&self, context: &str) -> bool;

    /// Check if this config is included from within `http` context.
    fn is_included_from_http(&self) -> bool;

    /// Check if this config is included from within `http > server` context.
    fn is_included_from_http_server(&self) -> bool;

    /// Check if this config is included from within `http > ... > location` context.
    fn is_included_from_http_location(&self) -> bool;

    /// Check if this config is included from within `stream` context.
    fn is_included_from_stream(&self) -> bool;

    /// Get the immediate parent context (last element in include_context).
    fn immediate_parent_context(&self) -> Option<&str>;
}

impl ConfigExt for Config {
    fn all_directives(&self) -> nginx_lint_common::parser::ast::AllDirectives<'_> {
        // Delegate to Config's inherent method
        Config::all_directives(self)
    }

    fn all_directives_with_context(&self) -> AllDirectivesWithContextIter<'_> {
        // Delegate to Config's inherent method
        Config::all_directives_with_context(self)
    }

    fn is_included_from(&self, context: &str) -> bool {
        Config::is_included_from(self, context)
    }

    fn is_included_from_http(&self) -> bool {
        Config::is_included_from_http(self)
    }

    fn is_included_from_http_server(&self) -> bool {
        Config::is_included_from_http_server(self)
    }

    fn is_included_from_http_location(&self) -> bool {
        Config::is_included_from_http_location(self)
    }

    fn is_included_from_stream(&self) -> bool {
        Config::is_included_from_stream(self)
    }

    fn immediate_parent_context(&self) -> Option<&str> {
        Config::immediate_parent_context(self)
    }
}

/// Extension trait for [`Directive`] providing inspection and fix-generation helpers.
///
/// This trait adds convenience methods to [`Directive`] for:
/// - **Inspection**: [`is()`](DirectiveExt::is), [`first_arg()`](DirectiveExt::first_arg),
///   [`has_arg()`](DirectiveExt::has_arg), etc.
/// - **Fix generation**: [`replace_with()`](DirectiveExt::replace_with),
///   [`delete_line()`](DirectiveExt::delete_line), [`insert_after()`](DirectiveExt::insert_after), etc.
///
/// # Example
///
/// ```
/// use nginx_lint_plugin::prelude::*;
///
/// let config = nginx_lint_plugin::parse_string(
///     "proxy_pass http://backend;"
/// ).unwrap();
/// let directive = config.all_directives().next().unwrap();
///
/// assert!(directive.is("proxy_pass"));
/// assert_eq!(directive.first_arg(), Some("http://backend"));
/// assert_eq!(directive.arg_count(), 1);
///
/// // Generate a fix to replace the directive
/// let fix = directive.replace_with("proxy_pass http://new-backend;");
/// assert!(fix.is_range_based());
/// ```
pub trait DirectiveExt {
    /// Check if the directive has the given name.
    fn is(&self, name: &str) -> bool;
    /// Get the first argument's string value, if any.
    fn first_arg(&self) -> Option<&str>;
    /// Check if the first argument equals the given value.
    fn first_arg_is(&self, value: &str) -> bool;
    /// Get the argument at the given index.
    fn arg_at(&self, index: usize) -> Option<&str>;
    /// Get the last argument's string value, if any.
    fn last_arg(&self) -> Option<&str>;
    /// Check if any argument equals the given value.
    fn has_arg(&self, value: &str) -> bool;
    /// Return the number of arguments.
    fn arg_count(&self) -> usize;
    /// Get the start line number (1-based).
    fn line(&self) -> usize;
    /// Get the start column number (1-based).
    fn column(&self) -> usize;
    /// Get the byte offset including leading whitespace.
    fn full_start_offset(&self) -> usize;
    /// Create a [`Fix`] that replaces this directive with new text, preserving indentation.
    fn replace_with(&self, new_text: &str) -> Fix;
    /// Create a [`Fix`] that deletes this directive's line.
    fn delete_line(&self) -> Fix;
    /// Create a [`Fix`] that inserts a new line after this directive, matching indentation.
    fn insert_after(&self, new_text: &str) -> Fix;
    /// Create a [`Fix`] that inserts multiple new lines after this directive.
    fn insert_after_many(&self, lines: &[&str]) -> Fix;
    /// Create a [`Fix`] that inserts a new line before this directive, matching indentation.
    fn insert_before(&self, new_text: &str) -> Fix;
    /// Create a [`Fix`] that inserts multiple new lines before this directive.
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

    fn line(&self) -> usize {
        self.span.start.line
    }

    fn column(&self) -> usize {
        self.span.start.column
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

impl<T: DirectiveExt + ?Sized> DirectiveExt for &T {
    fn is(&self, name: &str) -> bool {
        (**self).is(name)
    }
    fn first_arg(&self) -> Option<&str> {
        (**self).first_arg()
    }
    fn first_arg_is(&self, value: &str) -> bool {
        (**self).first_arg_is(value)
    }
    fn arg_at(&self, index: usize) -> Option<&str> {
        (**self).arg_at(index)
    }
    fn last_arg(&self) -> Option<&str> {
        (**self).last_arg()
    }
    fn has_arg(&self, value: &str) -> bool {
        (**self).has_arg(value)
    }
    fn arg_count(&self) -> usize {
        (**self).arg_count()
    }
    fn line(&self) -> usize {
        (**self).line()
    }
    fn column(&self) -> usize {
        (**self).column()
    }
    fn full_start_offset(&self) -> usize {
        (**self).full_start_offset()
    }
    fn replace_with(&self, new_text: &str) -> Fix {
        (**self).replace_with(new_text)
    }
    fn delete_line(&self) -> Fix {
        (**self).delete_line()
    }
    fn insert_after(&self, new_text: &str) -> Fix {
        (**self).insert_after(new_text)
    }
    fn insert_after_many(&self, lines: &[&str]) -> Fix {
        (**self).insert_after_many(lines)
    }
    fn insert_before(&self, new_text: &str) -> Fix {
        (**self).insert_before(new_text)
    }
    fn insert_before_many(&self, lines: &[&str]) -> Fix {
        (**self).insert_before_many(lines)
    }
}

impl DirectiveExt for Box<Directive> {
    fn is(&self, name: &str) -> bool {
        (**self).is(name)
    }
    fn first_arg(&self) -> Option<&str> {
        (**self).first_arg()
    }
    fn first_arg_is(&self, value: &str) -> bool {
        (**self).first_arg_is(value)
    }
    fn arg_at(&self, index: usize) -> Option<&str> {
        (**self).arg_at(index)
    }
    fn last_arg(&self) -> Option<&str> {
        (**self).last_arg()
    }
    fn has_arg(&self, value: &str) -> bool {
        (**self).has_arg(value)
    }
    fn arg_count(&self) -> usize {
        (**self).arg_count()
    }
    fn line(&self) -> usize {
        (**self).line()
    }
    fn column(&self) -> usize {
        (**self).column()
    }
    fn full_start_offset(&self) -> usize {
        (**self).full_start_offset()
    }
    fn replace_with(&self, new_text: &str) -> Fix {
        (**self).replace_with(new_text)
    }
    fn delete_line(&self) -> Fix {
        (**self).delete_line()
    }
    fn insert_after(&self, new_text: &str) -> Fix {
        (**self).insert_after(new_text)
    }
    fn insert_after_many(&self, lines: &[&str]) -> Fix {
        (**self).insert_after_many(lines)
    }
    fn insert_before(&self, new_text: &str) -> Fix {
        (**self).insert_before(new_text)
    }
    fn insert_before_many(&self, lines: &[&str]) -> Fix {
        (**self).insert_before_many(lines)
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
