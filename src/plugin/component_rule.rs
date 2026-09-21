//! Component model based lint rule implementation
//!
//! This module implements the LintRule trait for WIT component model plugins.
//! Plugins communicate with the host via WIT resource handles for config access,
//! eliminating the need for JSON serialization. What the host provides to a
//! component — bindings, store, the `Host` impls — is in [`super::host`].

use super::error::PluginError;
use super::host::bindings::{self, Plugin, PluginPre};
use super::host::findings::{PluginSpec, convert_lint_error, convert_plugin_spec, sanitize_text};
use super::host::rules_bindings::PluginRulesPre;
use super::host::{ComponentStoreData, ConfigResource, add_wasi_subset};
use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wasmtime::component::ResourceTable;
use wasmtime::{Engine, Store, StoreLimitsBuilder, Trap};

/// Pre-instantiated bindings of a component, by the world it targets.
/// Import resolution and type checking are done once at load time, so each
/// check call only pays for instantiation itself (shared across threads;
/// also holds the engine).
#[derive(Clone)]
enum Exports {
    /// The `plugin-rules` world: the component carries one or more rules,
    /// and this `ComponentLintRule` is one of them. Every rule of a
    /// component shares the one `PluginRulesPre` (it is reference counted),
    /// so the component is compiled once however many rules it carries,
    /// and — through `batch_key` — instantiated once per file for all of
    /// them by the linter.
    Rules {
        pre: PluginRulesPre<ComponentStoreData>,
        /// The name and category of every rule the component carries, so
        /// a check can tell a finding of a sibling rule from one under an
        /// unknown name, and a failure can be reported under each asked
        /// rule with that rule's category
        rules: Arc<[(String, String)]>,
        /// Identifies the component: every rule loaded from it carries the
        /// same id, which is what lets the linter check them in one call
        id: u64,
    },
    /// The original `plugin` world: the component is one rule. Kept so
    /// components built before `plugin-rules` existed stay loadable.
    Plugin(PluginPre<ComponentStoreData>),
}

impl Exports {
    fn engine(&self) -> &Engine {
        match self {
            Exports::Rules { pre, .. } => pre.engine(),
            Exports::Plugin(pre) => pre.engine(),
        }
    }
}

/// A lint rule implemented as a WIT component model plugin
#[derive(Clone)]
pub struct ComponentLintRule {
    /// Path to the component file (for error reporting)
    path: PathBuf,
    /// Plugin metadata
    spec: PluginSpec,
    /// The component's pre-instantiated exports
    exports: Exports,
    /// Memory limit in bytes
    memory_limit: u64,
    /// Execution timeout per call in epoch ticks (None = no timeout, for
    /// trusted plugins)
    timeout_ticks: Option<u64>,
    /// Leaked static strings for LintRule trait
    name: &'static str,
    category: &'static str,
    description: &'static str,
}

impl ComponentLintRule {
    /// Load every rule a compiled component carries: one per spec for the
    /// `plugin-rules` world, one for the original `plugin` world
    pub fn load(
        engine: &Engine,
        path: PathBuf,
        component_bytes: &[u8],
        memory_limit: u64,
        timeout_ticks: Option<u64>,
        allow_wasi: bool,
    ) -> Result<Vec<Self>, PluginError> {
        // Compile the component
        let component = wasmtime::component::Component::new(engine, component_bytes)
            .map_err(|e| PluginError::compile_error(&path, e.to_string()))?;

        // Register all host functions (types + config-api) and resolve the
        // component's imports once; per-call work is instantiation only.
        // Both worlds import the same interfaces, so one linker serves both.
        let mut linker = wasmtime::component::Linker::<ComponentStoreData>::new(engine);
        Plugin::add_to_linker::<ComponentStoreData, ComponentStoreData>(&mut linker, |data| data)
            .map_err(|e| {
            PluginError::instantiate_error(&path, format!("Failed to add imports to linker: {}", e))
        })?;
        // Off unless asked for: a plugin that imports `wasi:*` fails to
        // instantiate instead, which is the guarantee every plugin has had
        // until now (see add_wasi_subset for what enabling it grants).
        if allow_wasi {
            add_wasi_subset(&mut linker).map_err(|e| {
                PluginError::instantiate_error(
                    &path,
                    format!("Failed to add WASI imports to linker: {}", e),
                )
            })?;
        }
        let instance_pre = linker
            .instantiate_pre(&component)
            .map_err(|e| PluginError::instantiate_error(&path, e.to_string()))?;

        // The world is told from the exports: `specs` exists only in
        // `plugin-rules`. The typed wrapper for that world then checks
        // the full export signatures.
        let is_plugin_rules = component
            .component_type()
            .exports(engine)
            .any(|(export, _)| export == "specs");

        let (exports, specs) = if is_plugin_rules {
            let pre = PluginRulesPre::new(instance_pre)
                .map_err(|e| PluginError::instantiate_error(&path, e.to_string()))?;
            let specs = Self::get_rule_specs(&pre, &path, memory_limit, timeout_ticks)?;
            let rules: Arc<[(String, String)]> = specs
                .iter()
                .map(|spec| (sanitize_text(&spec.name), sanitize_text(&spec.category)))
                .collect();
            let id = next_component_id();
            (Exports::Rules { pre, rules, id }, specs)
        } else {
            let pre = PluginPre::new(instance_pre)
                .map_err(|e| PluginError::instantiate_error(&path, e.to_string()))?;
            let spec = Self::get_plugin_spec(&pre, &path, memory_limit, timeout_ticks)?;
            (Exports::Plugin(pre), vec![spec])
        };

        Ok(specs
            .iter()
            .map(|spec_wit| {
                let spec = convert_plugin_spec(spec_wit);

                // Leak strings for 'static lifetime required by the LintRule trait.
                // These live for the entire program duration. Since plugins are loaded once
                // at startup and never unloaded, this is acceptable.
                let name: &'static str = Box::leak(spec.name.clone().into_boxed_str());
                let category: &'static str = Box::leak(spec.category.clone().into_boxed_str());
                let description: &'static str =
                    Box::leak(spec.description.clone().into_boxed_str());

                Self {
                    path: path.clone(),
                    spec,
                    exports: exports.clone(),
                    memory_limit,
                    timeout_ticks,
                    name,
                    category,
                    description,
                }
            })
            .collect())
    }

    /// Whether the component targets the `plugin-rules` world rather than
    /// the original `plugin` world
    pub fn is_plugin_rules(&self) -> bool {
        matches!(self.exports, Exports::Rules { .. })
    }

    /// Create a store with limits and the execution deadline
    fn create_store(
        engine: &Engine,
        memory_limit: u64,
        timeout_ticks: Option<u64>,
    ) -> Store<ComponentStoreData> {
        let limits = StoreLimitsBuilder::new()
            .memory_size(memory_limit as usize)
            .build();
        let mut store = Store::new(
            engine,
            ComponentStoreData {
                limits,
                table: ResourceTable::new(),
                wasi: None,
            },
        );
        store.limiter(|data| &mut data.limits);
        if let Some(ticks) = timeout_ticks {
            // The loader's epoch ticker advances the engine epoch at a fixed
            // interval; execution traps with Trap::Interrupt once the
            // deadline is reached (wasmtime's default deadline behavior).
            store.set_epoch_deadline(ticks);
        }
        store
    }

    /// Get plugin spec by instantiating the component and calling spec()
    fn get_plugin_spec(
        plugin_pre: &PluginPre<ComponentStoreData>,
        path: &Path,
        memory_limit: u64,
        timeout_ticks: Option<u64>,
    ) -> Result<bindings::nginx_lint::plugin::types::PluginSpec, PluginError> {
        let mut store = Self::create_store(plugin_pre.engine(), memory_limit, timeout_ticks);
        let plugin = plugin_pre
            .instantiate(&mut store)
            .map_err(|e| PluginError::instantiate_error(path, e.to_string()))?;

        plugin
            .call_spec(&mut store)
            .map_err(|e| PluginError::execution_error(path, format!("spec() call failed: {}", e)))
    }

    /// Get the specs of a `plugin-rules` component by instantiating it and
    /// calling specs(). A component with no rules, a rule without a name,
    /// or two rules with the same name is rejected: the host addresses
    /// rules by name, both in `check` and in the configuration.
    fn get_rule_specs(
        rules_pre: &PluginRulesPre<ComponentStoreData>,
        path: &Path,
        memory_limit: u64,
        timeout_ticks: Option<u64>,
    ) -> Result<Vec<bindings::nginx_lint::plugin::types::PluginSpec>, PluginError> {
        let mut store = Self::create_store(rules_pre.engine(), memory_limit, timeout_ticks);
        let rules = rules_pre
            .instantiate(&mut store)
            .map_err(|e| PluginError::instantiate_error(path, e.to_string()))?;

        let specs = rules.call_specs(&mut store).map_err(|e| {
            PluginError::execution_error(path, format!("specs() call failed: {}", e))
        })?;

        if specs.is_empty() {
            return Err(PluginError::invalid_plugin_spec(
                path,
                "specs() returned no rules",
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for spec in &specs {
            let name = sanitize_text(&spec.name);
            if name.is_empty() {
                return Err(PluginError::invalid_plugin_spec(
                    path,
                    "specs() returned a rule with an empty name",
                ));
            }
            // The host asks for a rule by its sanitized name and the
            // component matches on the name it declared; a name the
            // sanitizer alters would never match, and the rule would load
            // and then silently never run. The message shows the name as
            // declared, so the author can see which character is at
            // fault; `{:?}` escapes it rather than writing it to the
            // terminal.
            if name != spec.name {
                return Err(PluginError::invalid_plugin_spec(
                    path,
                    format!(
                        "specs() returned the rule name {:?}, which contains control characters",
                        spec.name
                    ),
                ));
            }
            if !seen.insert(name.clone()) {
                return Err(PluginError::invalid_plugin_spec(
                    path,
                    format!("specs() returned the rule name '{}' twice", name),
                ));
            }
        }
        Ok(specs)
    }

    /// Execute the check function using resource-based config access.
    /// `asked` names the rules to run, for a `plugin-rules` component: this
    /// one alone, or every rule of the component the linter has enabled.
    fn execute_check(
        &self,
        asked: &[&str],
        config: Arc<Config>,
        file_path: &Path,
    ) -> Result<Vec<LintError>, PluginError> {
        // The deadline is per rule: a call that checks several rules of the
        // component gets each rule's budget, so a component is not cut off
        // for carrying many rules — up to a cap, since the number of rules
        // is the component's to declare. A call that does hit the deadline
        // is retried by the linter one rule at a time, each with its own,
        // so the most an untrusted component can spend on a file is the
        // batch attempt plus the per-rule deadlines: about twice what the
        // per-rule calls alone allowed, on the failure path only. That is
        // an upper bound, not a cost: a component that works returns in
        // milliseconds, and only one that hangs is waited for — up to the
        // scaled deadline for the batch, and then the per-rule deadlines
        // one after another, where the per-rule calls ran in parallel. The
        // memory limit is not scaled: it bounds what one instance may
        // hold, and a batched call reconstructs the config once — pruned
        // to the union of the asked rules' relevant directives, so larger
        // than any one rule's slice but at most the whole file, which a
        // rule declaring no pruning gets on its own. A component that
        // does exceed it when batched falls back to one rule at a time,
        // for good (see run_batch).
        let timeout_ticks = self
            .timeout_ticks
            .map(|ticks| ticks.saturating_mul((asked.len() as u64).clamp(1, DEADLINE_RULES_CAP)));
        let mut store = Self::create_store(self.exports.engine(), self.memory_limit, timeout_ticks);

        // Create config resource handle
        let config_resource = store
            .data_mut()
            .table
            .push(ConfigResource { config })
            .map_err(|e| {
                PluginError::execution_error(
                    &self.path,
                    format!("Failed to create config resource: {}", e),
                )
            })?;

        let path_str = file_path.to_string_lossy().to_string();
        let check_failed = |e: wasmtime::Error| {
            // Epoch deadline expiry surfaces as Trap::Interrupt
            if e.downcast_ref::<Trap>() == Some(&Trap::Interrupt) {
                PluginError::timeout(&self.path)
            } else {
                PluginError::execution_error(&self.path, format!("check() failed: {}", e))
            }
        };
        let wit_errors = match &self.exports {
            Exports::Rules {
                pre,
                rules: carried,
                ..
            } => {
                let rules = pre
                    .instantiate(&mut store)
                    .map_err(|e| PluginError::instantiate_error(&self.path, e.to_string()))?;
                let asked_owned: Vec<String> = asked.iter().map(|name| name.to_string()).collect();
                let mut errors = rules
                    .call_check(&mut store, config_resource, &path_str, &asked_owned)
                    .map_err(check_failed)?;
                // Only the asked rules' findings. A component that ignores
                // the list and reports every rule would otherwise have the
                // findings of rules that are disabled, so a sibling's
                // findings are dropped. A finding under a name the
                // component does not carry at all is kept: that is a rule
                // whose spec and findings disagree on its name, which
                // test-plugins diagnoses from what it gets back.
                errors.retain(|e| {
                    let rule = sanitize_text(&e.rule);
                    asked.contains(&rule.as_str()) || !carried.iter().any(|(name, _)| *name == rule)
                });
                errors
            }
            Exports::Plugin(pre) => {
                let plugin = pre
                    .instantiate(&mut store)
                    .map_err(|e| PluginError::instantiate_error(&self.path, e.to_string()))?;
                plugin
                    .call_check(&mut store, config_resource, &path_str)
                    .map_err(check_failed)?
            }
        };

        // Note: The config resource and any directive resources created during
        // the check are cleaned up when `store` is dropped at function exit.
        // A fresh store is created for each execute_check call, so resources
        // do not leak across lint runs.

        Ok(wit_errors.iter().map(convert_lint_error).collect())
    }

    /// Run a check for the asked rules with a shared config handle,
    /// converting a failure into a reported lint error under each of them:
    /// a rule the user asked for — with `--rule-only`, say — is what they
    /// look for the failure under.
    fn run_check(&self, asked: &[&str], config: Arc<Config>, path: &Path) -> Vec<LintError> {
        match self.execute_check(asked, config, path) {
            Ok(errors) => errors,
            Err(e) => asked
                .iter()
                .map(|name| {
                    LintError::new(
                        name,
                        self.category_of(name),
                        &format!("Plugin execution failed: {}", e),
                        Severity::Error,
                    )
                })
                .collect(),
        }
    }

    /// The category of one of the component's rules, for a failure
    /// reported under that rule; this rule's own for a name the component
    /// does not carry, which the linter never asks for.
    fn category_of(&self, name: &str) -> &str {
        match &self.exports {
            Exports::Rules { rules, .. } => rules
                .iter()
                .find(|(rule, _)| rule == name)
                .map(|(_, category)| category.as_str())
                .unwrap_or(self.category),
            Exports::Plugin(_) => self.category,
        }
    }
}

/// The most rules a batched call's deadline is scaled by. A component
/// declares its own rules, so without a bound the deadline would be the
/// component's to set; past this many rules, the remaining ones share.
const DEADLINE_RULES_CAP: u64 = 32;

/// The next component id (see `Exports::Rules::id`). Ids only have to be
/// distinct within a process; components are never unloaded.
fn next_component_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl LintRule for ComponentLintRule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn category(&self) -> &'static str {
        self.category
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn check(&self, config: &Config, path: &Path) -> Vec<LintError> {
        // Direct callers only have a borrowed Config, so this pays a deep
        // clone. The linter passes a shared handle via check_shared instead.
        self.run_check(&[self.name], Arc::new(config.clone()), path)
    }

    fn wants_shared_config(&self) -> bool {
        true
    }

    fn check_shared(&self, config: &Arc<Config>, path: &Path) -> Vec<LintError> {
        self.run_check(&[self.name], config.clone(), path)
    }

    /// Every rule of a `plugin-rules` component shares its id, so the
    /// linter checks them in one call; a rule of the original `plugin`
    /// world is the component, and has none.
    fn batch_key(&self) -> Option<nginx_lint_common::linter::BatchKey> {
        match &self.exports {
            Exports::Rules { id, .. } => {
                Some(nginx_lint_common::linter::BatchKey::new::<Self>(*id))
            }
            Exports::Plugin(_) => None,
        }
    }

    /// One call for every asked rule. A failure — a rule that traps, or
    /// the call timing out — is the linter's to handle: it checks the
    /// rules one at a time instead (see `run_batch`), so a failing rule
    /// takes only its own findings with it and is the only one reported
    /// failed, as it was when every rule was its own call. That costs the
    /// batch attempt plus what the rules cost before, bounded by their
    /// deadlines; the batched call is the fast path.
    ///
    /// A rule of the `plugin` world has no batch key and no siblings, so
    /// like the default it answers only for its own name: its `check`
    /// takes no list and would report itself whatever was asked.
    fn check_shared_batch(
        &self,
        names: &[&str],
        config: &Arc<Config>,
        path: &Path,
    ) -> Result<Vec<LintError>, String> {
        if let Exports::Plugin(_) = &self.exports {
            match names {
                [] => return Ok(Vec::new()),
                [name] if *name == self.name() => {}
                _ => {
                    return Err(format!(
                        "{} is a rule of the plugin world and checks only itself",
                        self.name()
                    ));
                }
            }
        }
        self.execute_check(names, config.clone(), path)
            .map_err(|e| e.to_string())
    }

    fn why(&self) -> Option<&str> {
        self.spec.why.as_deref()
    }

    fn bad_example(&self) -> Option<&str> {
        self.spec.bad_example.as_deref()
    }

    fn good_example(&self) -> Option<&str> {
        self.spec.good_example.as_deref()
    }

    fn references(&self) -> Option<Vec<String>> {
        self.spec.references.clone()
    }

    fn min_nginx_version(&self) -> Option<&str> {
        self.spec.min_nginx_version.as_deref()
    }

    fn max_nginx_version(&self) -> Option<&str> {
        self.spec.max_nginx_version.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::host::config_api;
    use wasmtime::component::Resource;

    /// Load a real compiled builtin plugin by rule-table name (e.g.
    /// `"autoindex_enabled"`), skipping the test if `make build-plugins`
    /// (or `make collect-plugins`) has not been run.
    fn load_real_plugin(name: &str) -> Option<ComponentLintRule> {
        use crate::plugin::{CompilationCache, PluginLoader};

        let wasm_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("target/builtin-plugins/{name}.wasm"));
        if !wasm_path.exists() {
            eprintln!("SKIP: run `make build-plugins` first (missing {wasm_path:?})");
            return None;
        }
        let loader = PluginLoader::new_with_cache(CompilationCache::Disabled).unwrap();
        let bytes = std::fs::read(&wasm_path).unwrap();
        let mut rules = loader
            .load_component_from_bytes(&wasm_path, &bytes)
            .unwrap();
        assert_eq!(rules.len(), 1, "builtin plugins carry one rule each");
        rules.pop()
    }

    /// A component of the original `plugin` world still loads as one rule,
    /// and runs. Nothing in the tree builds that world any more (every SDK
    /// targets plugin-rules), so the component is a committed fixture; see
    /// tests/fixtures/plugins/plugin-world/README.md.
    #[test]
    fn plugin_world_component_still_loads() {
        use crate::plugin::{CompilationCache, PluginLoader};

        let wasm_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/plugins/plugin-world/server-tokens-enabled-lua.wasm");
        let loader = PluginLoader::new_with_cache(CompilationCache::Disabled).unwrap();
        let bytes = std::fs::read(&wasm_path).unwrap();
        let rules = loader
            .load_component_from_bytes(&wasm_path, &bytes)
            .unwrap();

        assert_eq!(rules.len(), 1);
        let rule = &rules[0];
        assert!(!rule.is_plugin_rules());
        assert_eq!(rule.name(), "server-tokens-enabled-lua");
        assert_eq!(rule.category(), "security");

        let bad = rule.bad_example().expect("the spec carries a bad example");
        let config = Arc::new(crate::parser::parse_string(bad).unwrap());
        let errors = rule.check_shared(&config, Path::new("bad.conf"));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].rule, "server-tokens-enabled-lua");
        assert_eq!(errors[0].fixes.len(), 1);

        // The fix, applied the way --fix applies it, resolves the finding
        let fixes: Vec<&crate::linter::Fix> = errors[0].fixes.iter().collect();
        let (fixed, applied) = crate::apply_fixes_to_content(bad, &fixes);
        assert_eq!(applied, 1);
        let config = Arc::new(crate::parser::parse_string(&fixed).unwrap());
        assert!(
            rule.check_shared(&config, Path::new("fixed.conf"))
                .is_empty(),
            "after fixing:\n{fixed}"
        );

        let good = rule
            .good_example()
            .expect("the spec carries a good example");
        let config = Arc::new(crate::parser::parse_string(good).unwrap());
        assert!(
            rule.check_shared(&config, Path::new("good.conf"))
                .is_empty()
        );

        // Batched, it answers only for itself: its check takes no list
        let config = Arc::new(crate::parser::parse_string(bad).unwrap());
        let own = rule
            .check_shared_batch(
                &["server-tokens-enabled-lua"],
                &config,
                Path::new("bad.conf"),
            )
            .unwrap();
        assert_eq!(own.len(), 1);
        assert!(
            rule.check_shared_batch(&["other"], &config, Path::new("bad.conf"))
                .is_err()
        );
        assert!(
            rule.check_shared_batch(
                &["server-tokens-enabled-lua", "other"],
                &config,
                Path::new("bad.conf")
            )
            .is_err()
        );
    }

    /// What one rule's failure does to its siblings' findings in the same
    /// call is the Lua runtime's to decide, and it isolates them. This
    /// calls the Lua SDK's failing-rules component with all three of its
    /// rules at once, as the linter does: the working rule's finding has
    /// to come back beside the two failures, each under its own rule.
    /// Skips unless `make -C plugins/nginx-lint-plugin-sdk test-e2e` has
    /// built the component.
    #[test]
    fn lua_rules_fail_one_at_a_time() {
        use crate::plugin::{CompilationCache, PluginLoader};

        let wasm_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("plugins/nginx-lint-plugin-sdk/tests/failing-rules/failing-rules.wasm");
        if !wasm_path.exists() {
            eprintln!("SKIP: run `make -C plugins/nginx-lint-plugin-sdk test-e2e` first");
            return;
        }
        let loader = PluginLoader::new_with_cache(CompilationCache::Disabled).unwrap();
        let bytes = std::fs::read(&wasm_path).unwrap();
        let rules = loader
            .load_component_from_bytes(&wasm_path, &bytes)
            .unwrap();
        let names: Vec<&str> = rules.iter().map(|rule| rule.name()).collect();
        assert_eq!(names, ["ok-rule", "shape-rule", "throw-rule"]);

        // Every rule shares the component; call it directly with all three
        let rule = &rules[0];
        let Exports::Rules { pre, .. } = &rule.exports else {
            panic!("the Lua runtime targets plugin-rules");
        };
        let mut store =
            ComponentLintRule::create_store(pre.engine(), rule.memory_limit, rule.timeout_ticks);
        let component = pre.instantiate(&mut store).unwrap();
        let config =
            Arc::new(crate::parser::parse_string("http {\n    server_tokens on;\n}\n").unwrap());
        let handle = store
            .data_mut()
            .table
            .push(ConfigResource { config })
            .unwrap();
        let asked: Vec<String> = names.iter().map(|name| name.to_string()).collect();
        let findings = component
            .call_check(&mut store, handle, "t.conf", &asked)
            .unwrap();

        let summary: Vec<(String, String)> = findings
            .iter()
            .map(|e| (e.rule.clone(), e.message.clone()))
            .collect();
        assert_eq!(summary.len(), 3, "{summary:?}");
        assert_eq!(summary[0], ("ok-rule".to_string(), "found".to_string()));
        assert_eq!(summary[1].0, "shape-rule");
        assert!(
            summary[1]
                .1
                .contains("finding 2 returned by check() is a number"),
            "{summary:?}"
        );
        assert_eq!(summary[2].0, "throw-rule");
        assert!(summary[2].1.contains("boom"), "{summary:?}");
    }

    /// Load the two-rule example component, skipping the test if
    /// `make -C plugins/rust/security-rules build` has not been run.
    fn load_real_two_rule_plugin() -> Option<Vec<ComponentLintRule>> {
        use crate::plugin::{CompilationCache, PluginLoader};

        let wasm_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("plugins/rust/security-rules/security-rules.wasm");
        if !wasm_path.exists() {
            eprintln!(
                "SKIP: run `make -C plugins/rust/security-rules build` first (missing {wasm_path:?})"
            );
            return None;
        }
        let loader = PluginLoader::new_with_cache(CompilationCache::Disabled).unwrap();
        let bytes = std::fs::read(&wasm_path).unwrap();
        Some(
            loader
                .load_component_from_bytes(&wasm_path, &bytes)
                .unwrap(),
        )
    }

    /// A `plugin-rules` component loads as one rule per spec, in spec
    /// order, each carrying its own metadata.
    #[test]
    fn two_rule_plugin_loads_as_one_rule_per_spec() {
        let Some(rules) = load_real_two_rule_plugin() else {
            return;
        };

        let names: Vec<&str> = rules.iter().map(|rule| rule.name()).collect();
        assert_eq!(names, ["server-tokens-enabled-rs", "autoindex-enabled-rs"]);
        assert!(rules.iter().all(|rule| rule.is_plugin_rules()));
        assert!(rules.iter().all(|rule| rule.category() == "security"));
        assert!(rules[0].description().contains("server_tokens"));
        assert!(rules[1].description().contains("autoindex"));
        assert!(rules[1].bad_example().unwrap().contains("autoindex on;"));
    }

    /// Through the linter, the two rules of the component are checked in
    /// one call — the batched path, `check_shared_batch` with both names —
    /// and each finding comes back under its rule.
    #[test]
    fn two_rule_plugin_is_linted_in_one_call() {
        let Some(rules) = load_real_two_rule_plugin() else {
            return;
        };
        assert_eq!(rules[0].batch_key(), rules[1].batch_key());
        assert!(rules[0].batch_key().is_some());

        let src = "http { server_tokens on; server { location / { autoindex on; } } }";
        let config = Arc::new(crate::parser::parse_string(src).unwrap());

        // The batched call itself succeeds with both findings — asserted
        // directly, since through the linter a failed batch would fall back
        // to one rule at a time and report the same
        let both = ["server-tokens-enabled-rs", "autoindex-enabled-rs"];
        let mut batched = rules[0]
            .check_shared_batch(&both, &config, Path::new("test.conf"))
            .expect("the batched check succeeds");
        batched.sort_by(|x, y| x.rule.cmp(&y.rule));
        let names: Vec<&str> = batched.iter().map(|e| e.rule.as_str()).collect();
        assert_eq!(names, ["autoindex-enabled-rs", "server-tokens-enabled-rs"]);
        assert!(batched.iter().all(|e| e.fixes.len() == 1), "{batched:?}");

        // And the linter routes the two rules through it
        let mut linter = crate::linter::Linter::new();
        for rule in rules {
            linter.add_rule(Box::new(rule));
        }
        let mut errors = linter.lint(&config, Path::new("test.conf"));
        errors.sort_by(|x, y| x.rule.cmp(&y.rule));
        assert_eq!(
            errors.iter().map(|e| e.rule.as_str()).collect::<Vec<_>>(),
            names
        );
    }

    /// Each rule of the component reports only its own findings: the host asks
    /// the component for that one rule, and keeps only findings that name it.
    #[test]
    fn two_rule_plugin_rules_report_their_own_findings_only() {
        let Some(rules) = load_real_two_rule_plugin() else {
            return;
        };

        let src = "http { server_tokens on; server { location / { autoindex on; } } }";
        let config = Arc::new(crate::parser::parse_string(src).unwrap());
        for rule in &rules {
            let errors = rule.check_shared(&config, Path::new("test.conf"));
            assert_eq!(errors.len(), 1, "{}: {errors:?}", rule.name());
            assert_eq!(errors[0].rule, rule.name());
        }
    }

    /// End-to-end check that `relevant_directives()`-based filtering (see
    /// `autoindex_enabled`'s `snapshot_filtered` opt-in) does not change
    /// observable behavior: same warnings, same ancestor-context handling
    /// (`is_inside("http")`), as running through the real host+WASM stack
    /// (not the plugin's native unit tests, which call `Plugin::check`
    /// directly and never exercise `snapshot_filtered` or the pruning in
    /// `flatten_item_to_wit_filtered`).
    #[test]
    fn autoindex_enabled_filtered_snapshot_matches_expected_behavior() {
        let Some(rule) = load_real_plugin("autoindex_enabled") else {
            return;
        };

        let cases: &[(&str, usize)] = &[
            // Warns: autoindex on, inside http.
            ("http { server { location / { autoindex on; } } }", 1),
            // No warning: explicitly off.
            ("http { server { location / { autoindex off; } } }", 0),
            // No warning: absent entirely (empty filtered snapshot).
            ("http { server { listen 80; } }", 0),
            // No warning: autoindex outside http context. This is the case
            // that would break if ancestor pruning dropped the "stream"
            // ancestor needed for is_inside("http") to return false.
            ("stream { autoindex on; }", 0),
            // Warns on both, each under its own location ancestor chain.
            (
                "http { server { location /a { autoindex on; } location /b { autoindex on; } } }",
                2,
            ),
        ];

        for (src, expected_count) in cases {
            let config = Arc::new(crate::parser::parse_string(src).unwrap());
            let errors = rule.check_shared(&config, Path::new("test.conf"));
            assert_eq!(
                errors.len(),
                *expected_count,
                "unexpected error count for {src:?}: {errors:?}"
            );
        }
    }

    /// `include_context` (the `--context` CLI flag / included-file scenario)
    /// is a separate top-level `Config` field, not part of the pruned items
    /// tree, and `HostConfig::snapshot_filtered` copies it unconditionally
    /// regardless of the `names` filter. Confirms a directive matched only
    /// via `include_context` (no literal ancestor directive in this file)
    /// still resolves `is_inside` correctly under filtering, using the real
    /// compiled server-tokens-enabled.wasm (relevant_directives() ->
    /// ["http", "server_tokens"]).
    #[test]
    fn server_tokens_enabled_filtered_snapshot_respects_include_context() {
        let Some(rule) = load_real_plugin("server_tokens_enabled") else {
            return;
        };

        let cases: &[(&str, &[&str], usize)] = &[
            // Warns: server_tokens on, reached only via include_context
            // ("http"), no literal http directive in this file at all.
            ("server_tokens on;", &["http"], 1),
            // No warning: same include_context, but off.
            ("server_tokens off;", &["http"], 0),
            // No warning: matches the plugin's own documented behavior
            // (skip the "missing server_tokens" warning for included
            // files — the parent config should set it) — unaffected by
            // filtering either way, since this file has no server_tokens
            // directive and no literal http block.
            ("listen 80;", &["http"], 0),
            // No warning: include_context doesn't mention http, so
            // is_inside("http") must stay false even though "server_tokens"
            // matches the filter.
            ("server_tokens on;", &["stream"], 0),
        ];

        for (src, include_context, expected_count) in cases {
            let mut config = crate::parser::parse_string(src).unwrap();
            config.include_context = include_context.iter().map(|s| s.to_string()).collect();
            let config = Arc::new(config);
            let errors = rule.check_shared(&config, Path::new("test.conf"));
            assert_eq!(
                errors.len(),
                *expected_count,
                "unexpected error count for {src:?} with include_context {include_context:?}: {errors:?}"
            );
        }
    }

    /// (rule table name, plugin directory relative to CARGO_MANIFEST_DIR)
    /// for every builtin, matching the declaration order in
    /// `src/plugin/builtin.rs`'s `PLUGIN_ENTRIES` (checked by a test there).
    const ALL_BUILTIN_PLUGIN_DIRS: &[(&str, &str)] = &[
        (
            "server_tokens_enabled",
            "plugins/builtin/security/server_tokens_enabled",
        ),
        (
            "autoindex_enabled",
            "plugins/builtin/security/autoindex_enabled",
        ),
        (
            "gzip_not_enabled",
            "plugins/builtin/best_practices/gzip_not_enabled",
        ),
        (
            "duplicate_directive",
            "plugins/builtin/syntax/duplicate_directive",
        ),
        (
            "space_before_semicolon",
            "plugins/builtin/style/space_before_semicolon",
        ),
        (
            "trailing_whitespace",
            "plugins/builtin/style/trailing_whitespace",
        ),
        ("block_lines", "plugins/builtin/style/block_lines"),
        (
            "proxy_pass_domain",
            "plugins/builtin/best_practices/proxy_pass_domain",
        ),
        (
            "upstream_server_no_resolve",
            "plugins/builtin/best_practices/upstream_server_no_resolve",
        ),
        (
            "directive_inheritance",
            "plugins/builtin/best_practices/directive_inheritance",
        ),
        (
            "root_in_location",
            "plugins/builtin/best_practices/root_in_location",
        ),
        (
            "alias_location_slash_mismatch",
            "plugins/builtin/best_practices/alias_location_slash_mismatch",
        ),
        (
            "proxy_pass_with_uri",
            "plugins/builtin/best_practices/proxy_pass_with_uri",
        ),
        (
            "proxy_keepalive",
            "plugins/builtin/best_practices/proxy_keepalive",
        ),
        (
            "try_files_with_proxy",
            "plugins/builtin/best_practices/try_files_with_proxy",
        ),
        (
            "if_is_evil_in_location",
            "plugins/builtin/best_practices/if_is_evil_in_location",
        ),
        (
            "unreachable_location",
            "plugins/builtin/best_practices/unreachable_location",
        ),
        (
            "missing_error_log",
            "plugins/builtin/best_practices/missing_error_log",
        ),
        (
            "deprecated_ssl_protocol",
            "plugins/builtin/security/deprecated_ssl_protocol",
        ),
        (
            "weak_ssl_ciphers",
            "plugins/builtin/security/weak_ssl_ciphers",
        ),
        (
            "invalid_directive_context",
            "plugins/builtin/syntax/invalid_directive_context",
        ),
        (
            "map_missing_default",
            "plugins/builtin/best_practices/map_missing_default",
        ),
        (
            "ssl_on_deprecated",
            "plugins/builtin/deprecation/ssl_on_deprecated",
        ),
        (
            "listen_http2_deprecated",
            "plugins/builtin/deprecation/listen_http2_deprecated",
        ),
        (
            "proxy_missing_host_header",
            "plugins/builtin/best_practices/proxy_missing_host_header",
        ),
        (
            "client_max_body_size_not_set",
            "plugins/builtin/best_practices/client_max_body_size_not_set",
        ),
        ("nginx_rift", "plugins/builtin/security/nginx_rift"),
        (
            "map_unnamed_capture",
            "plugins/builtin/security/map_unnamed_capture",
        ),
    ];

    /// `ALL_BUILTIN_PLUGIN_DIRS` is a third, hand-maintained table alongside
    /// `PLUGIN_ENTRIES` (builtin.rs) and `BUILTIN_PLUGIN_NAMES` (mod.rs;
    /// itself already checked against `PLUGIN_ENTRIES` by
    /// `test_plugin_entries_match_builtin_plugin_names` in builtin.rs).
    /// Guard against it silently going stale: a plugin added, removed, or
    /// renamed here without updating this table would make
    /// `all_builtin_plugins_match_expected_behavior_via_real_wasm` quietly
    /// skip it (`load_real_plugin` just returns `None`) instead of failing.
    #[test]
    fn test_all_builtin_plugin_dirs_matches_builtin_plugin_names() {
        let table_names: Vec<String> = ALL_BUILTIN_PLUGIN_DIRS
            .iter()
            .map(|(wasm_name, _)| wasm_name.replace('_', "-"))
            .collect();
        let table_names: Vec<&str> = table_names.iter().map(String::as_str).collect();
        assert_eq!(
            table_names,
            crate::plugin::BUILTIN_PLUGIN_NAMES,
            "ALL_BUILTIN_PLUGIN_DIRS (component_rule.rs test) and BUILTIN_PLUGIN_NAMES \
             (plugin/mod.rs) must list the same rules in the same order (modulo '-' vs '_')"
        );
    }

    /// Regression net for the `relevant_directives()` rollout (2026-07-09):
    /// for every builtin plugin, run its OWN `tests/fixtures/*/{error,expected}`
    /// pairs and `examples/{bad,good}.conf` through the REAL compiled .wasm
    /// (not `Plugin::check()` directly, which every plugin's own unit tests
    /// already do and which never exercises `snapshot_filtered` or the
    /// pruning in `flatten_item_to_wit_filtered`). `error`/`bad.conf` must
    /// produce at least one error from that plugin's rule; `expected`/
    /// `good.conf` must produce none. This validates every builtin equally,
    /// whether or not it overrides `relevant_directives()`.
    #[test]
    fn all_builtin_plugins_match_expected_behavior_via_real_wasm() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        for (wasm_name, plugin_dir) in ALL_BUILTIN_PLUGIN_DIRS {
            // Skip (not fail) when `make build-plugins` hasn't been run —
            // CI's plain `cargo test` job doesn't build the .wasm files, so
            // this must degrade to a no-op there, same as `load_real_plugin`'s
            // other callers (phase_timing, autoindex_enabled_filtered_...).
            let Some(rule) = load_real_plugin(wasm_name) else {
                continue;
            };
            let rule_name = rule.name().to_string();
            let plugin_dir = manifest_dir.join(plugin_dir);

            let mut cases: Vec<(String, PathBuf, usize)> = Vec::new();

            let fixtures_dir = plugin_dir.join("tests/fixtures");
            if fixtures_dir.is_dir() {
                for entry in std::fs::read_dir(&fixtures_dir).unwrap() {
                    let case_path = entry.unwrap().path();
                    if !case_path.is_dir() {
                        continue;
                    }
                    let case_name = case_path.file_name().unwrap().to_string_lossy().to_string();
                    let error_path = case_path.join("error/nginx.conf");
                    if error_path.exists() {
                        cases.push((format!("{wasm_name}/{case_name}/error"), error_path, 1));
                    }
                    let expected_path = case_path.join("expected/nginx.conf");
                    if expected_path.exists() {
                        cases.push((
                            format!("{wasm_name}/{case_name}/expected"),
                            expected_path,
                            0,
                        ));
                    }
                }
            }

            let bad_path = plugin_dir.join("examples/bad.conf");
            if bad_path.exists() {
                cases.push((format!("{wasm_name}/examples/bad"), bad_path, 1));
            }
            let good_path = plugin_dir.join("examples/good.conf");
            if good_path.exists() {
                cases.push((format!("{wasm_name}/examples/good"), good_path, 0));
            }

            for (label, path, min_or_exact_zero) in cases {
                let content = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"));
                let config = Arc::new(crate::parser::parse_string(&content).unwrap_or_else(|e| {
                    panic!("failed to parse {path:?}: {e}");
                }));
                let errors = rule.check_shared(&config, Path::new("test.conf"));
                let rule_errors: Vec<_> = errors.iter().filter(|e| e.rule == rule_name).collect();

                if min_or_exact_zero == 0 {
                    assert!(
                        rule_errors.is_empty(),
                        "{label}: expected no {rule_name} errors, got: {rule_errors:?}"
                    );
                } else {
                    assert!(
                        !rule_errors.is_empty(),
                        "{label}: expected at least one {rule_name} error, got none"
                    );
                }
            }
        }
    }

    /// Measurement harness for the WIT-boundary cost investigation: what
    /// a check costs, split into instantiation, host-side conversion and
    /// the guest's own work. Run manually with:
    /// cargo test --release --features plugins --lib phase_timing -- --ignored --nocapture
    /// It measures the builtin server_tokens_enabled component, or the one
    /// named by NGINX_LINT_BENCH_COMPONENT — a Python or TypeScript
    /// component, say, whose instantiation is the runtime's.
    #[test]
    #[ignore]
    fn phase_timing() {
        use crate::plugin::{CompilationCache, PluginLoader};
        use std::time::Instant;

        let wasm_path = match std::env::var_os("NGINX_LINT_BENCH_COMPONENT") {
            Some(path) => std::path::PathBuf::from(path),
            None => std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/builtin-plugins/server_tokens_enabled.wasm"),
        };
        if !wasm_path.exists() {
            eprintln!("SKIP: run `make build-plugins` first (missing {wasm_path:?})");
            return;
        }

        // WASI allowed, for a Go component; the builtins import none
        let loader = PluginLoader::new_with_cache(CompilationCache::Disabled)
            .unwrap()
            .with_wasi(true);
        let bytes = std::fs::read(&wasm_path).unwrap();
        let rule = loader
            .load_component_from_bytes(&wasm_path, &bytes)
            .unwrap()
            .pop()
            .unwrap();
        println!("component: {} ({} bytes)", wasm_path.display(), bytes.len());

        for n_servers in [30, 300] {
            let mut src = String::from("http {\n  gzip on;\n");
            for i in 0..n_servers {
                src.push_str(&format!(
                    "  server {{\n    listen 80;\n    server_name s{i}.example.com;\n    server_tokens off;\n    location / {{\n      proxy_pass http://127.0.0.1:8080;\n      proxy_set_header Host $host;\n    }}\n    error_log /var/log/nginx/error.log;\n  }}\n"
                ));
            }
            src.push_str("}\n");
            let config = crate::parser::parse_string(&src).unwrap();
            let n_directives = config.all_directives().count();
            let shared = Arc::new(config);
            let iters = 200;

            // Phase A: store + instantiate only
            let start = Instant::now();
            for _ in 0..iters {
                let mut store = ComponentLintRule::create_store(
                    rule.exports.engine(),
                    rule.memory_limit,
                    rule.timeout_ticks,
                );
                match &rule.exports {
                    Exports::Rules { pre, .. } => {
                        pre.instantiate(&mut store).unwrap();
                    }
                    Exports::Plugin(pre) => {
                        pre.instantiate(&mut store).unwrap();
                    }
                }
            }
            let phase_a = start.elapsed() / iters;

            // Phase B: full check (instantiate + guest reconstruct + rule logic)
            let start = Instant::now();
            for _ in 0..iters {
                let _ = rule.check_shared(&shared, Path::new("test.conf"));
            }
            let phase_b = start.elapsed() / iters;

            // Phase C: host-side conversion only (what the host does when the
            // guest walks items/data/block-items), no WIT lowering, no guest
            let start = Instant::now();
            for _ in 0..iters {
                let mut data = ComponentStoreData {
                    limits: StoreLimitsBuilder::new().build(),
                    table: ResourceTable::new(),
                    wasi: None,
                };
                let cfg_res = data
                    .table
                    .push(ConfigResource {
                        config: shared.clone(),
                    })
                    .unwrap();
                let items = config_api::HostConfig::items(&mut data, cfg_res);
                walk_items(&mut data, items);
            }
            let phase_c = start.elapsed() / iters;

            println!(
                "directives={n_directives}: instantiate={phase_a:?} full_check={phase_b:?} host_conv={phase_c:?} guest+lowering={:?}",
                phase_b.saturating_sub(phase_a).saturating_sub(phase_c)
            );
        }

        fn walk_items(data: &mut ComponentStoreData, items: Vec<config_api::ConfigItem>) {
            for item in items {
                if let config_api::ConfigItem::DirectiveItem(handle) = item {
                    let alias = Resource::new_borrow(handle.rep());
                    let d = config_api::HostDirective::data(data, alias);
                    if d.has_block {
                        let alias = Resource::new_borrow(handle.rep());
                        let children = config_api::HostDirective::block_items(data, alias);
                        walk_items(data, children);
                    }
                }
            }
        }
    }

    /// Measures the real win from `relevant_directives()` filtering for one
    /// plugin (autoindex-enabled: `Some(&["autoindex"])`) against a large,
    /// realistic config where the target directive never appears (the
    /// common case for most files and most rules) versus where it appears
    /// once. Run manually with:
    /// cargo test --release --features plugins --lib filtered_snapshot_timing -- --ignored --nocapture
    #[test]
    #[ignore]
    fn filtered_snapshot_timing() {
        use std::time::Instant;

        let Some(rule) = load_real_plugin("autoindex_enabled") else {
            return;
        };

        for n_servers in [30, 300] {
            for autoindex_present in [false, true] {
                let mut src = String::from("http {\n  gzip on;\n");
                for i in 0..n_servers {
                    src.push_str(&format!(
                        "  server {{\n    listen 80;\n    server_name s{i}.example.com;\n    server_tokens off;\n    location / {{\n      proxy_pass http://127.0.0.1:8080;\n      proxy_set_header Host $host;\n    }}\n    error_log /var/log/nginx/error.log;\n  }}\n"
                    ));
                }
                if autoindex_present {
                    src.push_str(
                        "  server {\n    location /files {\n      autoindex on;\n    }\n  }\n",
                    );
                }
                src.push_str("}\n");
                let config = crate::parser::parse_string(&src).unwrap();
                let n_directives = config.all_directives().count();
                let shared = Arc::new(config);
                let iters = 200;

                let start = Instant::now();
                for _ in 0..iters {
                    let _ = rule.check_shared(&shared, Path::new("test.conf"));
                }
                let elapsed = start.elapsed() / iters;

                println!(
                    "directives={n_directives} autoindex_present={autoindex_present}: check={elapsed:?}"
                );
            }
        }
    }

    #[test]
    fn test_load_with_invalid_bytes() {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config).unwrap();
        let result = ComponentLintRule::load(
            &engine,
            PathBuf::from("test.wasm"),
            b"not a wasm component",
            256 * 1024 * 1024,
            Some(100),
            false,
        );
        assert!(matches!(result, Err(PluginError::CompileError { .. })));
    }
}
