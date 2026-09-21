//! Running the rules that share a [`batch_key`](LintRule::batch_key) in
//! one call: grouping them, the call itself, and the fallback to one rule
//! at a time when it fails.

use super::{BatchKey, LintError, LintRule, run_rule};
use crate::parser::ast::Config;
use std::path::Path;

/// Run a group of rules sharing a [`batch_key`](LintRule::batch_key) — as
/// [`batch_rules`] groups them — in one call, through the first rule's
/// [`check_shared_batch`](LintRule::check_shared_batch). A group of one is
/// run through [`run_rule`], so a rule with a key of its own costs nothing
/// extra; several keyless rules are not a group, and run one at a time.
///
/// Should the batched check fail, the rules are checked one at a time
/// instead — each reporting its own outcome, a failing rule taking only
/// its own findings with it, as when every rule was its own call — and
/// the failure is reported to stderr once per group, as `memo` remembers
/// it. From then on the group is checked one rule at a time without
/// trying the batch again: a component that fails only when checked for
/// several rules is a defect of the component, which `nginx-lint
/// test-plugins` also catches, and would otherwise cost the failed
/// attempt on every file.
pub fn run_batch(
    rules: &[&dyn LintRule],
    config: &Config,
    path: &Path,
    shared_config: &std::sync::OnceLock<std::sync::Arc<Config>>,
    memo: &BatchMemo,
) -> Vec<LintError> {
    match rules {
        [] => Vec::new(),
        [rule] => run_rule(*rule, config, path, shared_config),
        [first, ..] => {
            let names: Vec<&str> = rules.iter().map(|rule| rule.name()).collect();
            let shared = shared_config.get_or_init(|| std::sync::Arc::new(config.clone()));
            let one_at_a_time = || -> Vec<LintError> {
                rules
                    .iter()
                    .flat_map(|rule| rule.check_shared(shared, path))
                    .collect()
            };
            // A group is several rules sharing one key, which is what
            // batch_rules produces. Keyless rules handed here together are
            // not a group: they are checked one at a time, and remembered
            // as nothing, since they have no key to remember by.
            let Some(key) = first.batch_key() else {
                return one_at_a_time();
            };
            if memo.has_failed(key) {
                return one_at_a_time();
            }
            match first.check_shared_batch(&names, shared, path) {
                Ok(errors) => errors,
                Err(why) => {
                    if memo.remember_failed(key) {
                        eprintln!(
                            "Warning: checking {} together failed on {} ({}); checking them one at a time from now on",
                            names.join(", "),
                            path.display(),
                            why
                        );
                    }
                    one_at_a_time()
                }
            }
        }
    }
}

/// The groups whose batched check has failed, for [`run_batch`] to stop
/// attempting. Owned by whoever runs the rules — one per linter, so two
/// rule sets in one process, or two tests, never disable each other's
/// batching.
#[derive(Debug, Default)]
pub struct BatchMemo(std::sync::Mutex<std::collections::HashSet<BatchKey>>);

impl BatchMemo {
    fn failed(&self) -> std::sync::MutexGuard<'_, std::collections::HashSet<BatchKey>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether the group's batched check has failed before.
    pub fn has_failed(&self, key: BatchKey) -> bool {
        self.failed().contains(&key)
    }

    /// Record the group's batched check failing; true the first time.
    pub fn remember_failed(&self, key: BatchKey) -> bool {
        self.failed().insert(key)
    }

    /// Split every group whose batched check has failed into groups of
    /// one, so a caller running groups in parallel runs those rules in
    /// parallel again, as it did before they were grouped, rather than
    /// through [`run_batch`]'s one-at-a-time fallback in one task. A
    /// group of one has no key to look up.
    pub fn split_failed<'a>(
        &self,
        groups: Vec<Vec<&'a dyn LintRule>>,
    ) -> Vec<Vec<&'a dyn LintRule>> {
        let failed = self.failed();
        groups
            .into_iter()
            .flat_map(|group| {
                let key = group.first().and_then(|rule| rule.batch_key());
                match key {
                    Some(key) if group.len() > 1 && failed.contains(&key) => {
                        group.into_iter().map(|rule| vec![rule]).collect()
                    }
                    _ => vec![group],
                }
            })
            .collect()
    }
}

/// Group rules for [`run_batch`]: rules sharing a [`batch_key`](LintRule::batch_key)
/// form one group, in the order their first member appears; every other
/// rule is a group of its own, in place — including a keyed rule that
/// [wants the file content](LintRule::wants_content), which a batched
/// check has no way to hand it. The order of findings across groups is
/// the caller's to fix, as it is across rules today.
pub fn batch_rules<'a>(rules: &'a [Box<dyn LintRule>]) -> Vec<Vec<&'a dyn LintRule>> {
    let mut groups: Vec<Vec<&'a dyn LintRule>> = Vec::new();
    let mut by_key: std::collections::HashMap<BatchKey, usize> = std::collections::HashMap::new();
    for rule in rules {
        let key = rule.batch_key().filter(|_| !rule.wants_content());
        match key {
            Some(key) => match by_key.get(&key) {
                Some(&index) => groups[index].push(rule.as_ref()),
                None => {
                    by_key.insert(key, groups.len());
                    groups.push(vec![rule.as_ref()]);
                }
            },
            None => groups.push(vec![rule.as_ref()]),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Keyed(&'static str, Option<u64>);
    struct OtherKeyed(&'static str, u64);

    impl LintRule for OtherKeyed {
        fn name(&self) -> &'static str {
            self.0
        }
        fn category(&self) -> &'static str {
            "test"
        }
        fn description(&self) -> &'static str {
            "keyed by another implementor"
        }
        fn check(&self, _config: &Config, _path: &Path) -> Vec<LintError> {
            Vec::new()
        }
        fn batch_key(&self) -> Option<BatchKey> {
            Some(BatchKey::new::<OtherKeyed>(self.1))
        }
    }

    impl LintRule for Keyed {
        fn name(&self) -> &'static str {
            self.0
        }
        fn category(&self) -> &'static str {
            "test"
        }
        fn description(&self) -> &'static str {
            "keyed"
        }
        fn check(&self, _config: &Config, _path: &Path) -> Vec<LintError> {
            Vec::new()
        }
        fn batch_key(&self) -> Option<BatchKey> {
            self.1.map(BatchKey::new::<Keyed>)
        }
    }

    /// Two implementors numbering their groups the same way do not share
    /// one: the implementor's type is part of the key.
    #[test]
    fn batch_keys_of_different_implementors_never_collide() {
        let rules: Vec<Box<dyn LintRule>> =
            vec![Box::new(Keyed("a", Some(1))), Box::new(OtherKeyed("b", 1))];
        assert_eq!(batch_rules(&rules).len(), 2);
    }

    /// Rules sharing a key group at the position of their first member;
    /// the rest stay single, in place.
    #[test]
    fn batch_rules_groups_by_key_in_first_appearance_order() {
        let rules: Vec<Box<dyn LintRule>> = vec![
            Box::new(Keyed("a1", Some(1))),
            Box::new(Keyed("n", None)),
            Box::new(Keyed("b1", Some(2))),
            Box::new(Keyed("a2", Some(1))),
            Box::new(Keyed("m", None)),
            Box::new(Keyed("a3", Some(1))),
        ];
        let groups: Vec<Vec<&str>> = batch_rules(&rules)
            .iter()
            .map(|group| group.iter().map(|rule| rule.name()).collect())
            .collect();
        assert_eq!(
            groups,
            vec![vec!["a1", "a2", "a3"], vec!["n"], vec!["b1"], vec!["m"]]
        );
    }

    /// Two keyless rules never share a group, however similar.
    #[test]
    fn batch_rules_keeps_keyless_rules_apart() {
        let rules: Vec<Box<dyn LintRule>> =
            vec![Box::new(Keyed("x", None)), Box::new(Keyed("y", None))];
        assert_eq!(batch_rules(&rules).len(), 2);
    }

    /// A keyed rule that wants the file content stays a group of its own —
    /// a batched check could not hand it the content — while its siblings
    /// still batch.
    #[test]
    fn batch_rules_keeps_a_rule_that_wants_content_on_its_own() {
        struct WantsContent(u64);
        impl LintRule for WantsContent {
            fn name(&self) -> &'static str {
                "wants-content"
            }
            fn category(&self) -> &'static str {
                "test"
            }
            fn description(&self) -> &'static str {
                "keyed, but reads the source text"
            }
            fn check(&self, _config: &Config, _path: &Path) -> Vec<LintError> {
                Vec::new()
            }
            fn batch_key(&self) -> Option<BatchKey> {
                Some(BatchKey::new::<Keyed>(self.0))
            }
            fn wants_content(&self) -> bool {
                true
            }
        }
        let rules: Vec<Box<dyn LintRule>> = vec![
            Box::new(Keyed("a1", Some(1))),
            Box::new(WantsContent(1)),
            Box::new(Keyed("a2", Some(1))),
        ];
        let groups: Vec<Vec<&str>> = batch_rules(&rules)
            .iter()
            .map(|group| group.iter().map(|rule| rule.name()).collect())
            .collect();
        assert_eq!(groups, vec![vec!["a1", "a2"], vec!["wants-content"]]);
    }

    /// A group whose batched check has failed is split into groups of
    /// one; the others are left as they are.
    #[test]
    fn split_failed_breaks_up_only_the_failed_groups() {
        let rules: Vec<Box<dyn LintRule>> = vec![
            Box::new(Keyed("a1", Some(1))),
            Box::new(Keyed("b1", Some(2))),
            Box::new(Keyed("a2", Some(1))),
            Box::new(Keyed("b2", Some(2))),
        ];
        let memo = BatchMemo::default();
        memo.remember_failed(BatchKey::new::<Keyed>(1));
        let groups: Vec<Vec<&str>> = memo
            .split_failed(batch_rules(&rules))
            .iter()
            .map(|group| group.iter().map(|rule| rule.name()).collect())
            .collect();
        assert_eq!(groups, vec![vec!["a1"], vec!["a2"], vec!["b1", "b2"]]);
    }

    /// The default batched check answers only for the rule itself: asked
    /// for a sibling by name, it declines rather than report its own
    /// findings as the sibling's.
    #[test]
    fn default_batched_check_answers_only_its_own_name() {
        let rule = Keyed("a", Some(1));
        let config = std::sync::Arc::new(Config::default());
        let path = Path::new("t.conf");
        assert!(rule.check_shared_batch(&["a"], &config, path).is_ok());
        assert!(rule.check_shared_batch(&[], &config, path).is_ok());
        assert!(rule.check_shared_batch(&["b"], &config, path).is_err());
        assert!(rule.check_shared_batch(&["a", "b"], &config, path).is_err());
    }
}
