//! The check of a rule beside its siblings: what a component that behaves
//! differently when asked for several rules looks like to `test-plugins`.

use super::*;
use nginx_lint::parser::ast::Config;
use nginx_lint_common::linter::{BatchKey, Severity};
use std::sync::Arc;

/// Two rules of one component: each reports the directive it is named
/// for, and the batched check runs whichever rules are asked for over
/// the shared config — or fails, or leaks a sibling's directive into
/// what a rule sees, as the test dictates.
struct Member {
    name: &'static str,
    directive: &'static str,
    bad: Option<&'static str>,
    batch: BatchBehaviour,
}

#[derive(Clone, Copy)]
enum BatchBehaviour {
    Honest,
    Fails,
    /// When batched, the rule also reports its sibling's directive: what
    /// a rule that reads its siblings' pruning into its own does
    SeesSiblings,
    /// When batched, the finding is the same but its fix is not
    FixDiffers,
    /// When batched, the finding carries the first asked rule's
    /// category: what a runtime taking one spec for the whole call does
    CategoryDiffers,
}

fn report(name: &str, config: &Config, directive: &str) -> Vec<LintError> {
    config
        .all_directives()
        .filter(|d| d.name == directive)
        .map(|d| {
            LintError::new(name, "test", "found", Severity::Warning)
                .with_location(d.span.start.line, d.span.start.column)
        })
        .collect()
}

impl LintRule for Member {
    fn name(&self) -> &'static str {
        self.name
    }
    fn category(&self) -> &'static str {
        "test"
    }
    fn description(&self) -> &'static str {
        "a member"
    }
    fn bad_example(&self) -> Option<&str> {
        self.bad
    }
    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        report(self.name, config, self.directive)
    }
    fn batch_key(&self) -> Option<BatchKey> {
        Some(BatchKey::new::<Member>(1))
    }
    fn check_shared_batch(
        &self,
        names: &[&str],
        config: &Arc<Config>,
        _path: &Path,
    ) -> Result<Vec<LintError>, String> {
        match self.batch {
            BatchBehaviour::Fails => Err("trapped".to_string()),
            BatchBehaviour::Honest => Ok(names
                .iter()
                .flat_map(|name| {
                    // Each asked member reports its own directive
                    let directive = match *name {
                        "tokens" => "server_tokens",
                        _ => "autoindex",
                    };
                    report(name, config, directive)
                })
                .collect()),
            BatchBehaviour::SeesSiblings => Ok(names
                .iter()
                .flat_map(|name| {
                    let mut all = report(name, config, "server_tokens");
                    all.extend(report(name, config, "autoindex"));
                    all
                })
                .collect()),
            BatchBehaviour::FixDiffers => Ok(names
                .iter()
                .flat_map(|name| {
                    let directive = match *name {
                        "tokens" => "server_tokens",
                        _ => "autoindex",
                    };
                    report(name, config, directive).into_iter().map(|e| {
                        e.with_fix(nginx_lint_common::linter::Fix::replace_range(
                            0, 1, "batched",
                        ))
                    })
                })
                .collect()),
            BatchBehaviour::CategoryDiffers => Ok(names
                .iter()
                .flat_map(|name| {
                    let directive = match *name {
                        "tokens" => "server_tokens",
                        _ => "autoindex",
                    };
                    report(name, config, directive).into_iter().map(|mut e| {
                        e.category = "batched".to_string();
                        e
                    })
                })
                .collect()),
        }
    }
}

impl Clone for Member {
    fn clone(&self) -> Self {
        *self
    }
}
impl Copy for Member {}

const TOKENS: Member = Member {
    name: "tokens",
    directive: "server_tokens",
    bad: Some("http {\n    server_tokens on;\n}\n"),
    batch: BatchBehaviour::Honest,
};
const AUTOINDEX: Member = Member {
    name: "autoindex",
    directive: "autoindex",
    bad: Some("http {\n    autoindex on;\n}\n"),
    batch: BatchBehaviour::Honest,
};

fn outcome(rule: Member, group: [Member; 2]) -> Outcome {
    let refs: Vec<&dyn LintRule> = group.iter().map(|m| m as &dyn LintRule).collect();
    check_beside_siblings(&rule, &SiblingRuns::of(&refs))
}

#[test]
fn a_rule_that_reports_the_same_beside_its_siblings_passes() {
    assert!(matches!(
        outcome(TOKENS, [TOKENS, AUTOINDEX]),
        Outcome::Passed
    ));
}

#[test]
fn a_batched_check_that_fails_fails_the_check() {
    let failing = Member {
        batch: BatchBehaviour::Fails,
        ..TOKENS
    };
    match outcome(failing, [failing, AUTOINDEX]) {
        Outcome::Failed(why) => assert!(why.contains("trapped"), "{why}"),
        other => panic!("{other:?}"),
    }
}

/// The leak only shows over the sibling's example, which holds the
/// sibling's directive; over the rule's own example there is nothing
/// to leak.
#[test]
fn a_rule_that_sees_its_siblings_directives_fails_over_their_example() {
    let leaky = Member {
        batch: BatchBehaviour::SeesSiblings,
        ..TOKENS
    };
    match outcome(leaky, [leaky, AUTOINDEX]) {
        Outcome::Failed(why) => assert!(why.contains("over autoindex's bad example"), "{why}"),
        other => panic!("{other:?}"),
    }
}

/// A rule with no bad example of its own is still compared over its
/// siblings' — a leak shows there — but agreeing is a skip, not a pass:
/// the run has to say it could check nothing of the rule's own.
#[test]
fn a_rule_without_a_bad_example_is_compared_but_not_passed() {
    let mute = Member {
        bad: None,
        ..TOKENS
    };
    assert!(matches!(
        outcome(mute, [mute, AUTOINDEX]),
        Outcome::Skipped(_)
    ));

    let mute_and_leaky = Member {
        bad: None,
        batch: BatchBehaviour::SeesSiblings,
        ..TOKENS
    };
    match outcome(mute_and_leaky, [mute_and_leaky, AUTOINDEX]) {
        Outcome::Failed(why) => assert!(why.contains("over autoindex's bad example"), "{why}"),
        other => panic!("{other:?}"),
    }
}

/// A finding that differs only in its fix is a difference: the shared
/// config is what a fix is computed from.
#[test]
fn a_fix_that_differs_when_batched_fails_the_check() {
    let with_fix = Member {
        batch: BatchBehaviour::FixDiffers,
        ..TOKENS
    };
    match outcome(with_fix, [with_fix, AUTOINDEX]) {
        Outcome::Failed(why) => assert!(why.contains("new \"batched\""), "{why}"),
        other => panic!("{other:?}"),
    }
}

/// The category is part of what a finding says — the reporter prints
/// it — so one that changes when batched is a difference too.
#[test]
fn a_category_that_differs_when_batched_fails_the_check() {
    let recategorised = Member {
        batch: BatchBehaviour::CategoryDiffers,
        ..TOKENS
    };
    match outcome(recategorised, [recategorised, AUTOINDEX]) {
        Outcome::Failed(why) => assert!(why.contains("[batched]"), "{why}"),
        other => panic!("{other:?}"),
    }
}
