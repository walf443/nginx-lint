//! `nginx-lint test-plugins` — run a plugin against its own documentation.
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
use nginx_lint_common::linter::apply_fixes_to_content_detailed;
use nginx_lint_common::linter::{LintError, LintRule};
use nginx_lint_common::parse_string_with_errors;
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

pub fn run_test_plugins(fixtures: Option<PathBuf>, cli: &Cli) -> ExitCode {
    if cli.color {
        colored::control::set_override(true);
    } else if cli.no_color {
        colored::control::set_override(false);
    }

    let Some(ref dir) = cli.plugins else {
        eprintln!(
            "Error: test-plugins needs a plugin directory\n\n\
             Usage: nginx-lint test-plugins --plugins <DIR>"
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
    // test command should refuse to pass. It exits 2 rather than counting
    // towards the failed checks: a component that cannot be instantiated is a
    // build problem, not a rule that behaves wrongly, and reporting it as a
    // failed check would print "1 failed" with nothing having been checked.
    let found = wasm_files(dir);
    if found > plugins.len() {
        eprintln!(
            "Error: {} of the {} .wasm file(s) in {} did not load (see the warnings above)",
            found - plugins.len(),
            found,
            dir.display()
        );
        return ExitCode::from(2);
    }

    // A fixture case is written for one rule: `error/nginx.conf` is a
    // configuration that rule reports. Running the same cases against every
    // plugin in the directory would fail all the others, so rather than
    // guessing which plugin a case belongs to, say what is wrong.
    if fixtures.is_some() && plugins.len() > 1 {
        eprintln!(
            "Error: --fixtures describes one plugin's cases, but {} plugins loaded from {}\n\n\
             Point --plugins at the one plugin whose fixtures these are.",
            plugins.len(),
            dir.display()
        );
        return ExitCode::from(2);
    }

    let mut failed = 0;
    let mut passed = 0;
    let mut unchecked = Vec::new();
    for plugin in &plugins {
        let checks = test_plugin(plugin.as_ref(), fixtures.as_deref());
        report(plugin.name(), &checks);

        let mut checked = 0;
        for check in &checks {
            match check.outcome {
                Outcome::Passed => {
                    passed += 1;
                    checked += 1;
                }
                Outcome::Failed(_) => {
                    failed += 1;
                    checked += 1;
                }
                Outcome::Skipped(_) => {}
            }
        }
        // Per plugin rather than over the run: a plugin that declares no
        // examples has been loaded and nothing else, and a working plugin
        // beside it must not report success on its behalf.
        if checked == 0 {
            unchecked.push(plugin.name());
        }
    }

    println!();
    // Both are reported before returning: they are not exclusive, and holding
    // the second back until the first is fixed makes a condition that was true
    // all along read as a new regression on the next run.
    if !unchecked.is_empty() {
        println!(
            "{} for {} — {}",
            "nothing could be checked".yellow(),
            unchecked.join(", "),
            "do they declare bad and good examples?".yellow()
        );
    }
    if failed > 0 {
        println!(
            "{} plugin(s), {} check(s) passed, {}",
            plugins.len(),
            passed,
            format!("{failed} failed").red().bold()
        );
    } else {
        println!("{} plugin(s), {} check(s) passed", plugins.len(), passed);
    }

    if failed > 0 || !unchecked.is_empty() {
        return ExitCode::from(1);
    }
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

/// What running the plugin over one configuration produced.
struct Checked {
    /// Findings the plugin reported under its own rule name.
    found: Vec<LintError>,
    /// How many it reported under some other name. A plugin whose spec and
    /// findings disagree about the rule name looks silent from here, and
    /// saying so is the difference between a useful message and a puzzle.
    under_other_names: usize,
    syntax_errors: usize,
}

/// Run the plugin over `source`, keeping the findings that are its own.
///
/// Filtering by name rather than running the plugin alone is what keeps this
/// independent of whether the rule is enabled anywhere: a rule disabled by
/// default cannot be reached through `--rule-only` at all.
fn findings(plugin: &dyn LintRule, source: &str, path: &str) -> Checked {
    // The tolerant parser, because that is the one the linter uses: it builds
    // the AST anyway and lets the rules run. Refusing a configuration that
    // does not parse cleanly would reject every syntax rule's bad example,
    // which is exactly the kind of rule this command should be able to check.
    let (config, syntax_errors) = parse_string_with_errors(source);
    let reported = plugin.check(&config, Path::new(path));
    let total = reported.len();
    let found: Vec<LintError> = reported
        .into_iter()
        .filter(|error| error.rule == plugin.name())
        .collect();
    Checked {
        under_other_names: total - found.len(),
        found,
        syntax_errors: syntax_errors.len(),
    }
}

fn check_bad_example(plugin: &dyn LintRule) -> Outcome {
    let Some(bad) = plugin.bad_example().filter(|example| !example.is_empty()) else {
        return Outcome::Skipped("the plugin declares no bad example".to_string());
    };
    let checked = findings(plugin, bad, "bad.conf");
    if !checked.found.is_empty() {
        return Outcome::Passed;
    }
    Outcome::Failed(match checked.under_other_names {
        0 => "the bad example was reported clean".to_string(),
        other => format!(
            "the bad example was reported clean under {:?}, though the plugin \
             reported {other} finding(s) under another rule name — the spec and \
             the findings have to agree on it",
            plugin.name()
        ),
    })
}

fn check_good_example(plugin: &dyn LintRule) -> Outcome {
    let Some(good) = plugin.good_example().filter(|example| !example.is_empty()) else {
        return Outcome::Skipped("the plugin declares no good example".to_string());
    };
    clean(findings(plugin, good, "good.conf"))
}

/// The clean direction of a check: no findings, and no syntax errors either.
///
/// Tolerating a malformed configuration is deliberate on the bad side — a
/// syntax rule's example is malformed on purpose — but a good example is the
/// configuration the author is recommending. Without this, a dropped brace
/// mangles the AST, the rule finds nothing in the wreckage, and the check
/// reads as success.
fn clean(checked: Checked) -> Outcome {
    if checked.syntax_errors > 0 {
        return Outcome::Failed(format!(
            "it does not parse: {} syntax error(s)",
            checked.syntax_errors
        ));
    }
    if checked.found.is_empty() {
        return Outcome::Passed;
    }
    Outcome::Failed(describe(&checked.found))
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
    resolve_by_fixing(plugin, bad, "bad.conf")
}

/// Apply every fix the plugin reports on `source` and require the rule to
/// stop firing.
///
/// The result is not compared with anything: a good example or an `expected/`
/// fixture may show more than the fix produces, which is why the Rust test
/// binary this replaces treated an exact match as informational too.
fn resolve_by_fixing(plugin: &dyn LintRule, source: &str, path: &str) -> Outcome {
    let checked = findings(plugin, source, path);
    let fixes: Vec<_> = checked
        .found
        .iter()
        .flat_map(|error| error.fixes.iter())
        .collect();
    if fixes.is_empty() {
        // Told apart, because "the rule did not fire here" and "the rule has
        // no autofix" send an author looking in different places.
        return Outcome::Skipped(if checked.found.is_empty() {
            "the plugin reported nothing here, so there is nothing to fix".to_string()
        } else {
            "the plugin reports no fixes".to_string()
        });
    }

    // Every reported fix has to land. The applier drops a fix whose offsets
    // are out of range or split a character, and drops one that overlaps a
    // fix already applied without counting it anywhere — and offsets computed
    // by hand in an SDK are the mistake this command exists to catch, so a
    // fix that quietly does not apply must not read as success.
    let result = apply_fixes_to_content_detailed(source, &fixes);
    if result.applied != fixes.len() {
        return Outcome::Failed(format!(
            "{} of the {} fix(es) reported were applied; {} had offsets the applier \
             rejected, and the rest it dropped for conflicting with a fix it had \
             already applied",
            result.applied,
            fixes.len(),
            result.skipped_invalid
        ));
    }
    let fixed = result.content;

    let after = findings(plugin, &fixed, path);
    // Counted rather than required to be zero: a syntax rule's own example is
    // malformed to begin with, so what makes a fix wrong is introducing
    // errors that were not there.
    if after.syntax_errors > checked.syntax_errors {
        return Outcome::Failed(format!(
            "fixing introduced {} syntax error(s); the fixed configuration was:\n{}",
            after.syntax_errors - checked.syntax_errors,
            indent(&fixed)
        ));
    }
    if after.found.is_empty() {
        return Outcome::Passed;
    }
    Outcome::Failed(format!(
        "after fixing, the rule still reports:\n{}\nthe fixed configuration was:\n{}",
        describe(&after.found),
        indent(&fixed)
    ))
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
        let before = checks.len();

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
        if let Some(outcome) = fixture_fix(plugin, &case.join("error").join("nginx.conf")) {
            checks.push(Check {
                name: leak(format!("fixture {name}/error is resolved by fixing it")),
                outcome,
            });
        }

        // Neither file means the case exercises nothing, and passing silently
        // is the worst direction for a command whose job is to verify: a
        // directory nested one level too deep, or a misspelled `expected/`,
        // would otherwise read as success.
        if checks.len() == before {
            checks.push(Check {
                name: leak(format!("fixture {name} declares a configuration")),
                outcome: Outcome::Failed(format!(
                    "{} has neither error/nginx.conf nor expected/nginx.conf",
                    case.display()
                )),
            });
        }
    }
    checks
}

/// Apply what the plugin reports on a fixture's `error/nginx.conf` and
/// require the rule to stop firing, the same way [`check_fix`] does for the
/// bad example.
///
/// `None` when there is no `error/nginx.conf`, so a case that only declares
/// the clean direction is not asked about fixes.
fn fixture_fix(plugin: &dyn LintRule, path: &Path) -> Option<Outcome> {
    if !path.exists() {
        return None;
    }
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(e) => return Some(Outcome::Failed(format!("{}: {e}", path.display()))),
    };
    Some(resolve_by_fixing(plugin, &source, &path.to_string_lossy()))
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
    let checked = findings(plugin, &source, &path.to_string_lossy());
    if !expect_findings {
        return Some(clean(checked));
    }
    Some(if checked.found.is_empty() {
        Outcome::Failed("reported clean".to_string())
    } else {
        Outcome::Passed
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
}
