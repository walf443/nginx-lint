// Re-export core types from nginx-lint-common
use nginx_lint_common::config::LintConfig;
pub use nginx_lint_common::linter::{Fix, LintError, LintRule, Severity};
use nginx_lint_common::parser::ast::Config;
#[cfg(feature = "cli")]
use rayon::prelude::*;
use std::path::Path;

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
        // (for extension modules like nginx-rtmp-module); otherwise use WASM/native plugin
        #[cfg(any(feature = "wasm-builtin-plugins", feature = "native-builtin-plugins"))]
        let use_native_invalid_directive_context = config
            .and_then(|c| c.additional_contexts())
            .is_some_and(|additional| !additional.is_empty());
        #[cfg(not(any(feature = "wasm-builtin-plugins", feature = "native-builtin-plugins")))]
        let use_native_invalid_directive_context = true;

        if is_enabled("invalid-directive-context") && use_native_invalid_directive_context {
            let rule =
                if let Some(additional) = config.and_then(|c| c.additional_contexts()).cloned() {
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
        // block-lines: use configured max_block_lines if specified
        #[cfg(any(feature = "native-builtin-plugins", feature = "wasm-builtin-plugins"))]
        let use_configured_block_lines = config
            .and_then(|c| c.get_rule_config("block-lines"))
            .and_then(|cfg| cfg.max_block_lines)
            .is_some();
        #[cfg(any(feature = "native-builtin-plugins", feature = "wasm-builtin-plugins"))]
        if is_enabled("block-lines") && use_configured_block_lines {
            use nginx_lint_plugin::native::NativePluginRule;
            let max_lines = config
                .and_then(|c| c.get_rule_config("block-lines"))
                .and_then(|cfg| cfg.max_block_lines)
                .unwrap();
            let plugin = block_lines_plugin::BlockLinesPlugin::with_max_lines(max_lines);
            linter.add_rule(Box::new(NativePluginRule::with_plugin(plugin)));
        }
        // directive-inheritance: use configured excluded/additional directives if specified
        #[cfg(any(feature = "native-builtin-plugins", feature = "wasm-builtin-plugins"))]
        let use_configured_directive_inheritance = config
            .map(|c| {
                c.directive_inheritance_excluded()
                    .is_some_and(|v| !v.is_empty())
                    || c.directive_inheritance_additional()
                        .is_some_and(|v| !v.is_empty())
            })
            .unwrap_or(false);
        #[cfg(any(feature = "native-builtin-plugins", feature = "wasm-builtin-plugins"))]
        if is_enabled("directive-inheritance") && use_configured_directive_inheritance {
            use nginx_lint_plugin::native::NativePluginRule;
            let excluded = config
                .and_then(|c| c.directive_inheritance_excluded())
                .map(|v| v.to_vec())
                .unwrap_or_default();
            let additional = config
                .and_then(|c| c.directive_inheritance_additional())
                .map(|v| {
                    v.iter()
                        .map(|a| directive_inheritance_plugin::DirectiveSpecOwned {
                            name: a.name.clone(),
                            case_insensitive: a.case_insensitive,
                            multi_key: a.multi_key,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let plugin = directive_inheritance_plugin::DirectiveInheritancePlugin::with_config(
                excluded, additional,
            );
            linter.add_rule(Box::new(NativePluginRule::with_plugin(plugin)));
        }

        // Load native plugins when native-builtin-plugins feature is enabled
        #[cfg(feature = "native-builtin-plugins")]
        {
            use crate::plugin::native_builtin::load_native_builtin_plugins;

            let plugins: Vec<Box<dyn LintRule>> = load_native_builtin_plugins();
            for plugin in plugins {
                // Skip invalid-directive-context if native implementation is used
                if plugin.name() == "invalid-directive-context"
                    && use_native_invalid_directive_context
                {
                    continue;
                }
                // Skip block-lines if configured max_block_lines is used
                if plugin.name() == "block-lines" && use_configured_block_lines {
                    continue;
                }
                // Skip directive-inheritance if configured excluded/additional is used
                if plugin.name() == "directive-inheritance" && use_configured_directive_inheritance
                {
                    continue;
                }
                if is_enabled(plugin.name()) {
                    linter.add_rule(plugin);
                }
            }
        }

        // Load WASM builtin plugins when wasm-builtin-plugins is enabled but native-builtin-plugins is not
        #[cfg(all(
            feature = "wasm-builtin-plugins",
            not(feature = "native-builtin-plugins")
        ))]
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
                    // Skip block-lines if configured max_block_lines is used
                    if plugin.name() == "block-lines" && use_configured_block_lines {
                        continue;
                    }
                    // Skip directive-inheritance if configured excluded/additional is used
                    if plugin.name() == "directive-inheritance"
                        && use_configured_directive_inheritance
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

    /// Get a set of all rule names registered in this linter
    pub fn rule_names(&self) -> std::collections::HashSet<String> {
        self.rules.iter().map(|r| r.name().to_string()).collect()
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
        use nginx_lint_common::ignore::{IgnoreTracker, filter_errors, warnings_to_errors};

        let valid_rules = self.rule_names();
        let (mut tracker, warnings) =
            IgnoreTracker::from_content_with_rules(content, Some(&valid_rules));
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
        use nginx_lint_common::ignore::{IgnoreTracker, filter_errors, warnings_to_errors};

        let valid_rules = self.rule_names();
        let (mut tracker, warnings) =
            IgnoreTracker::from_content_with_rules(content, Some(&valid_rules));
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
        use nginx_lint_common::ignore::{IgnoreTracker, filter_errors, warnings_to_errors};

        let valid_rules = self.rule_names();
        let (mut tracker, warnings) =
            IgnoreTracker::from_content_with_rules(content, Some(&valid_rules));
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
