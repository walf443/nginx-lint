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
}

/// Severity level for lint errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A lint error reported by a plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintError {
    pub rule: String,
    pub category: String,
    pub message: String,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
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
        }
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
}

impl ConfigExt for Config {
    fn all_directives(&self) -> AllDirectivesIter<'_> {
        AllDirectivesIter {
            stack: vec![self.items.iter()],
        }
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

/// Extension trait for Directive to provide helper methods
pub trait DirectiveExt {
    /// Check if this directive has a specific name
    fn is(&self, name: &str) -> bool;

    /// Get the first argument value as a string
    fn first_arg(&self) -> Option<&str>;

    /// Check if the first argument equals a specific value
    fn first_arg_is(&self, value: &str) -> bool;
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
}

/// Extension trait for Argument
pub trait ArgumentExt {
    /// Get the string value (without quotes for quoted strings)
    fn as_str(&self) -> &str;

    /// Check if this is an "on" value
    fn is_on(&self) -> bool;

    /// Check if this is an "off" value
    fn is_off(&self) -> bool;
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
}
