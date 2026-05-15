// Re-export core types from nginx-lint-common
use nginx_lint_common::config::LintConfig;
use nginx_lint_common::ignore::IgnoreTracker;
pub use nginx_lint_common::linter::{Fix, LintError, LintRule, Severity};
use nginx_lint_common::nginx_version::{NginxVersion, is_in_range};
use nginx_lint_common::parser::ast::Config;
#[cfg(feature = "cli")]
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// Process-wide set of `(rule_name, "min"|"max")` pairs for which we have
/// already warned about an unparseable version bound. Plugins are registered
/// once per process, so a single warning is sufficient — without this dedup
/// the same warning would fire every time `Linter::with_config` is called.
fn warned_invalid_bounds() -> &'static Mutex<HashSet<(String, &'static str)>> {
    static WARNED: OnceLock<Mutex<HashSet<(String, &'static str)>>> = OnceLock::new();
    WARNED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Try to parse a plugin-declared bound; emit a one-shot warning if parsing
/// fails so plugin authors notice typos (`"1.30"` instead of `"1.30.0"`).
/// Treats the bound as unbounded on parse failure, matching the silent
/// behaviour that existed before this warning was added.
fn parse_plugin_bound(
    rule_name: &str,
    which: &'static str,
    raw: Option<&str>,
) -> Option<NginxVersion> {
    let raw = raw?;
    match NginxVersion::parse(raw) {
        Ok(v) => Some(v),
        Err(e) => {
            let key = (rule_name.to_string(), which);
            let mut warned = warned_invalid_bounds().lock().expect("warned set poisoned");
            if warned.insert(key) {
                eprintln!(
                    "warning: rule '{}' declares an unparseable {}_nginx_version '{}': {} \
                     (treated as unbounded; rule will not be version-filtered on this side)",
                    rule_name, which, raw, e
                );
            }
            None
        }
    }
}

/// Result of evaluating a rule against the configured target nginx version.
enum VersionGate {
    /// Rule applies to the target version, or filtering is disabled.
    Allow,
    /// Rule does not apply; skip silently (no `[rules.<name>]` block exists).
    SkipSilently,
    /// Rule does not apply but the user explicitly enabled it without
    /// `skip_version_check = true`; skip and warn so the mismatch is visible.
    SkipWithWarning {
        rule: String,
        target: String,
        min: Option<String>,
        max: Option<String>,
    },
}

/// Evaluate whether a rule should run given the configured target nginx
/// version and the user's explicit configuration of that rule.
fn evaluate_version_gate(
    rule: &dyn LintRule,
    target: Option<&NginxVersion>,
    config: Option<&LintConfig>,
) -> VersionGate {
    let Some(target) = target else {
        return VersionGate::Allow;
    };

    // If the rule opts in to skipping the version check, allow it through.
    if let Some(cfg) = config
        && cfg.rule_skip_version_check(rule.name())
    {
        return VersionGate::Allow;
    }

    let min = parse_plugin_bound(rule.name(), "min", rule.min_nginx_version());
    let max = parse_plugin_bound(rule.name(), "max", rule.max_nginx_version());

    if is_in_range(target, min.as_ref(), max.as_ref()) {
        return VersionGate::Allow;
    }

    // Out of range. Decide whether to warn.
    let explicitly_enabled = config
        .map(|c| c.rule_explicitly_configured(rule.name()) && c.is_rule_enabled(rule.name()))
        .unwrap_or(false);

    if explicitly_enabled {
        VersionGate::SkipWithWarning {
            rule: rule.name().to_string(),
            target: target.to_string(),
            min: rule.min_nginx_version().map(String::from),
            max: rule.max_nginx_version().map(String::from),
        }
    } else {
        VersionGate::SkipSilently
    }
}

pub struct Linter {
    rules: Vec<Box<dyn LintRule>>,
    /// Rule names that exist in the catalog but are intentionally not running
    /// in this `Linter` (e.g. filtered out by the CLI's `--rule-only`). They
    /// are still recognised by ignore-comment parsing — both as valid rule
    /// names *and* as dormant rules whose unused ignore directives are
    /// suppressed — so toggling the filter does not churn the user's config.
    inactive_rules: HashSet<String>,
}

impl Linter {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            inactive_rules: HashSet::new(),
        }
    }

    pub fn with_default_rules() -> Self {
        Self::with_config(None, None)
    }

    pub fn with_config(config: Option<&LintConfig>, include_prefix: Option<&Path>) -> Self {
        #[cfg(feature = "cli")]
        use crate::rules::IncludePathExists;
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
        #[cfg(feature = "cli")]
        if is_enabled("include-path-exists") {
            let rule = if let Some(c) = config {
                let mappings = c.include_path_mappings();
                if mappings.is_empty() && include_prefix.is_none() {
                    IncludePathExists::new()
                } else {
                    IncludePathExists::with_path_mappings_and_prefix(
                        mappings.to_vec(),
                        include_prefix.map(|p| p.to_path_buf()),
                    )
                }
            } else if include_prefix.is_some() {
                IncludePathExists::with_path_mappings_and_prefix(
                    Vec::new(),
                    include_prefix.map(|p| p.to_path_buf()),
                )
            } else {
                IncludePathExists::new()
            };
            linter.add_rule(Box::new(rule));
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

        // Apply target_nginx_version filter (if configured). Rules whose
        // declared min/max range does not include the configured version are
        // dropped; if the user explicitly enabled such a rule without
        // `skip_version_check = true`, a warning is emitted to stderr so the
        // mismatch is visible. Dropped rules are kept in `inactive_rules` so
        // existing `# nginx-lint:ignore[<rule>]` directives still parse cleanly.
        let target_version = config
            .and_then(|c| c.target_nginx_version())
            .and_then(|raw| match NginxVersion::parse(raw) {
                Ok(v) => Some(v),
                Err(e) => {
                    eprintln!(
                        "warning: ignoring target_nginx_version: {} (skipping version-based rule filtering)",
                        e
                    );
                    None
                }
            });

        if let Some(target) = target_version {
            let mut filtered_out: HashSet<String> = HashSet::new();
            linter.rules.retain(|rule| {
                match evaluate_version_gate(rule.as_ref(), Some(&target), config) {
                    VersionGate::Allow => true,
                    VersionGate::SkipSilently => {
                        filtered_out.insert(rule.name().to_string());
                        false
                    }
                    VersionGate::SkipWithWarning {
                        rule: name,
                        target,
                        min,
                        max,
                    } => {
                        let range = match (min.as_deref(), max.as_deref()) {
                            (Some(min), Some(max)) => format!("nginx >={}, <={}", min, max),
                            (Some(min), None) => format!("nginx >={}", min),
                            (None, Some(max)) => format!("nginx <={}", max),
                            (None, None) => "any nginx version".to_string(),
                        };
                        eprintln!(
                            "warning: rule '{}' requires {} but target_nginx_version is {}; \
                             skipping. Set [rules.{}] skip_version_check = true to override.",
                            name, range, target, name
                        );
                        filtered_out.insert(name);
                        false
                    }
                }
            });
            linter.inactive_rules.extend(filtered_out);
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
    pub fn rule_names(&self) -> HashSet<String> {
        self.rules.iter().map(|r| r.name().to_string()).collect()
    }

    /// Register rule names that are intentionally not running in this linter
    /// but should still be honoured by ignore-comment parsing — they will be
    /// treated as valid rule names *and* as dormant rules (so their unused
    /// ignore directives are suppressed). Used by the CLI's `--rule-only`
    /// filter to keep existing `# nginx-lint:ignore` directives quiet for
    /// rules the user has temporarily filtered out.
    ///
    /// Pass only the rules that are *not* currently running; including a
    /// rule that *is* running would incorrectly suppress its own unused
    /// ignore warnings.
    pub fn set_inactive_rules(&mut self, names: HashSet<String>) {
        self.inactive_rules = names;
    }

    /// Names recognised by the ignore-comment parser: registered rules plus
    /// any inactive rules supplied via [`set_inactive_rules`].
    fn valid_rule_names_for_ignore(&self) -> HashSet<String> {
        let mut names = self.rule_names();
        names.extend(self.inactive_rules.iter().cloned());
        names
    }

    /// Build an `IgnoreTracker` for the given content, wiring up both the
    /// valid-rule-name set (for unknown-rule warnings) and the dormant-rule
    /// set (for unused-ignore suppression). Centralised so the three
    /// `lint_with_content*` entry points stay in sync.
    fn make_ignore_tracker(
        &self,
        content: &str,
    ) -> (IgnoreTracker, Vec<nginx_lint_common::ignore::IgnoreWarning>) {
        let valid_rules = self.valid_rule_names_for_ignore();
        let (mut tracker, warnings) =
            IgnoreTracker::from_content_with_rules(content, Some(&valid_rules));
        if !self.inactive_rules.is_empty() {
            tracker.set_dormant_rules(&self.inactive_rules);
        }
        (tracker, warnings)
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
        use nginx_lint_common::ignore::{filter_errors, warnings_to_errors};

        let (mut tracker, warnings) = self.make_ignore_tracker(content);
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
        use nginx_lint_common::ignore::{filter_errors, warnings_to_errors};

        let (mut tracker, warnings) = self.make_ignore_tracker(content);
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
        use nginx_lint_common::ignore::{filter_errors, warnings_to_errors};

        let (mut tracker, warnings) = self.make_ignore_tracker(content);
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

#[cfg(test)]
mod version_filter_tests {
    use super::*;

    /// Mock rule with configurable name and version bounds for testing.
    struct MockRule {
        name: &'static str,
        min: Option<&'static str>,
        max: Option<&'static str>,
    }

    impl LintRule for MockRule {
        fn name(&self) -> &'static str {
            self.name
        }
        fn category(&self) -> &'static str {
            "test"
        }
        fn description(&self) -> &'static str {
            "mock rule"
        }
        fn check(&self, _config: &Config, _path: &Path) -> Vec<LintError> {
            Vec::new()
        }
        fn min_nginx_version(&self) -> Option<&str> {
            self.min
        }
        fn max_nginx_version(&self) -> Option<&str> {
            self.max
        }
    }

    fn build_config(toml: &str) -> LintConfig {
        LintConfig::parse(toml).expect("config parse")
    }

    #[test]
    fn no_target_version_allows_rule() {
        let rule = MockRule {
            name: "demo",
            min: None,
            max: Some("1.0.0"),
        };
        let gate = evaluate_version_gate(&rule, None, None);
        assert!(matches!(gate, VersionGate::Allow));
    }

    #[test]
    fn target_in_range_allows_rule() {
        let rule = MockRule {
            name: "demo",
            min: Some("0.6.27"),
            max: Some("1.30.0"),
        };
        let target = NginxVersion::parse("1.29.0").unwrap();
        let gate = evaluate_version_gate(&rule, Some(&target), None);
        assert!(matches!(gate, VersionGate::Allow));
    }

    #[test]
    fn target_above_max_skips_silently_when_not_configured() {
        let rule = MockRule {
            name: "demo",
            min: None,
            max: Some("1.30.0"),
        };
        let target = NginxVersion::parse("1.31.0").unwrap();
        let gate = evaluate_version_gate(&rule, Some(&target), None);
        assert!(matches!(gate, VersionGate::SkipSilently));
    }

    #[test]
    fn explicitly_enabled_out_of_range_warns() {
        let rule = MockRule {
            name: "demo",
            min: None,
            max: Some("1.30.0"),
        };
        let config = build_config("[rules.demo]\nenabled = true\n");
        let target = NginxVersion::parse("1.31.0").unwrap();
        let gate = evaluate_version_gate(&rule, Some(&target), Some(&config));
        assert!(matches!(gate, VersionGate::SkipWithWarning { .. }));
    }

    #[test]
    fn skip_version_check_overrides_filter() {
        let rule = MockRule {
            name: "demo",
            min: None,
            max: Some("1.30.0"),
        };
        let config = build_config("[rules.demo]\nenabled = true\nskip_version_check = true\n");
        let target = NginxVersion::parse("1.31.0").unwrap();
        let gate = evaluate_version_gate(&rule, Some(&target), Some(&config));
        assert!(matches!(gate, VersionGate::Allow));
    }

    #[test]
    fn explicitly_disabled_out_of_range_does_not_warn() {
        let rule = MockRule {
            name: "demo",
            min: None,
            max: Some("1.30.0"),
        };
        let config = build_config("[rules.demo]\nenabled = false\n");
        let target = NginxVersion::parse("1.31.0").unwrap();
        let gate = evaluate_version_gate(&rule, Some(&target), Some(&config));
        // Not "enabled", so we skip silently rather than nagging the user.
        assert!(matches!(gate, VersionGate::SkipSilently));
    }

    #[test]
    fn unparseable_rule_bounds_treated_as_unbounded() {
        // Use a name unique to this test so the warning-dedup set doesn't
        // interfere with other tests that exercise the same rule name.
        let rule = MockRule {
            name: "demo-unparseable-bounds",
            min: Some("garbage"),
            max: Some("also-garbage"),
        };
        let target = NginxVersion::parse("1.31.0").unwrap();
        let gate = evaluate_version_gate(&rule, Some(&target), None);
        // Both bounds fail to parse -> treated as None -> in range.
        // A stderr warning is emitted once per rule to flag the plugin
        // author about the typo, but the rule still runs (fail-open).
        assert!(matches!(gate, VersionGate::Allow));
    }

    #[test]
    fn unparseable_bound_warning_is_deduplicated() {
        // The dedup set is keyed by (rule_name, "min"|"max"), so calling
        // the gate twice for the same rule must not double-warn. We can't
        // easily capture stderr here, but we can at least exercise the
        // path and confirm the second call doesn't change observable
        // behaviour.
        let rule = MockRule {
            name: "demo-dedup",
            min: Some("oops"),
            max: None,
        };
        let target = NginxVersion::parse("1.31.0").unwrap();
        let _ = evaluate_version_gate(&rule, Some(&target), None);
        let gate = evaluate_version_gate(&rule, Some(&target), None);
        assert!(matches!(gate, VersionGate::Allow));
    }

    #[test]
    fn target_below_min_skips() {
        let rule = MockRule {
            name: "demo",
            min: Some("1.0.0"),
            max: None,
        };
        let target = NginxVersion::parse("0.9.0").unwrap();
        let gate = evaluate_version_gate(&rule, Some(&target), None);
        assert!(matches!(gate, VersionGate::SkipSilently));
    }
}
