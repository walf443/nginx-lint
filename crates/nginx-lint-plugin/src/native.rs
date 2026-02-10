//! Native plugin adapter.
//!
//! Provides [`NativePluginRule<P>`] which wraps a [`Plugin`] implementation
//! into a [`LintRule`](nginx_lint_common::linter::LintRule), allowing WASM plugins to be run
//! natively without WASM VM or serialization overhead.
//!
//! This is used internally by nginx-lint to embed builtin plugins directly
//! into the binary when built with the `wasm-builtin-plugins` feature.
//!
//! # Example
//!
//! ```
//! use nginx_lint_plugin::prelude::*;
//! use nginx_lint_plugin::native::NativePluginRule;
//!
//! # #[derive(Default)]
//! # struct MyPlugin;
//! # impl Plugin for MyPlugin {
//! #     fn spec(&self) -> PluginSpec {
//! #         PluginSpec::new("my-rule", "test", "Test rule")
//! #     }
//! #     fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
//! #         Vec::new()
//! #     }
//! # }
//! // Wrap a plugin as a native lint rule
//! let rule = NativePluginRule::<MyPlugin>::new();
//! // `rule` now implements LintRule and can be registered in the linter
//! ```

use crate::types::{
    Fix as PluginFix, LintError as PluginLintError, Plugin, Severity as PluginSeverity,
};
use nginx_lint_common::linter::{
    Fix as CommonFix, LintError as CommonLintError, LintRule, Severity as CommonSeverity,
};
use nginx_lint_common::parser::ast::Config;
use std::path::Path;

/// Convert a plugin Fix to a common Fix
fn convert_fix(fix: PluginFix) -> CommonFix {
    CommonFix {
        line: fix.line,
        old_text: fix.old_text,
        new_text: fix.new_text,
        delete_line: fix.delete_line,
        insert_after: fix.insert_after,
        start_offset: fix.start_offset,
        end_offset: fix.end_offset,
    }
}

/// Convert a plugin LintError to a common LintError
fn convert_lint_error(err: PluginLintError) -> CommonLintError {
    let severity = match err.severity {
        PluginSeverity::Error => CommonSeverity::Error,
        PluginSeverity::Warning => CommonSeverity::Warning,
    };

    let mut common = CommonLintError::new(&err.rule, &err.category, &err.message, severity);

    if let (Some(line), Some(column)) = (err.line, err.column) {
        common = common.with_location(line, column);
    } else if let Some(line) = err.line {
        common = common.with_location(line, 1);
    }

    for fix in err.fixes {
        common = common.with_fix(convert_fix(fix));
    }

    common
}

/// Adapter that wraps a `Plugin` implementation into a `LintRule`.
///
/// This allows running WASM plugin code natively, bypassing the
/// serialization/deserialization and WASM VM overhead.
pub struct NativePluginRule<P: Plugin> {
    plugin: P,
    name: &'static str,
    category: &'static str,
    description: &'static str,
    severity: Option<&'static str>,
    why: Option<&'static str>,
    bad_example: Option<&'static str>,
    good_example: Option<&'static str>,
    references: Option<Vec<String>>,
}

impl<P: Plugin> Default for NativePluginRule<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Plugin> NativePluginRule<P> {
    pub fn new() -> Self {
        let plugin = P::default();
        let spec = plugin.spec();

        // Leak strings for 'static lifetime (same approach as WasmLintRule)
        let name: &'static str = Box::leak(spec.name.into_boxed_str());
        let category: &'static str = Box::leak(spec.category.into_boxed_str());
        let description: &'static str = Box::leak(spec.description.into_boxed_str());
        let severity: Option<&'static str> = spec.severity.map(|s| &*Box::leak(s.into_boxed_str()));
        let why: Option<&'static str> = spec.why.map(|s| &*Box::leak(s.into_boxed_str()));
        let bad_example: Option<&'static str> =
            spec.bad_example.map(|s| &*Box::leak(s.into_boxed_str()));
        let good_example: Option<&'static str> =
            spec.good_example.map(|s| &*Box::leak(s.into_boxed_str()));
        let references = spec.references;

        Self {
            plugin,
            name,
            category,
            description,
            severity,
            why,
            bad_example,
            good_example,
            references,
        }
    }
}

impl<P: Plugin + Send + Sync> LintRule for NativePluginRule<P> {
    fn name(&self) -> &'static str {
        self.name
    }

    fn category(&self) -> &'static str {
        self.category
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn check(&self, config: &Config, path: &Path) -> Vec<CommonLintError> {
        let path_str = path.to_string_lossy();
        let errors = self.plugin.check(config, &path_str);
        errors.into_iter().map(convert_lint_error).collect()
    }

    fn severity(&self) -> Option<&str> {
        self.severity
    }

    fn why(&self) -> Option<&str> {
        self.why
    }

    fn bad_example(&self) -> Option<&str> {
        self.bad_example
    }

    fn good_example(&self) -> Option<&str> {
        self.good_example
    }

    fn references(&self) -> Option<Vec<String>> {
        self.references.clone()
    }
}
