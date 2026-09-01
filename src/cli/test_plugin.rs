//! `nginx-lint test-plugin` — run a plugin against its own documentation.
//!
//! Every SDK has a way to test a rule in its own language, and the tree has
//! had a Rust test binary that does the same for the builtin plugins. What
//! neither reaches is a plugin someone built somewhere else: an author with a
//! `.wasm` file had to hand-write shell around the CLI, checking exit codes
//! and filtering JSON, which is what this repository's own CI did three times
//! over, once per language.
//!
//! The checks here are the ones that binary makes, against the examples the
//! plugin carries in its own spec — the ones `nginx-lint why` renders — so
//! nothing has to be told where any file is.

use crate::Cli;
use colored::Colorize;
use nginx_lint::plugin::PluginLoader;
use nginx_lint_common::linter::{LintError, LintRule};
use nginx_lint_common::{apply_fixes_to_content, parse_string};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// What one check concluded.
enum Outcome {
    Passed,
    Failed(String),
    /// The plugin does not carry what the check needs — no example, or no
    /// fix to apply. Reported, and not a failure: a rule without an autofix
    /// is ordinary.
    Skipped(String),
}

/// One named check against one plugin.
struct Check {
    name: &'static str,
    outcome: Outcome,
}

pub fn run_test_plugin(fixtures: Option<PathBuf>, cli: &Cli) -> ExitCode {
    if cli.color {
        colored::control::set_override(true);
    } else if cli.no_color {
        colored::control::set_override(false);
    }

    let Some(ref dir) = cli.plugins else {
        eprintln!(
            "Error: test-plugin needs a plugin directory\n\n\
             Usage: nginx-lint test-plugin --plugins <DIR>"
        );
        return ExitCode::from(2);
    };
    if !dir.is_dir() {
        eprintln!(
            "Error loading plugins: Plugin directory not found: {}",
            dir.display()
        );
        return ExitCode::from(2);
    }

    let Ok(allow_wasi) = super::plugin_opts::allow_wasi(cli) else {
        return ExitCode::from(2);
    };
    let loader = match PluginLoader::new_with_cache(super::plugin_opts::cache_config(cli)) {
        Ok(loader) => loader.with_wasi(allow_wasi),
        Err(e) => {
            eprintln!("Error loading plugins: {}", e);
            return ExitCode::from(2);
        }
    };
    let plugins = match loader.load_plugins(dir) {
        Ok(plugins) => plugins,
        Err(e) => {
            eprintln!("Error loading plugins: {}", e);
            return ExitCode::from(2);
        }
    };

    if plugins.is_empty() {
        // The loader warns about each component it could not instantiate and
        // carries on, so an empty result means either an empty directory or a
        // directory where everything failed — a Go plugin run without
        // --allow-wasi-plugins is the common second case, and saying "no
        // plugins found" about a directory full of them is unhelpful.
        match wasm_files(dir) {
            0 => eprintln!("Error: no .wasm files in {}", dir.display()),
            found => eprintln!(
                "Error: none of the {} .wasm file(s) in {} could be loaded (see the warnings above)",
                found,
                dir.display()
            ),
        }
        return ExitCode::from(2);
    }

    // The loader warns about a component it cannot instantiate and carries
    // on, which is right for linting — one broken plugin should not stop the
    // run — and wrong here: a plugin that does not load is the first thing a
    // test command should refuse to pass.
    let found = wasm_files(dir);
    let mut failed = 0;
    if found > plugins.len() {
        eprintln!(
            "{} of the {} .wasm file(s) in {} did not load (see the warnings above)",
            found - plugins.len(),
            found,
            dir.display()
        );
        failed += found - plugins.len();
    }

    let mut passed = 0;
    for plugin in &plugins {
        let checks = test_plugin(plugin.as_ref(), fixtures.as_deref());
        report(plugin.name(), &checks);
        for check in &checks {
            match check.outcome {
                Outcome::Passed => passed += 1,
                Outcome::Failed(_) => failed += 1,
                Outcome::Skipped(_) => {}
            }
        }
    }

    println!();
    if failed > 0 {
        println!(
            "{} plugin(s), {} check(s) passed, {}",
            plugins.len(),
            passed,
            format!("{failed} failed").red().bold()
        );
        return ExitCode::from(1);
    }
    // Every check skipping is not success: a plugin with no examples has been
    // loaded and nothing else, and saying "passed" would hide that.
    if passed == 0 {
        println!(
            "{} plugin(s) loaded, but {}",
            plugins.len(),
            "nothing could be checked — do they declare bad and good examples?".yellow()
        );
        return ExitCode::from(1);
    }
    println!("{} plugin(s), {} check(s) passed", plugins.len(), passed);
    ExitCode::SUCCESS
}

fn wasm_files(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "wasm"))
                .count()
        })
        .unwrap_or(0)
}

fn test_plugin(plugin: &dyn LintRule, fixtures: Option<&Path>) -> Vec<Check> {
    let mut checks = vec![
        Check {
            name: "the bad example is reported",
            outcome: check_bad_example(plugin),
        },
        Check {
            name: "the good example is clean",
            outcome: check_good_example(plugin),
        },
        Check {
            name: "fixing the bad example resolves it",
            outcome: check_fix(plugin),
        },
    ];
    if let Some(fixtures) = fixtures {
        checks.extend(check_fixtures(plugin, fixtures));
    }
    checks
}

/// Findings this plugin reported, ignoring any other rule's.
///
/// Filtering by name rather than running the plugin alone is what keeps this
/// independent of whether the rule is enabled anywhere: a rule disabled by
/// default cannot be reached through `--rule-only` at all.
fn findings(plugin: &dyn LintRule, source: &str, path: &str) -> Result<Vec<LintError>, String> {
    let config = parse_string(source).map_err(|e| format!("the example does not parse: {e}"))?;
    Ok(plugin
        .check(&config, Path::new(path))
        .into_iter()
        .filter(|error| error.rule == plugin.name())
        .collect())
}

fn check_bad_example(plugin: &dyn LintRule) -> Outcome {
    let Some(bad) = plugin.bad_example().filter(|example| !example.is_empty()) else {
        return Outcome::Skipped("the plugin declares no bad example".to_string());
    };
    match findings(plugin, bad, "bad.conf") {
        Err(e) => Outcome::Failed(e),
        Ok(found) if found.is_empty() => {
            Outcome::Failed("the bad example was reported clean".to_string())
        }
        Ok(_) => Outcome::Passed,
    }
}

fn check_good_example(plugin: &dyn LintRule) -> Outcome {
    let Some(good) = plugin.good_example().filter(|example| !example.is_empty()) else {
        return Outcome::Skipped("the plugin declares no good example".to_string());
    };
    match findings(plugin, good, "good.conf") {
        Err(e) => Outcome::Failed(e),
        Ok(found) if found.is_empty() => Outcome::Passed,
        Ok(found) => Outcome::Failed(describe(&found)),
    }
}

/// Apply what the plugin reports on its bad example, and require the rule to
/// stop firing.
///
/// The result is not compared with the good example: a good example may show
/// more than the fix produces, which is why the Rust test binary treats an
/// exact match as informational too.
fn check_fix(plugin: &dyn LintRule) -> Outcome {
    let Some(bad) = plugin.bad_example().filter(|example| !example.is_empty()) else {
        return Outcome::Skipped("the plugin declares no bad example".to_string());
    };
    let found = match findings(plugin, bad, "bad.conf") {
        Ok(found) => found,
        Err(e) => return Outcome::Failed(e),
    };
    let fixes: Vec<_> = found.iter().flat_map(|error| error.fixes.iter()).collect();
    if fixes.is_empty() {
        return Outcome::Skipped("the plugin reports no fixes".to_string());
    }

    let (fixed, applied) = apply_fixes_to_content(bad, &fixes);
    if applied == 0 {
        return Outcome::Failed(format!(
            "none of the {} fix(es) reported could be applied",
            fixes.len()
        ));
    }
    match findings(plugin, &fixed, "bad.conf") {
        Err(e) => Outcome::Failed(format!("the fixed example does not parse: {e}")),
        Ok(remaining) if remaining.is_empty() => Outcome::Passed,
        Ok(remaining) => Outcome::Failed(format!(
            "after fixing, the rule still reports:\n{}\nthe fixed example was:\n{}",
            describe(&remaining),
            indent(&fixed)
        )),
    }
}

/// Run the `<case>/error/nginx.conf` and `<case>/expected/nginx.conf`
/// convention the SDKs document, with the semantics their own test runners
/// use: error/ has to be reported, expected/ has to be clean.
fn check_fixtures(plugin: &dyn LintRule, fixtures: &Path) -> Vec<Check> {
    let entries = match std::fs::read_dir(fixtures) {
        Ok(entries) => entries,
        Err(e) => {
            return vec![Check {
                name: "the fixtures directory is readable",
                outcome: Outcome::Failed(format!("{}: {e}", fixtures.display())),
            }];
        }
    };

    let mut cases: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    cases.sort();

    if cases.is_empty() {
        return vec![Check {
            name: "the fixtures directory has cases",
            outcome: Outcome::Failed(format!("no case directories in {}", fixtures.display())),
        }];
    }

    let mut checks = Vec::new();
    for case in cases {
        let name = case
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if let Some(outcome) = fixture_case(plugin, &case.join("error").join("nginx.conf"), true) {
            checks.push(Check {
                name: leak(format!("fixture {name}/error is reported")),
                outcome,
            });
        }
        if let Some(outcome) =
            fixture_case(plugin, &case.join("expected").join("nginx.conf"), false)
        {
            checks.push(Check {
                name: leak(format!("fixture {name}/expected is clean")),
                outcome,
            });
        }
    }
    checks
}

/// `None` when the fixture is absent, which is how a case declares that it
/// only exercises one of the two directions.
fn fixture_case(plugin: &dyn LintRule, path: &Path, expect_findings: bool) -> Option<Outcome> {
    if !path.exists() {
        return None;
    }
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(e) => return Some(Outcome::Failed(format!("{}: {e}", path.display()))),
    };
    Some(match findings(plugin, &source, &path.to_string_lossy()) {
        Err(e) => Outcome::Failed(e),
        Ok(found) if found.is_empty() != expect_findings => Outcome::Passed,
        Ok(_) if expect_findings => Outcome::Failed("reported clean".to_string()),
        Ok(found) => Outcome::Failed(describe(&found)),
    })
}

fn report(name: &str, checks: &[Check]) {
    println!("{}", name.bold());
    for check in checks {
        match &check.outcome {
            Outcome::Passed => println!("  {} {}", "ok".green(), check.name),
            Outcome::Skipped(why) => {
                println!(
                    "  {} {} — {}",
                    "--".dimmed(),
                    check.name.dimmed(),
                    why.dimmed()
                )
            }
            Outcome::Failed(detail) => {
                println!("  {} {}", "FAILED".red().bold(), check.name);
                for line in detail.lines() {
                    println!("      {}", line);
                }
            }
        }
    }
}

fn describe(errors: &[LintError]) -> String {
    errors
        .iter()
        .map(|error| {
            format!(
                "{}:{}: {}",
                error.line.unwrap_or(0),
                error.column.unwrap_or(0),
                error.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn indent(content: &str) -> String {
    content
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Check names are `&'static str` so the common ones can be literals; a
/// fixture's name is only known at run time, and there are as many of these
/// as there are fixtures, which is bounded by the directory.
fn leak(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn an_example_that_does_not_parse_fails() {
        let rule = Rule {
            bad: "http {\n",
            ..Rule::default()
        };

        assert_failed(&check_bad_example(&rule));
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

        assert_eq!(checks.len(), 2);
        for check in &checks {
            assert_passed(outcome(check));
        }
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

        assert_eq!(checks.len(), 1);
        assert_passed(outcome(&checks[0]));
    }

    #[test]
    fn an_empty_fixtures_directory_fails() {
        let dir = tempfile::tempdir().unwrap();

        let checks = check_fixtures(&Rule::default(), dir.path());

        assert_eq!(checks.len(), 1);
        assert_failed(outcome(&checks[0]));
    }
}
