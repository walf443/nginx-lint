//! Core types for the lint engine: rule definitions, error reporting, and fix proposals.
//!
//! This module contains the fundamental abstractions used by both native Rust
//! rules (in `src/rules/`) and WASM plugin rules:
//!
//! - [`LintRule`] — trait that every rule implements
//! - [`LintError`] — a single diagnostic produced by a rule
//! - [`Severity`] — error vs. warning classification
//! - [`Fix`] — an auto-fix action attached to a diagnostic
//! - [`Linter`] — collects rules and runs them against a parsed config
//!
//! The types live in `types.rs`, the trait in `rule.rs`, batching in
//! `batch.rs` and fix application in `fix.rs`; everything is re-exported
//! here, so `nginx_lint_common::linter::X` is the path for all of it.

mod batch;
mod fix;
mod rule;
mod types;

pub use batch::{BatchMemo, batch_rules, run_batch};
pub use fix::{
    FixApplyResult, apply_fixes_to_content, apply_fixes_to_content_detailed, compute_line_starts,
    normalize_line_fix,
};
pub use rule::{BatchKey, LintRule};
pub use types::{Fix, LintError, Severity};

use crate::parser::ast::Config;
use std::path::Path;

/// Display-ordered list of rule categories for UI output.
///
/// Used by the CLI and documentation generator to group rules consistently.
pub const RULE_CATEGORIES: &[&str] = &[
    "style",
    "syntax",
    "security",
    "best-practices",
    "deprecation",
];

/// Container that holds [`LintRule`]s and runs them against a parsed config.
///
/// Create a `Linter`, register rules with [`add_rule`](Self::add_rule), then
/// call [`lint`](Self::lint) to collect all diagnostics.
pub struct Linter {
    rules: Vec<Box<dyn LintRule>>,
}

impl Linter {
    /// Create an empty linter with no rules registered.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Register a lint rule. Rules are executed in registration order.
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

    /// Run all lint rules and collect errors (sequential version)
    pub fn lint(&self, config: &Config, path: &Path) -> Vec<LintError> {
        let shared_config = std::sync::OnceLock::new();

        self.rules
            .iter()
            .flat_map(|rule| run_rule(rule.as_ref(), config, path, &shared_config))
            .collect()
    }
}

impl Default for Linter {
    fn default() -> Self {
        Self::new()
    }
}

/// Run a single rule, dispatching to [`LintRule::check_shared`] with one
/// lazily-created `Arc<Config>` for rules that
/// [want a shared handle](LintRule::wants_shared_config), and to
/// [`LintRule::check`] otherwise.
///
/// The `Arc` is created at most once per `shared_config` cell (i.e. per
/// linted file), so purely native rule sets never pay for the clone. Linter
/// implementations should route every rule invocation through this function
/// so the dispatch policy stays in one place.
pub fn run_rule(
    rule: &dyn LintRule,
    config: &Config,
    path: &Path,
    shared_config: &std::sync::OnceLock<std::sync::Arc<Config>>,
) -> Vec<LintError> {
    if rule.wants_shared_config() {
        let shared = shared_config.get_or_init(|| std::sync::Arc::new(config.clone()));
        rule.check_shared(shared, path)
    } else {
        rule.check(config, path)
    }
}

/// Like [`run_rule`], but additionally dispatches to
/// [`LintRule::check_with_content`] for rules that
/// [want raw content](LintRule::wants_content), so those rules don't have to
/// re-read the file from disk when the caller already has it in memory.
/// Falls back to [`run_rule`]'s dispatch policy otherwise.
pub fn run_rule_with_content(
    rule: &dyn LintRule,
    config: &Config,
    path: &Path,
    content: &str,
    shared_config: &std::sync::OnceLock<std::sync::Arc<Config>>,
) -> Vec<LintError> {
    if rule.wants_content() {
        rule.check_with_content(config, path, content)
    } else {
        run_rule(rule, config, path, shared_config)
    }
}
