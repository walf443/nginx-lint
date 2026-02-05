use crate::config::LintConfig;
use crate::parser::ast::Config;
#[cfg(feature = "cli")]
use rayon::prelude::*;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "ERROR"),
            Severity::Warning => write!(f, "WARNING"),
            Severity::Info => write!(f, "INFO"),
        }
    }
}

/// Represents a fix that can be applied to resolve a lint error
#[derive(Debug, Clone, Serialize)]
pub struct Fix {
    /// Line number where the fix should be applied (1-indexed)
    pub line: usize,
    /// The original text to replace (if None and new_text is empty, delete the line)
    pub old_text: Option<String>,
    /// The new text to insert (empty string with old_text=None means delete)
    pub new_text: String,
    /// Whether to delete the entire line
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub delete_line: bool,
    /// Whether to insert new_text as a new line after the specified line
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub insert_after: bool,
    /// Start byte offset for range-based fix (0-indexed, inclusive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<usize>,
    /// End byte offset for range-based fix (0-indexed, exclusive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_offset: Option<usize>,
}

impl Fix {
    /// Create a fix that replaces text on a specific line
    pub fn replace(line: usize, old_text: &str, new_text: &str) -> Self {
        Self {
            line,
            old_text: Some(old_text.to_string()),
            new_text: new_text.to_string(),
            delete_line: false,
            insert_after: false,
            start_offset: None,
            end_offset: None,
        }
    }

    /// Create a fix that replaces an entire line
    pub fn replace_line(line: usize, new_text: &str) -> Self {
        Self {
            line,
            old_text: None,
            new_text: new_text.to_string(),
            delete_line: false,
            insert_after: false,
            start_offset: None,
            end_offset: None,
        }
    }

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

#[derive(Debug, Clone, Serialize)]
pub struct LintError {
    pub rule: String,
    pub category: String,
    pub message: String,
    pub severity: Severity,
    pub line: Option<usize>,
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

impl LintError {
    pub fn new(rule: &str, category: &str, message: &str, severity: Severity) -> Self {
        Self {
            rule: rule.to_string(),
            category: category.to_string(),
            message: message.to_string(),
            severity,
            line: None,
            column: None,
            fix: None,
        }
    }

    pub fn with_location(mut self, line: usize, column: usize) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }
}

pub trait LintRule: Send + Sync {
    fn name(&self) -> &'static str;
    fn category(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn check(&self, config: &Config, path: &Path) -> Vec<LintError>;

    /// Check with pre-serialized config JSON (optimization for WASM plugins)
    ///
    /// This method allows passing a pre-serialized config JSON to avoid
    /// repeated serialization when running multiple plugins.
    /// Default implementation ignores the serialized config and calls check().
    fn check_with_serialized_config(
        &self,
        config: &Config,
        path: &Path,
        _serialized_config: &str,
    ) -> Vec<LintError> {
        self.check(config, path)
    }

    /// Get detailed explanation of why this rule exists
    fn why(&self) -> Option<&str> {
        None
    }

    /// Get example of bad configuration
    fn bad_example(&self) -> Option<&str> {
        None
    }

    /// Get example of good configuration
    fn good_example(&self) -> Option<&str> {
        None
    }

    /// Get reference URLs
    fn references(&self) -> Option<Vec<String>> {
        None
    }
}

pub struct Linter {
    rules: Vec<Box<dyn LintRule>>,
}

impl Linter {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn with_default_rules() -> Self {
        Self::with_config(None)
    }

    pub fn with_config(config: Option<&LintConfig>) -> Self {
        use crate::rules::{
            Indent, InvalidDirectiveContext, MissingSemicolon, UnclosedQuote, UnmatchedBraces,
        };

        let mut linter = Self::new();

        let is_enabled = |name: &str| {
            config
                .map(|c| c.is_rule_enabled(name))
                .unwrap_or_else(|| !LintConfig::DISABLED_BY_DEFAULT.contains(&name))
        };

        // Syntax rules
        if is_enabled("unmatched-braces") {
            linter.add_rule(Box::new(UnmatchedBraces));
        }
        if is_enabled("unclosed-quote") {
            linter.add_rule(Box::new(UnclosedQuote));
        }
        if is_enabled("missing-semicolon") {
            linter.add_rule(Box::new(MissingSemicolon));
        }
        // invalid-directive-context: use native implementation when additional_contexts is configured
        // (for extension modules like nginx-rtmp-module); otherwise use WASM plugin
        #[cfg(feature = "builtin-plugins")]
        let use_native_invalid_directive_context = config
            .and_then(|c| c.additional_contexts())
            .is_some_and(|additional| !additional.is_empty());
        #[cfg(not(feature = "builtin-plugins"))]
        let use_native_invalid_directive_context = true;

        if is_enabled("invalid-directive-context") && use_native_invalid_directive_context {
            let rule = if let Some(additional) = config.and_then(|c| c.additional_contexts()).cloned()
            {
                InvalidDirectiveContext::with_additional_contexts(additional)
            } else {
                InvalidDirectiveContext::new()
            };
            linter.add_rule(Box::new(rule));
        }

        // Style rules
        if is_enabled("indent") {
            let rule = if let Some(indent_size) = config
                .and_then(|c| c.get_rule_config("indent"))
                .and_then(|cfg| cfg.indent_size)
            {
                Indent { indent_size }
            } else {
                Indent::default()
            };
            linter.add_rule(Box::new(rule));
        }
        // Load builtin plugins when feature is enabled
        #[cfg(feature = "builtin-plugins")]
        {
            use crate::plugin::builtin::load_builtin_plugins;

            if let Ok(plugins) = load_builtin_plugins() {
                for plugin in plugins {
                    // Skip invalid-directive-context if native implementation is used
                    if plugin.name() == "invalid-directive-context"
                        && use_native_invalid_directive_context
                    {
                        continue;
                    }
                    if is_enabled(plugin.name()) {
                        linter.add_rule(Box::new(plugin));
                    }
                }
            }
        }

        linter
    }

    pub fn add_rule(&mut self, rule: Box<dyn LintRule>) {
        self.rules.push(rule);
    }

    /// Remove rules that match the predicate
    pub fn remove_rules_by_name<F>(&mut self, should_remove: F)
    where
        F: Fn(&str) -> bool,
    {
        self.rules.retain(|rule| !should_remove(rule.name()));
    }

    /// Get a reference to all rules
    pub fn rules(&self) -> &[Box<dyn LintRule>] {
        &self.rules
    }

    /// Run all lint rules and collect errors
    ///
    /// Uses parallel iteration when the cli feature is enabled (via rayon)
    #[cfg(feature = "cli")]
    pub fn lint(&self, config: &Config, path: &Path) -> Vec<LintError> {
        // Pre-serialize config once for all rules (optimization for WASM plugins)
        let serialized_config = serde_json::to_string(config).unwrap_or_default();

        self.rules
            .par_iter()
            .map(|rule| rule.check_with_serialized_config(config, path, &serialized_config))
            .collect::<Vec<_>>()
            .into_iter()
            .flatten()
            .collect()
    }

    /// Run all lint rules and collect errors (sequential version for WASM)
    #[cfg(not(feature = "cli"))]
    pub fn lint(&self, config: &Config, path: &Path) -> Vec<LintError> {
        // Pre-serialize config once for all rules (optimization for WASM plugins)
        let serialized_config = serde_json::to_string(config).unwrap_or_default();

        self.rules
            .iter()
            .flat_map(|rule| rule.check_with_serialized_config(config, path, &serialized_config))
            .collect()
    }

    /// Run all lint rules with ignore comment support
    ///
    /// This method takes the file content to parse ignore comments
    /// and filter out ignored errors. Returns a tuple of (errors, ignored_count).
    #[cfg(feature = "cli")]
    pub fn lint_with_content(
        &self,
        config: &Config,
        path: &Path,
        content: &str,
    ) -> (Vec<LintError>, usize) {
        use crate::ignore::{filter_errors, known_rule_names, warnings_to_errors, IgnoreTracker};

        let valid_rules = known_rule_names();
        let (mut tracker, warnings) = IgnoreTracker::from_content_with_rules(content, Some(&valid_rules));
        let errors = self.lint(config, path);
        let result = filter_errors(errors, &mut tracker);
        let mut errors = result.errors;
        errors.extend(warnings_to_errors(warnings));
        errors.extend(warnings_to_errors(result.unused_warnings));
        (errors, result.ignored_count)
    }

    /// Run all lint rules with ignore comment support (sequential version for WASM)
    #[cfg(not(feature = "cli"))]
    pub fn lint_with_content(
        &self,
        config: &Config,
        path: &Path,
        content: &str,
    ) -> (Vec<LintError>, usize) {
        use crate::ignore::{filter_errors, known_rule_names, warnings_to_errors, IgnoreTracker};

        let valid_rules = known_rule_names();
        let (mut tracker, warnings) = IgnoreTracker::from_content_with_rules(content, Some(&valid_rules));
        let errors = self.lint(config, path);
        let result = filter_errors(errors, &mut tracker);
        let mut errors = result.errors;
        errors.extend(warnings_to_errors(warnings));
        errors.extend(warnings_to_errors(result.unused_warnings));
        (errors, result.ignored_count)
    }

    /// Run all lint rules with profiling and collect errors with timing information
    ///
    /// Returns a tuple of (errors, profile_results) where profile_results contains
    /// the time each rule took to execute.
    #[cfg(feature = "cli")]
    pub fn lint_with_profile(
        &self,
        config: &Config,
        path: &Path,
    ) -> (Vec<LintError>, Vec<RuleProfile>) {
        use std::time::Instant;

        // Pre-serialize config once for all rules (optimization for WASM plugins)
        let serialized_config = serde_json::to_string(config).unwrap_or_default();

        let results: Vec<(Vec<LintError>, RuleProfile)> = self
            .rules
            .iter()
            .map(|rule| {
                let start = Instant::now();
                let errors = rule.check_with_serialized_config(config, path, &serialized_config);
                let duration = start.elapsed();
                let profile = RuleProfile {
                    name: rule.name().to_string(),
                    category: rule.category().to_string(),
                    duration,
                    error_count: errors.len(),
                };
                (errors, profile)
            })
            .collect();

        let errors: Vec<LintError> = results.iter().flat_map(|(e, _)| e.clone()).collect();
        let profiles: Vec<RuleProfile> = results.into_iter().map(|(_, p)| p).collect();

        (errors, profiles)
    }

    /// Run all lint rules with profiling and ignore comment support
    #[cfg(feature = "cli")]
    pub fn lint_with_content_and_profile(
        &self,
        config: &Config,
        path: &Path,
        content: &str,
    ) -> (Vec<LintError>, usize, Vec<RuleProfile>) {
        use crate::ignore::{filter_errors, known_rule_names, warnings_to_errors, IgnoreTracker};

        let valid_rules = known_rule_names();
        let (mut tracker, warnings) = IgnoreTracker::from_content_with_rules(content, Some(&valid_rules));
        let (errors, profiles) = self.lint_with_profile(config, path);
        let result = filter_errors(errors, &mut tracker);
        let mut errors = result.errors;
        errors.extend(warnings_to_errors(warnings));
        errors.extend(warnings_to_errors(result.unused_warnings));
        (errors, result.ignored_count, profiles)
    }
}

/// Profiling information for a single rule
#[derive(Debug, Clone)]
pub struct RuleProfile {
    /// Rule name
    pub name: String,
    /// Rule category
    pub category: String,
    /// Time taken to execute the rule
    pub duration: std::time::Duration,
    /// Number of errors found by this rule
    pub error_count: usize,
}

impl Default for Linter {
    fn default() -> Self {
        Self::with_default_rules()
    }
}
