//! The [`LintRule`] trait every rule implements, and the [`BatchKey`] by
//! which rules that can be checked together say so.

use super::LintError;
use crate::parser::ast::Config;
use std::path::Path;

/// Identifies a group of rules that can be checked together; see
/// [`LintRule::batch_key`]. The implementor's type is part of the key, so
/// two implementors numbering their groups independently never share one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BatchKey {
    implementor: std::any::TypeId,
    id: u64,
}

impl BatchKey {
    /// A key for group `id` of the rules of type `T`.
    pub fn new<T: 'static>(id: u64) -> Self {
        Self {
            implementor: std::any::TypeId::of::<T>(),
            id,
        }
    }
}

/// A lint rule that can be checked against a parsed nginx configuration.
///
/// Every rule — whether implemented as a native Rust struct or as a WASM
/// plugin — implements this trait. The four required methods supply metadata
/// and the check logic; the optional methods provide documentation and
/// plugin-specific overrides.
///
/// # Required methods
///
/// | Method | Purpose |
/// |--------|---------|
/// | [`name`](Self::name) | Unique rule identifier (e.g. `"server-tokens-enabled"`) |
/// | [`category`](Self::category) | Category for grouping (e.g. `"security"`) |
/// | [`description`](Self::description) | One-line human-readable summary |
/// | [`check`](Self::check) | Run the rule and return diagnostics |
pub trait LintRule: Send + Sync {
    /// Unique identifier for this rule (e.g. `"server-tokens-enabled"`).
    fn name(&self) -> &'static str;
    /// Category this rule belongs to (e.g. `"security"`, `"style"`).
    fn category(&self) -> &'static str;
    /// One-line human-readable description of what this rule checks.
    fn description(&self) -> &'static str;
    /// Run the rule against `config` (parsed from `path`) and return diagnostics.
    fn check(&self, config: &Config, path: &Path) -> Vec<LintError>;

    /// Check with pre-serialized config JSON (optimization for WASM plugins)
    ///
    /// This method allows passing a pre-serialized config JSON to avoid
    /// repeated serialization when running multiple plugins.
    /// Default implementation ignores the serialized config and calls check().
    #[deprecated(
        since = "0.16.0",
        note = "no longer called by the linter; the serialized config was only used by \
                legacy core-module plugins. Implement check() or check_shared() instead."
    )]
    fn check_with_serialized_config(
        &self,
        config: &Config,
        path: &Path,
        _serialized_config: &str,
    ) -> Vec<LintError> {
        self.check(config, path)
    }

    /// Whether this rule wants the config as a shared `Arc` handle.
    ///
    /// Rules that hand the config to another owner (e.g. WASM plugin rules,
    /// which store it in the sandbox's resource table) should return `true`
    /// so the linter shares one `Arc<Config>` across all such rules instead
    /// of each rule deep-cloning the AST per check.
    fn wants_shared_config(&self) -> bool {
        false
    }

    /// Run the rule with a shared config handle.
    ///
    /// The linter calls this instead of [`check`](Self::check) when
    /// [`wants_shared_config`](Self::wants_shared_config) returns `true`.
    /// Default implementation borrows the config and calls `check()`.
    fn check_shared(&self, config: &std::sync::Arc<Config>, path: &Path) -> Vec<LintError> {
        self.check(config, path)
    }

    /// A key shared by rules that can be checked together in one call.
    ///
    /// Several rules of one WASM component are one such group: the linter
    /// calls [`check_shared_batch`](Self::check_shared_batch) once for the
    /// group, on any one of its rules, with every group member's name,
    /// instead of [`check_shared`](Self::check_shared) once per rule. The
    /// component is then instantiated, and the config transferred to it,
    /// once per file rather than once per rule. Rules with no key — the
    /// default — are checked one at a time.
    ///
    /// **A rule that returns a key should implement
    /// [`check_shared_batch`](Self::check_shared_batch)**: the linter checks
    /// a group through that method on one of its members. The default
    /// declines a call for more than one rule, which the linter answers by
    /// checking the group one rule at a time, with a warning — correct,
    /// but without the saving a key is for. A rule that also [wants the
    /// file content](Self::wants_content) is never grouped, whatever its
    /// key: a group of several has no content to pass, so it runs on its
    /// own, with the content, and its siblings batch without it. The
    /// execution deadline a host applies to a batched check is expected to
    /// be the per-rule deadline times the number of rules asked.
    fn batch_key(&self) -> Option<BatchKey> {
        None
    }

    /// Check every rule named in `names` — all sharing this rule's
    /// [`batch_key`](Self::batch_key), this one among them — and return
    /// their findings together, each naming its rule in
    /// [`LintError::rule`]. `Err` means the batched check itself failed
    /// (the component trapped, or ran out of time) and says why; the
    /// linter then checks the rules one at a time through
    /// [`check_shared`](Self::check_shared), each reporting its own
    /// outcome, and says so once per group. The default checks this rule
    /// alone when its own name is the only one, returns nothing for no
    /// names, and declines any other call as unimplemented — which the
    /// linter answers by checking the group one rule at a time, so a rule
    /// with a key that does not override this still has its siblings run,
    /// slower and with a warning saying why.
    fn check_shared_batch(
        &self,
        names: &[&str],
        config: &std::sync::Arc<Config>,
        path: &Path,
    ) -> Result<Vec<LintError>, String> {
        match names {
            [] => Ok(Vec::new()),
            [name] if *name == self.name() => Ok(self.check_shared(config, path)),
            _ => Err(format!(
                "{} does not implement check_shared_batch",
                self.name()
            )),
        }
    }

    /// Whether this rule wants the raw file content directly.
    ///
    /// Rules that need to re-derive diagnostics from the source text itself
    /// (rather than the parsed `Config`) should return `true` so the linter
    /// hands them the content it already has in memory, instead of each rule
    /// independently re-reading the file from disk and re-parsing it.
    ///
    /// Note: this does not compose with [`wants_shared_config`](Self::wants_shared_config) —
    /// the default [`check_with_content`](Self::check_with_content) delegates to
    /// [`check`](Self::check), not [`check_shared`](Self::check_shared). No current
    /// rule needs both; a future one that does would need a custom override.
    fn wants_content(&self) -> bool {
        false
    }

    /// Run the rule with the raw file content already available.
    ///
    /// The linter calls this instead of [`check`](Self::check)/[`check_shared`](Self::check_shared)
    /// when [`wants_content`](Self::wants_content) returns `true` and content is available.
    /// Default implementation ignores `content` and calls `check()`.
    fn check_with_content(&self, config: &Config, path: &Path, _content: &str) -> Vec<LintError> {
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

    /// Get severity level (for plugins)
    fn severity(&self) -> Option<&str> {
        None
    }

    /// Minimum nginx version this rule applies to (inclusive).
    ///
    /// `None` means the rule applies regardless of how old the nginx version is.
    /// Used by the linter's version-based rule filter to decide whether to
    /// run this rule against a config whose
    /// [`target_nginx_version`](crate::config::LintConfig::target_nginx_version)
    /// is set.
    fn min_nginx_version(&self) -> Option<&str> {
        None
    }

    /// Maximum nginx version this rule applies to (inclusive).
    ///
    /// `None` means the rule applies regardless of how new the nginx version is.
    fn max_nginx_version(&self) -> Option<&str> {
        None
    }
}
