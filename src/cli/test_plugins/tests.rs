//! What each check of `test-plugins` passes, fails and skips on, against
//! a rule whose every part the checks read can be left out or made wrong.

use super::*;
use nginx_lint::parser::ast::Config;
use nginx_lint_common::linter::{Fix, Severity};

/// A rule that reports `server_tokens on` and offers to turn it off, with
/// each part of what the checks read made settable so a test can leave one
/// out or make it wrong.
struct Rule {
    bad: &'static str,
    good: &'static str,
    fixes: bool,
    /// Report on every `server_tokens`, not only `on`, which is how a
    /// rule that fires on its own good example behaves.
    report_everything: bool,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            bad: "http {\n    server_tokens on;\n}\n",
            good: "http {\n    server_tokens off;\n}\n",
            fixes: true,
            report_everything: false,
        }
    }
}

impl LintRule for Rule {
    fn name(&self) -> &'static str {
        "server-tokens-test"
    }
    fn category(&self) -> &'static str {
        "security"
    }
    fn description(&self) -> &'static str {
        "test rule"
    }
    fn bad_example(&self) -> Option<&str> {
        Some(self.bad)
    }
    fn good_example(&self) -> Option<&str> {
        Some(self.good)
    }

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();
        for directive in config.all_directives() {
            if directive.name != "server_tokens" {
                continue;
            }
            let on = directive.args.first().map(|arg| arg.as_str()) == Some("on");
            if !on && !self.report_everything {
                continue;
            }
            let mut error = LintError {
                rule: self.name().to_string(),
                category: self.category().to_string(),
                message: "server_tokens is on".to_string(),
                severity: Severity::Warning,
                line: Some(directive.span.start.line),
                column: Some(directive.span.start.column),
                fixes: Vec::new(),
            };
            if self.fixes {
                let start = directive.span.start.offset;
                error.fixes.push(Fix::replace_range(
                    start,
                    directive.span.end.offset,
                    "server_tokens off;",
                ));
            }
            errors.push(error);
        }
        errors
    }
}

fn outcome(check: &Check) -> &Outcome {
    &check.outcome
}

#[track_caller]
fn assert_passed(outcome: &Outcome) {
    match outcome {
        Outcome::Passed => {}
        Outcome::Failed(detail) => panic!("expected a pass, got a failure: {detail}"),
        Outcome::Skipped(why) => panic!("expected a pass, got a skip: {why}"),
    }
}

#[track_caller]
fn assert_failed(outcome: &Outcome) {
    match outcome {
        Outcome::Failed(_) => {}
        Outcome::Passed => panic!("expected a failure, got a pass"),
        Outcome::Skipped(why) => panic!("expected a failure, got a skip: {why}"),
    }
}

#[track_caller]
fn assert_skipped(outcome: &Outcome) {
    match outcome {
        Outcome::Skipped(_) => {}
        Outcome::Passed => panic!("expected a skip, got a pass"),
        Outcome::Failed(detail) => panic!("expected a skip, got a failure: {detail}"),
    }
}

#[test]
fn a_working_rule_passes_every_check() {
    let checks = test_plugin(&Rule::default(), None);

    assert_eq!(checks.len(), 3);
    for check in &checks {
        assert_passed(outcome(check));
    }
}

#[test]
fn a_bad_example_that_is_not_reported_fails() {
    let rule = Rule {
        bad: "http {\n    server_tokens off;\n}\n",
        ..Rule::default()
    };

    assert_failed(&check_bad_example(&rule));
}

#[test]
fn a_good_example_that_is_reported_fails() {
    let rule = Rule {
        report_everything: true,
        ..Rule::default()
    };

    assert_failed(&check_good_example(&rule));
}

/// A good example is the configuration the author is recommending, so it
/// has to parse — otherwise a dropped brace mangles the AST, the rule
/// finds nothing in the wreckage, and the check reads as success.
#[test]
fn a_malformed_good_example_fails() {
    let rule = Rule {
        good: "http {\n    server_tokens off;\n",
        ..Rule::default()
    };

    assert_failed(&check_good_example(&rule));
}

#[test]
fn a_malformed_expected_fixture_fails() {
    let dir = tempfile::tempdir().unwrap();
    let case = dir.path().join("001_basic");
    std::fs::create_dir_all(case.join("expected")).unwrap();
    std::fs::write(
        case.join("expected").join("nginx.conf"),
        "http {\n    server_tokens off;\n",
    )
    .unwrap();

    let checks = check_fixtures(&Rule::default(), dir.path());

    assert_eq!(checks.len(), 1);
    assert_failed(outcome(&checks[0]));
}

/// A syntax rule's bad example is malformed on purpose. The linter parses
/// it anyway and runs the rules, so this has to as well.
#[test]
fn a_malformed_bad_example_is_still_checked() {
    let rule = Rule {
        // Recoverable: the parser reports an error and still produces the
        // server_tokens directive the rule is looking for.
        bad: "http {\n    server_tokens on;\n    listen\n}\n",
        ..Rule::default()
    };

    assert_passed(&check_bad_example(&rule));
}

/// What makes a fix wrong is leaving the configuration more broken than it
/// found it, which is why the syntax errors are counted rather than
/// required to be zero.
#[test]
fn a_fix_that_introduces_syntax_errors_fails() {
    struct Breaking;
    impl LintRule for Breaking {
        fn name(&self) -> &'static str {
            "breaking"
        }
        fn category(&self) -> &'static str {
            "security"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn bad_example(&self) -> Option<&str> {
            Some("http {\n    server_tokens on;\n}\n")
        }
        fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
            config
                .all_directives()
                .filter(|directive| directive.name == "server_tokens")
                .map(|directive| LintError {
                    rule: self.name().to_string(),
                    category: self.category().to_string(),
                    message: "server_tokens is on".to_string(),
                    severity: Severity::Warning,
                    line: Some(directive.span.start.line),
                    column: Some(directive.span.start.column),
                    // Drops the terminator, so the configuration that
                    // comes out no longer parses cleanly.
                    fixes: vec![Fix::replace_range(
                        directive.span.start.offset,
                        directive.span.end.offset,
                        "server_tokens off",
                    )],
                })
                .collect()
        }
    }

    assert_failed(&check_fix(&Breaking));
}

/// A fix the applier drops is the mistake this command exists to catch —
/// offsets computed by hand in an SDK — so it must not read as success
/// just because another fix happened to resolve the finding.
#[test]
fn a_fix_the_applier_drops_fails() {
    struct Overlapping;
    impl LintRule for Overlapping {
        fn name(&self) -> &'static str {
            "overlapping"
        }
        fn category(&self) -> &'static str {
            "security"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn bad_example(&self) -> Option<&str> {
            Some("http {\n    server_tokens on;\n}\n")
        }
        fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
            config
                .all_directives()
                .filter(|directive| directive.name == "server_tokens")
                .map(|directive| LintError {
                    rule: self.name().to_string(),
                    category: self.category().to_string(),
                    message: "server_tokens is on".to_string(),
                    severity: Severity::Warning,
                    line: Some(directive.span.start.line),
                    column: Some(directive.span.start.column),
                    // Two edits over the same range: the applier takes
                    // one and drops the other without counting it.
                    fixes: vec![
                        Fix::replace_range(
                            directive.span.start.offset,
                            directive.span.end.offset,
                            "server_tokens off;",
                        ),
                        Fix::replace_range(
                            directive.span.start.offset,
                            directive.span.end.offset,
                            "server_tokens off;",
                        ),
                    ],
                })
                .collect()
        }
    }

    assert_failed(&check_fix(&Overlapping));
}

/// "the rule did not fire here" and "the rule has no autofix" send an
/// author looking in different places.
#[test]
fn a_rule_that_reported_nothing_says_so_rather_than_no_fixes() {
    let rule = Rule {
        bad: "http {\n    gzip on;\n}\n",
        ..Rule::default()
    };

    match check_fix(&rule) {
        Outcome::Skipped(why) => assert!(
            why.contains("reported nothing"),
            "the skip should say the rule did not fire, got: {why}"
        ),
        other => panic!(
            "expected a skip, got {:?}",
            matches!(other, Outcome::Passed)
        ),
    }
}

/// A plugin whose findings carry a different rule name than its spec looks
/// silent from here, and the message has to point at the name.
#[test]
fn a_rule_name_the_findings_disagree_with_is_named() {
    struct Mismatched;
    impl LintRule for Mismatched {
        fn name(&self) -> &'static str {
            "declared-name"
        }
        fn category(&self) -> &'static str {
            "security"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn bad_example(&self) -> Option<&str> {
            Some("http {\n    server_tokens on;\n}\n")
        }
        fn check(&self, _config: &Config, _path: &Path) -> Vec<LintError> {
            vec![LintError {
                rule: "reported-name".to_string(),
                category: "security".to_string(),
                message: "server_tokens is on".to_string(),
                severity: Severity::Warning,
                line: Some(2),
                column: Some(5),
                fixes: Vec::new(),
            }]
        }
    }

    match check_bad_example(&Mismatched) {
        Outcome::Failed(detail) => assert!(
            detail.contains("another rule name"),
            "the failure should point at the name, got: {detail}"
        ),
        _ => panic!("expected a failure"),
    }
}

/// A rule without an autofix is ordinary, so the fix check steps aside
/// rather than failing the plugin.
#[test]
fn a_rule_without_fixes_skips_the_fix_check() {
    let rule = Rule {
        fixes: false,
        ..Rule::default()
    };

    assert_skipped(&check_fix(&rule));
}

#[test]
fn a_fix_that_does_not_resolve_the_finding_fails() {
    // Replacing the directive with itself applies cleanly and changes
    // nothing, which is exactly the fix a rule should not ship.
    struct Unresolving;
    impl LintRule for Unresolving {
        fn name(&self) -> &'static str {
            "unresolving"
        }
        fn category(&self) -> &'static str {
            "security"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn bad_example(&self) -> Option<&str> {
            Some("http {\n    server_tokens on;\n}\n")
        }
        fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
            config
                .all_directives()
                .filter(|directive| directive.name == "server_tokens")
                .map(|directive| LintError {
                    rule: self.name().to_string(),
                    category: self.category().to_string(),
                    message: "still here".to_string(),
                    severity: Severity::Warning,
                    line: Some(directive.span.start.line),
                    column: Some(directive.span.start.column),
                    fixes: vec![Fix::replace_range(
                        directive.span.start.offset,
                        directive.span.end.offset,
                        "server_tokens on;",
                    )],
                })
                .collect()
        }
    }

    assert_failed(&check_fix(&Unresolving));
}

/// A plugin nothing could be checked on has been loaded and no more, so a
/// working plugin beside it must not carry the run to success.
#[test]
fn a_plugin_with_no_examples_is_reported_by_name() {
    struct NoExamples;
    impl LintRule for NoExamples {
        fn name(&self) -> &'static str {
            "no-examples"
        }
        fn category(&self) -> &'static str {
            "security"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn check(&self, _config: &Config, _path: &Path) -> Vec<LintError> {
            Vec::new()
        }
    }

    let checks = test_plugin(&NoExamples, None);

    assert!(
        checks
            .iter()
            .all(|check| matches!(check.outcome, Outcome::Skipped(_))),
        "every check should have stepped aside, leaving nothing verified"
    );
}

#[test]
fn a_missing_example_is_skipped_rather_than_failed() {
    struct NoExamples;
    impl LintRule for NoExamples {
        fn name(&self) -> &'static str {
            "no-examples"
        }
        fn category(&self) -> &'static str {
            "security"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn check(&self, _config: &Config, _path: &Path) -> Vec<LintError> {
            Vec::new()
        }
    }

    assert_skipped(&check_bad_example(&NoExamples));
    assert_skipped(&check_good_example(&NoExamples));
    assert_skipped(&check_fix(&NoExamples));
}

/// Findings from another rule must not decide this one's result, which is
/// what lets a directory of plugins be checked in one pass.
#[test]
fn another_rules_findings_are_ignored() {
    struct Noisy;
    impl LintRule for Noisy {
        fn name(&self) -> &'static str {
            "noisy"
        }
        fn category(&self) -> &'static str {
            "security"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn good_example(&self) -> Option<&str> {
            Some("http {\n    gzip on;\n}\n")
        }
        fn check(&self, _config: &Config, _path: &Path) -> Vec<LintError> {
            vec![LintError {
                rule: "somebody-else".to_string(),
                category: "security".to_string(),
                message: "not mine".to_string(),
                severity: Severity::Warning,
                line: Some(1),
                column: Some(1),
                fixes: Vec::new(),
            }]
        }
    }

    assert_passed(&check_good_example(&Noisy));
}

#[test]
fn fixtures_are_checked_in_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let case = dir.path().join("001_basic");
    std::fs::create_dir_all(case.join("error")).unwrap();
    std::fs::create_dir_all(case.join("expected")).unwrap();
    std::fs::write(
        case.join("error").join("nginx.conf"),
        "http {\n    server_tokens on;\n}\n",
    )
    .unwrap();
    std::fs::write(
        case.join("expected").join("nginx.conf"),
        "http {\n    server_tokens off;\n}\n",
    )
    .unwrap();

    let checks = check_fixtures(&Rule::default(), dir.path());

    assert_eq!(checks.len(), 3, "error/, expected/, and fixing error/");
    for check in &checks {
        assert_passed(outcome(check));
    }
}

/// A fixture's error/ is held to the same standard as the bad example:
/// the fixes it reports have to resolve it.
#[test]
fn a_fixture_error_that_fixing_does_not_resolve_fails() {
    let dir = tempfile::tempdir().unwrap();
    let case = dir.path().join("001_basic");
    std::fs::create_dir_all(case.join("error")).unwrap();
    std::fs::write(
        case.join("error").join("nginx.conf"),
        "http {\n    server_tokens on;\n}\n",
    )
    .unwrap();

    // Reports the finding, and offers a fix that changes nothing.
    struct Unresolving;
    impl LintRule for Unresolving {
        fn name(&self) -> &'static str {
            "unresolving"
        }
        fn category(&self) -> &'static str {
            "security"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
            config
                .all_directives()
                .filter(|directive| directive.name == "server_tokens")
                .map(|directive| LintError {
                    rule: self.name().to_string(),
                    category: self.category().to_string(),
                    message: "still here".to_string(),
                    severity: Severity::Warning,
                    line: Some(directive.span.start.line),
                    column: Some(directive.span.start.column),
                    fixes: vec![Fix::replace_range(
                        directive.span.start.offset,
                        directive.span.end.offset,
                        "server_tokens on;",
                    )],
                })
                .collect()
        }
    }

    let checks = check_fixtures(&Unresolving, dir.path());

    assert_eq!(checks.len(), 2, "error/ is checked, then its fixes are");
    assert_passed(outcome(&checks[0]));
    assert_failed(outcome(&checks[1]));
}

/// A case with only one of the two directories exercises only that one,
/// which is how the SDKs' own runners read the convention.
#[test]
fn a_fixture_case_may_declare_only_one_direction() {
    let dir = tempfile::tempdir().unwrap();
    let case = dir.path().join("001_error_only");
    std::fs::create_dir_all(case.join("error")).unwrap();
    std::fs::write(
        case.join("error").join("nginx.conf"),
        "http {\n    server_tokens on;\n}\n",
    )
    .unwrap();

    let checks = check_fixtures(&Rule::default(), dir.path());

    assert_eq!(
        checks.len(),
        2,
        "error/ and fixing it, but nothing about expected/"
    );
    for check in &checks {
        assert_passed(outcome(check));
    }
}

/// A case that names neither file exercises nothing, and a verification
/// command must not report that as success.
#[test]
fn a_fixture_case_with_neither_direction_fails() {
    let dir = tempfile::tempdir().unwrap();
    // `erro/` rather than `error/`: the shape a typo or a directory
    // nested one level too deep leaves behind.
    let case = dir.path().join("001_basic").join("erro");
    std::fs::create_dir_all(&case).unwrap();
    std::fs::write(case.join("nginx.conf"), "http {\n}\n").unwrap();

    let checks = check_fixtures(&Rule::default(), dir.path());

    assert_eq!(checks.len(), 1);
    assert_failed(outcome(&checks[0]));
}

#[test]
fn an_empty_fixtures_directory_fails() {
    let dir = tempfile::tempdir().unwrap();

    let checks = check_fixtures(&Rule::default(), dir.path());

    assert_eq!(checks.len(), 1);
    assert_failed(outcome(&checks[0]));
}
