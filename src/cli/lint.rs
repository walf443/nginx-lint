use super::Cli;
use clap::CommandFactory;
use colored::control;
use nginx_lint::{
    ColorMode, IncludedFile, LintConfig, LintError, Linter, Reporter, RuleProfile, Severity,
    apply_fixes, apply_fixes_to_content_detailed, collect_included_files,
    collect_included_files_with_context, parse_config, parse_string_with_errors,
    pre_parse_checks_from_content, syntax_errors_to_lint_errors,
};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Warn about fixes that were skipped due to invalid offsets (out of range
/// or not on UTF-8 char boundaries), which indicates a buggy or misbehaving
/// rule/plugin.
fn warn_skipped_fixes(skipped: usize, path: &Path) {
    if skipped > 0 {
        eprintln!(
            "Warning: skipped {} fix(es) with invalid offsets in {}",
            skipped,
            path.display()
        );
    }
}

/// Extend `errors` with `additional`, dropping exact duplicates.
///
/// Pre-parse checks and the registered syntax rules (missing-semicolon,
/// unmatched-braces, unclosed-quote) detect the same problems; keeping both
/// copies would report each problem twice and, under `--fix`, apply the same
/// fix twice (e.g. inserting `;;` for one missing semicolon).
fn extend_errors_dedup(errors: &mut Vec<LintError>, additional: Vec<LintError>) {
    for err in additional {
        let is_duplicate = errors.iter().any(|existing| {
            existing.rule == err.rule
                && existing.line == err.line
                && existing.column == err.column
                && existing.message == err.message
        });
        if !is_duplicate {
            errors.push(err);
        }
    }
}

/// Resolve a path value from the config file: a relative path is resolved
/// against the directory containing the config file.
fn resolve_against_config_dir(path: PathBuf, config_dir: Option<&Path>) -> PathBuf {
    if path.is_relative() {
        config_dir.unwrap_or(Path::new(".")).join(path)
    } else {
        path
    }
}

/// Result of linting a single file
enum FileResult {
    LintErrors {
        path: PathBuf,
        errors: Vec<LintError>,
        ignored_count: usize,
        profiles: Option<Vec<RuleProfile>>,
    },
}

/// Check whether a rule name corresponds to a known builtin rule, regardless
/// of whether it is currently registered on the linter. Used by `--rule-only`
/// validation to distinguish "disabled in config" from "no such rule".
fn rule_exists_in_catalog(name: &str) -> bool {
    #[cfg(any(feature = "wasm-builtin-plugins", feature = "native-builtin-plugins"))]
    {
        nginx_lint::docs::get_rule_doc_with_plugins(name).is_some()
    }
    #[cfg(not(any(feature = "wasm-builtin-plugins", feature = "native-builtin-plugins")))]
    {
        nginx_lint::docs::all_rule_names().contains(&name)
    }
}

/// Display profiling results
fn display_profile(profiles: &[RuleProfile]) {
    use colored::Colorize;
    use std::time::Duration;

    // Aggregate profiles by rule name (sum durations across files)
    let mut aggregated: HashMap<String, (Duration, String, usize)> = HashMap::new();
    for p in profiles {
        let entry =
            aggregated
                .entry(p.name.clone())
                .or_insert((Duration::ZERO, p.category.clone(), 0));
        entry.0 += p.duration;
        entry.2 += p.error_count;
    }

    // Convert to vec and sort by duration (descending)
    let mut sorted: Vec<_> = aggregated.into_iter().collect();
    sorted.sort_by_key(|entry| std::cmp::Reverse(entry.1.0));

    // Calculate total time
    let total_time: Duration = sorted.iter().map(|(_, (d, _, _))| *d).sum();

    eprintln!();
    eprintln!("{}", "Profile Results".bold().underline());
    eprintln!();

    // Header
    eprintln!(
        "{:>10}  {:>6}  {:>6}  {:<30}  {}",
        "Time".bold(),
        "%".bold(),
        "Errors".bold(),
        "Rule".bold(),
        "Category".bold()
    );
    eprintln!("{}", "-".repeat(70));

    for (name, (duration, category, error_count)) in &sorted {
        let percentage = if total_time.as_nanos() > 0 {
            (duration.as_nanos() as f64 / total_time.as_nanos() as f64) * 100.0
        } else {
            0.0
        };

        let time_str = format_duration(*duration);
        let pct_str = format!("{:.1}%", percentage);

        // Highlight slow rules (>10% of total time)
        let time_display = if percentage > 10.0 {
            time_str.red().to_string()
        } else if percentage > 5.0 {
            time_str.yellow().to_string()
        } else {
            time_str
        };

        eprintln!(
            "{:>10}  {:>6}  {:>6}  {:<30}  {}",
            time_display,
            pct_str,
            error_count,
            name,
            category.dimmed()
        );
    }

    eprintln!("{}", "-".repeat(70));
    eprintln!(
        "{:>10}  {:>6}  {:>6}  {}",
        format_duration(total_time).bold(),
        "100%".bold(),
        sorted.iter().map(|(_, (_, _, c))| c).sum::<usize>(),
        "Total".bold()
    );
    eprintln!();
}

/// Format a duration for display
fn format_duration(d: std::time::Duration) -> String {
    let micros = d.as_micros();
    if micros < 1000 {
        format!("{}µs", micros)
    } else if micros < 1_000_000 {
        format!("{:.2}ms", micros as f64 / 1000.0)
    } else {
        format!("{:.2}s", d.as_secs_f64())
    }
}

/// Run linter on a parsed config and return a FileResult
fn run_lint_on_config(
    config: &nginx_lint::parser::ast::Config,
    path: &Path,
    content: &str,
    linter: &Linter,
    profile: bool,
) -> FileResult {
    if profile {
        let (errors, ignored_count, profiles) =
            linter.lint_with_content_and_profile(config, path, content);
        FileResult::LintErrors {
            path: path.to_path_buf(),
            errors,
            ignored_count,
            profiles: Some(profiles),
        }
    } else {
        let (errors, ignored_count) = linter.lint_with_content(config, path, content);
        FileResult::LintErrors {
            path: path.to_path_buf(),
            errors,
            ignored_count,
            profiles: None,
        }
    }
}

/// Lint a single included file and return the result
fn lint_file(
    included: &IncludedFile,
    linter: &Linter,
    lint_config: Option<&LintConfig>,
    profile: bool,
) -> FileResult {
    let path = &included.path;

    let content = std::fs::read_to_string(path).unwrap_or_default();

    // Run pre-parse checks on already-read content to avoid reading the file twice
    let pre_parse_errors = pre_parse_checks_from_content(&content, lint_config);

    // Always parse with error recovery — rowan produces a usable AST even with errors
    let (config, syntax_errors) = if let Some(ref config) = included.config {
        (config.clone(), Vec::new())
    } else {
        let (config, errors) = parse_string_with_errors(&content);
        (config, errors)
    };

    let mut result = run_lint_on_config(&config, path, &content, linter, profile);

    // Merge pre-parse errors and syntax errors into lint errors, dropping
    // exact duplicates (the linter's syntax rules and pre_parse_checks
    // overlap; rowan syntax-error may also repeat).
    let FileResult::LintErrors { ref mut errors, .. } = result;
    extend_errors_dedup(errors, pre_parse_errors);
    if !syntax_errors.is_empty() {
        extend_errors_dedup(
            errors,
            syntax_errors_to_lint_errors(&syntax_errors, &content),
        );
    }

    result
}

/// Lint a file, apply autofixes, and re-lint the fixed content.
///
/// Reporting the re-lint result (instead of the pre-fix errors) means the
/// reported errors and the exit code always describe what actually remains
/// in the written file: positions are computed against the rewritten
/// content, and problems left behind by fixes that failed to apply or were
/// skipped stay visible.
fn fix_file(
    inc: &IncludedFile,
    linter: &Linter,
    lint_config: Option<&LintConfig>,
    profile: bool,
) -> FileResult {
    let FileResult::LintErrors {
        path,
        errors,
        ignored_count,
        profiles,
    } = lint_file(inc, linter, lint_config, profile);

    if errors.iter().all(|e| e.fixes.is_empty()) {
        return FileResult::LintErrors {
            path,
            errors,
            ignored_count,
            profiles,
        };
    }

    match apply_fixes(&path, &errors) {
        Ok(result) => {
            warn_skipped_fixes(result.skipped_invalid, &path);
            if result.applied == 0 {
                // Nothing was written: the original lint results still hold.
                return FileResult::LintErrors {
                    path,
                    errors,
                    ignored_count,
                    profiles,
                };
            }
            eprintln!("Applied {} fix(es) to {}", result.applied, path.display());
            let FileResult::LintErrors {
                errors: remaining,
                ignored_count: remaining_ignored,
                ..
            } = lint_content(
                &result.content,
                &path,
                linter,
                lint_config,
                false,
                inc.include_context.clone(),
            );
            FileResult::LintErrors {
                path,
                errors: remaining,
                ignored_count: remaining_ignored,
                profiles,
            }
        }
        Err(e) => {
            eprintln!("Error applying fixes to {}: {}", path.display(), e);
            // Nothing was written: every error still stands.
            FileResult::LintErrors {
                path,
                errors,
                ignored_count,
                profiles,
            }
        }
    }
}

/// Apply autofixes to stdin content, print the result to stdout, and
/// re-lint the fixed content for the stderr report and the exit code.
fn fix_stdin(
    result: FileResult,
    content: &str,
    linter: &Linter,
    lint_config: Option<&LintConfig>,
    initial_context: Vec<String>,
) -> FileResult {
    let FileResult::LintErrors {
        path,
        errors,
        ignored_count,
        profiles,
    } = result;

    let fixes: Vec<_> = errors.iter().flat_map(|e| e.fixes.iter()).collect();
    let apply_result = apply_fixes_to_content_detailed(content, &fixes);
    warn_skipped_fixes(apply_result.skipped_invalid, &path);

    if apply_result.applied == 0 {
        // Nothing was fixed: echo the input and keep the original results.
        print!("{}", content);
        return FileResult::LintErrors {
            path,
            errors,
            ignored_count,
            profiles,
        };
    }

    print!("{}", apply_result.content);
    let FileResult::LintErrors {
        errors: remaining,
        ignored_count: remaining_ignored,
        ..
    } = lint_content(
        &apply_result.content,
        &path,
        linter,
        lint_config,
        false,
        initial_context,
    );
    FileResult::LintErrors {
        path,
        errors: remaining,
        ignored_count: remaining_ignored,
        profiles,
    }
}

/// Process lint results: report errors and determine the exit code.
///
/// Under `--fix` the results have already been fixed and re-linted by
/// `fix_file`/`fix_stdin`, so they are reported like any other lint result;
/// in stdin mode the report goes to stderr because stdout carries the fixed
/// content.
fn process_results(
    results: Vec<FileResult>,
    fix: bool,
    no_fail_on_warnings: bool,
    profile: bool,
    reporter: &Reporter,
    stdin_mode: bool,
) -> ExitCode {
    let mut all_errors = Vec::new();
    let mut all_profiles: Vec<RuleProfile> = Vec::new();
    // Once the output consumer closes the stream (e.g. piping into `head`),
    // stop reporting but still compute the exit code.
    let mut output_closed = false;

    for result in results {
        let FileResult::LintErrors {
            path,
            errors,
            ignored_count,
            profiles,
        } = result;

        let report_result = if output_closed {
            Ok(())
        } else if fix && stdin_mode {
            // stdout carries the fixed content, so report to stderr
            if !errors.is_empty() || ignored_count > 0 {
                reporter.report_to_stderr(&errors, &path, ignored_count)
            } else {
                Ok(())
            }
        } else {
            reporter.report(&errors, &path, ignored_count)
        };
        if let Err(e) = report_result {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                output_closed = true;
            } else {
                eprintln!("Error writing report: {}", e);
                return ExitCode::from(2);
            }
        }

        all_errors.extend(errors);
        if let Some(p) = profiles {
            all_profiles.extend(p);
        }
    }

    // Display profile results if requested
    if profile && !all_profiles.is_empty() {
        display_profile(&all_profiles);
    }

    let has_issues = if no_fail_on_warnings {
        all_errors.iter().any(|e| e.severity == Severity::Error)
    } else {
        !all_errors.is_empty()
    };

    if has_issues {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Lint in-memory content (stdin mode) and return the result
fn lint_content(
    content: &str,
    path: &Path,
    linter: &Linter,
    lint_config: Option<&LintConfig>,
    profile: bool,
    initial_context: Vec<String>,
) -> FileResult {
    // Run pre-parse checks on the content
    let pre_parse_errors = pre_parse_checks_from_content(content, lint_config);

    // Parse the content (always produces AST, even with syntax errors)
    let (mut parse_result, syntax_errors) = parse_string_with_errors(content);

    // Set context if specified
    if !initial_context.is_empty() {
        parse_result.include_context = initial_context;
    }

    let mut result = run_lint_on_config(&parse_result, path, content, linter, profile);

    // Merge pre-parse errors and syntax errors into lint errors, dropping
    // exact duplicates (see lint_file)
    let FileResult::LintErrors { ref mut errors, .. } = result;
    extend_errors_dedup(errors, pre_parse_errors);
    if !syntax_errors.is_empty() {
        extend_errors_dedup(
            errors,
            syntax_errors_to_lint_errors(&syntax_errors, content),
        );
    }

    result
}

pub fn run_lint(cli: Cli) -> ExitCode {
    // 1. Detect stdin mode and read content if applicable
    let stdin_mode = cli.files.len() == 1 && cli.files[0].as_os_str() == "-";
    let stdin_content = if stdin_mode {
        let mut content = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut content) {
            eprintln!("Error reading from stdin: {}", e);
            return ExitCode::from(2);
        }
        Some(content)
    } else {
        None
    };

    // 2. Validate files and resolve paths (file mode only)
    let file_paths = if stdin_content.is_none() {
        if cli.files.is_empty() {
            let _ = Cli::command().print_help();
            eprintln!();
            return ExitCode::from(2);
        }

        let mut paths: Vec<PathBuf> = Vec::new();
        for file in &cli.files {
            if file.is_dir() {
                let nginx_conf = file.join("nginx.conf");
                if !nginx_conf.exists() {
                    eprintln!(
                        "Error: nginx.conf not found in directory {}",
                        file.display()
                    );
                    return ExitCode::from(2);
                }
                paths.push(nginx_conf);
            } else {
                paths.push(file.clone());
            }
        }
        paths
    } else {
        Vec::new()
    };

    // 3. Load configuration
    let (lint_config, config_dir) = if let Some(config_path) = &cli.config {
        match LintConfig::from_file(config_path) {
            Ok(cfg) => {
                if cli.verbose {
                    eprintln!("Using config: {}", config_path.display());
                }
                let dir = config_path.parent().map(|p| p.to_path_buf());
                (Some(cfg), dir)
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                return ExitCode::from(2);
            }
        }
    } else {
        let search_dir = if stdin_content.is_some() {
            Path::new(".")
        } else {
            file_paths
                .first()
                .and_then(|p| p.parent())
                .unwrap_or(Path::new("."))
        };
        match LintConfig::find_and_load(search_dir) {
            Some((cfg, config_path)) => {
                if cli.verbose {
                    eprintln!("Found .nginx-lint.toml: {}", config_path.display());
                }
                let dir = config_path.parent().map(|p| p.to_path_buf());
                (Some(cfg), dir)
            }
            None => (None, None),
        }
    };

    // 4. Configure color output (CLI flags take precedence over config)
    if cli.color {
        control::set_override(true);
    } else if cli.no_color {
        control::set_override(false);
    } else if let Some(ref config) = lint_config {
        match config.color_mode() {
            ColorMode::Always => control::set_override(true),
            ColorMode::Never => control::set_override(false),
            ColorMode::Auto => {}
        }
    }

    // 5. Create reporter with color configuration
    let color_config = lint_config
        .as_ref()
        .map(|c| c.color.clone())
        .unwrap_or_default();
    let reporter = Reporter::with_colors(cli.format.into(), color_config);

    // 6. Parse context option if specified (comma-separated list of block names)
    let initial_context: Vec<String> = cli
        .context
        .as_ref()
        .map(|s| s.split(',').map(|c| c.trim().to_string()).collect())
        .unwrap_or_default();

    if cli.verbose && !initial_context.is_empty() {
        eprintln!("Using context: {}", initial_context.join(" > "));
    }

    // 7. Resolve include prefix (CLI --prefix takes precedence over config)
    // Priority: CLI --prefix > config include.prefix > config file directory (default)
    let include_prefix: Option<PathBuf> = cli.prefix.clone().or_else(|| {
        if let Some(ref config) = lint_config {
            if let Some(p) = config.include_prefix() {
                Some(resolve_against_config_dir(
                    PathBuf::from(p),
                    config_dir.as_deref(),
                ))
            } else {
                // Config exists but no prefix set: default to config file directory
                config_dir.clone()
            }
        } else {
            None
        }
    });

    if cli.verbose
        && let Some(ref prefix) = include_prefix
    {
        eprintln!("Include prefix: {}", prefix.display());
    }

    // 8. Resolve the cache root and create the linter
    // Cache root precedence: --no-cache > --cache-dir > cache_dir in
    // .nginx-lint.toml (relative to the config file) > per-user default
    #[cfg(feature = "plugins")]
    let compilation_cache = {
        use nginx_lint::plugin::CompilationCache;

        if cli.no_cache {
            CompilationCache::Disabled
        } else if let Some(ref cache_dir) = cli.cache_dir {
            CompilationCache::Directory(cache_dir.clone())
        } else if let Some(cache_dir) = lint_config.as_ref().and_then(|c| c.cache_dir()) {
            CompilationCache::Directory(resolve_against_config_dir(
                PathBuf::from(cache_dir),
                config_dir.as_deref(),
            ))
        } else {
            CompilationCache::Default
        }
    };

    // In builds without the plugins feature the cache is never used; tell the
    // user instead of silently ignoring their configuration.
    #[cfg(not(feature = "plugins"))]
    if lint_config.as_ref().and_then(|c| c.cache_dir()).is_some() {
        eprintln!(
            "Warning: cache_dir in the configuration file has no effect in this build (compiled without the plugins feature)"
        );
    }

    // Builtin WASM plugins are compiled through a process-global loader, so
    // the cache root must be configured before the first Linter is created.
    #[cfg(feature = "wasm-builtin-plugins")]
    nginx_lint::plugin::builtin::configure_builtin_plugin_cache(compilation_cache.clone());

    // Pass --rule-only to the linter constructor so excluded builtin rules
    // are never constructed (WASM builtins outside the set are not even
    // compiled). External plugins are filtered after loading instead: their
    // rule names are only known once they are compiled.
    let rule_only: Option<std::collections::HashSet<String>> = if cli.rule_only.is_empty() {
        None
    } else {
        Some(cli.rule_only.iter().cloned().collect())
    };
    #[allow(unused_mut)]
    let mut linter = Linter::with_config_and_rule_only(
        lint_config.as_ref(),
        include_prefix.as_deref(),
        rule_only.as_ref(),
    );

    // Show builtin plugins in verbose mode
    #[cfg(any(feature = "wasm-builtin-plugins", feature = "native-builtin-plugins"))]
    if cli.verbose {
        use nginx_lint::plugin::BUILTIN_PLUGIN_NAMES;
        eprintln!("Loaded {} builtin plugin(s)", BUILTIN_PLUGIN_NAMES.len());
        for name in BUILTIN_PLUGIN_NAMES {
            eprintln!("  - {}", name);
        }
    }

    // Load custom plugins if specified
    #[cfg(feature = "plugins")]
    if let Some(ref plugins_dir) = cli.plugins {
        use nginx_lint::plugin::PluginLoader;

        match PluginLoader::new_with_cache(compilation_cache) {
            Ok(loader) => match loader.load_plugins(plugins_dir) {
                Ok(plugins) => {
                    if cli.verbose {
                        eprintln!(
                            "Loaded {} plugin(s) from {}",
                            plugins.len(),
                            plugins_dir.display()
                        );
                        if let (Some(cache_dir), Some((hits, misses))) =
                            (loader.cache_directory(), loader.cache_stats())
                        {
                            eprintln!(
                                "Plugin compilation cache: {} ({} hit(s), {} miss(es))",
                                cache_dir.display(),
                                hits,
                                misses
                            );
                        }
                    }
                    for plugin in plugins {
                        if cli.verbose {
                            eprintln!("  - {} ({})", plugin.name(), plugin.description());
                        }
                        linter.add_rule(plugin);
                    }
                }
                Err(e) => {
                    eprintln!("Error loading plugins: {}", e);
                    return ExitCode::from(2);
                }
            },
            Err(e) => {
                eprintln!("Error initializing plugin loader: {}", e);
                return ExitCode::from(2);
            }
        }
    }

    // Finish the --rule-only handling: builtin rules were already filtered
    // inside the linter constructor; validate that every requested name
    // corresponds to a rule this run could load, and prune the external
    // plugins that were loaded above.
    if !cli.rule_only.is_empty() {
        // Kept builtin rules plus every external plugin
        let registered = linter.rule_names();

        // Split unknown names into "exists but not loaded" vs "no such rule"
        // by consulting the full builtin catalog (rules + plugins).
        let mut not_loaded: Vec<&str> = Vec::new();
        let mut no_such_rule: Vec<&str> = Vec::new();
        for name in &cli.rule_only {
            if registered.contains(name) {
                continue;
            }
            if rule_exists_in_catalog(name) {
                not_loaded.push(name);
            } else {
                no_such_rule.push(name);
            }
        }

        if !not_loaded.is_empty() || !no_such_rule.is_empty() {
            if !no_such_rule.is_empty() {
                eprintln!(
                    "Error: --rule-only references unknown rule(s): {}",
                    no_such_rule.join(", ")
                );
                #[cfg(not(any(
                    feature = "wasm-builtin-plugins",
                    feature = "native-builtin-plugins"
                )))]
                eprintln!(
                    "Note: this binary was built without builtin plugins, so plugin rule names cannot be recognised even if they exist in other builds."
                );
            }
            if !not_loaded.is_empty() {
                eprintln!(
                    "Error: --rule-only rule(s) not loaded in this build (disabled in config or unsupported by the current feature set): {}",
                    not_loaded.join(", ")
                );
            }
            // Rules the filter excluded are inactive, not registered; list
            // them too so the user sees every name --rule-only accepts.
            let mut available: Vec<&str> = registered
                .iter()
                .chain(linter.inactive_rule_names())
                .map(String::as_str)
                .collect();
            available.sort();
            available.dedup();
            eprintln!("Loaded rules:");
            for name in &available {
                eprintln!("  - {}", name);
            }
            return ExitCode::from(2);
        }

        let keep: HashSet<&str> = cli.rule_only.iter().map(String::as_str).collect();
        // Builtin rules were already filtered inside the constructor; this
        // only prunes the external plugins loaded above.
        linter.remove_rules_by_name(|name| !keep.contains(name));
        // Surface the *filtered-out* external plugins to the ignore-comment
        // parser so existing `# nginx-lint:ignore <other-rule>` directives
        // stay quiet (valid names + dormant for unused-warning suppression);
        // filtered-out builtin rules are already inactive from the linter
        // constructor. The kept rules are intentionally excluded — their own
        // unused-ignore directives must still produce warnings.
        let mut inactive = linter.inactive_rule_names().clone();
        inactive.extend(
            registered
                .into_iter()
                .filter(|name| !keep.contains(name.as_str())),
        );
        linter.set_inactive_rules(inactive);

        if cli.verbose {
            eprintln!("Running only rule(s): {}", cli.rule_only.join(", "));
        }
    }

    // 8. Build results: stdin mode vs file mode
    let results: Vec<FileResult> = if let Some(ref content) = stdin_content {
        let result = lint_content(
            content,
            Path::new("<stdin>"),
            &linter,
            lint_config.as_ref(),
            cli.profile,
            initial_context.clone(),
        );
        if cli.fix {
            vec![fix_stdin(
                result,
                content,
                &linter,
                lint_config.as_ref(),
                initial_context,
            )]
        } else {
            vec![result]
        }
    } else {
        // Collect all files to lint (including files referenced by include directives)
        let mut seen_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        let mut included_files: Vec<IncludedFile> = Vec::new();

        let path_mappings = lint_config
            .as_ref()
            .map(|c| c.include_path_mappings())
            .unwrap_or(&[]);

        for file_path in &file_paths {
            let files_for_path = if initial_context.is_empty() {
                collect_included_files(
                    file_path,
                    |path| parse_config(path).map_err(|e| e.to_string()),
                    path_mappings,
                    include_prefix.as_deref(),
                )
            } else {
                collect_included_files_with_context(
                    file_path,
                    |path| parse_config(path).map_err(|e| e.to_string()),
                    initial_context.clone(),
                    path_mappings,
                    include_prefix.as_deref(),
                )
            };

            for inc in files_for_path {
                let canonical = inc.path.canonicalize().unwrap_or_else(|_| inc.path.clone());
                if !seen_paths.contains(&canonical) {
                    seen_paths.insert(canonical);
                    included_files.push(inc);
                }
            }
        }

        if cli.verbose {
            eprintln!("Linting {} file(s)", included_files.len());
            for inc in &included_files {
                eprintln!("  - {}", inc.path.display());
            }
        }

        // Lint files (parallel when not fixing and not profiling, sequential otherwise)
        if cli.fix {
            included_files
                .iter()
                .map(|inc| fix_file(inc, &linter, lint_config.as_ref(), cli.profile))
                .collect()
        } else if cli.profile {
            included_files
                .iter()
                .map(|inc| lint_file(inc, &linter, lint_config.as_ref(), true))
                .collect()
        } else {
            included_files
                .par_iter()
                .map(|inc| lint_file(inc, &linter, lint_config.as_ref(), false))
                .collect()
        }
    };

    // 9. Process results (report/exit code)
    process_results(
        results,
        cli.fix,
        cli.no_fail_on_warnings,
        cli.profile,
        &reporter,
        stdin_mode,
    )
}
