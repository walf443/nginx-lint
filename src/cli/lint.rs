use super::Cli;
use clap::CommandFactory;
use colored::control;
#[cfg(feature = "plugins")]
use nginx_lint::linter::LintRule;
use nginx_lint::{
    ColorMode, IncludedFile, LintConfig, LintError, Linter, Reporter, RuleProfile, Severity,
    apply_fixes, apply_fixes_to_content, collect_included_files,
    collect_included_files_with_context, parse_config, parse_string, pre_parse_checks_from_content,
    pre_parse_checks_with_config,
};
use rayon::prelude::*;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Result of linting a single file
enum FileResult {
    PreParseErrors {
        path: PathBuf,
        errors: Vec<LintError>,
    },
    ParseError {
        path: PathBuf,
        error: String,
    },
    LintErrors {
        path: PathBuf,
        errors: Vec<LintError>,
        ignored_count: usize,
        profiles: Option<Vec<RuleProfile>>,
    },
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
    sorted.sort_by(|a, b| b.1.0.cmp(&a.1.0));

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

/// Lint a single included file and return the result
fn lint_file(
    included: &IncludedFile,
    linter: &Linter,
    lint_config: Option<&LintConfig>,
    profile: bool,
) -> FileResult {
    let path = &included.path;

    // Run pre-parse checks first
    let pre_parse_errors = pre_parse_checks_with_config(path, lint_config);

    // If there are pre-parse errors (like unmatched braces), return them
    if pre_parse_errors
        .iter()
        .any(|e| e.severity == Severity::Error)
    {
        return FileResult::PreParseErrors {
            path: path.clone(),
            errors: pre_parse_errors,
        };
    }

    // Handle parse errors
    if let Some(ref error) = included.parse_error {
        return FileResult::ParseError {
            path: path.clone(),
            error: error.clone(),
        };
    }

    // Read file content for ignore comment support
    let content = std::fs::read_to_string(path).unwrap_or_default();

    // Lint the parsed config
    if let Some(ref config) = included.config {
        if profile {
            let (errors, ignored_count, profiles) =
                linter.lint_with_content_and_profile(config, path, &content);
            FileResult::LintErrors {
                path: path.clone(),
                errors,
                ignored_count,
                profiles: Some(profiles),
            }
        } else {
            let (errors, ignored_count) = linter.lint_with_content(config, path, &content);
            FileResult::LintErrors {
                path: path.clone(),
                errors,
                ignored_count,
                profiles: None,
            }
        }
    } else {
        FileResult::LintErrors {
            path: path.clone(),
            errors: Vec::new(),
            ignored_count: 0,
            profiles: None,
        }
    }
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
    let lint_config = if let Some(config_path) = &cli.config {
        match LintConfig::from_file(config_path) {
            Ok(cfg) => {
                if cli.verbose {
                    eprintln!("Using config: {}", config_path.display());
                }
                Some(cfg)
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
        let config = LintConfig::find_and_load(search_dir);
        if cli.verbose && config.is_some() {
            eprintln!("Found .nginx-lint.toml");
        }
        config
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

    // 7. Create linter and load plugins
    #[allow(unused_mut)]
    let mut linter = Linter::with_config(lint_config.as_ref());

    // Show builtin plugins in verbose mode
    #[cfg(feature = "builtin-plugins")]
    if cli.verbose {
        use nginx_lint::plugin::builtin::BUILTIN_PLUGIN_NAMES;
        eprintln!("Loaded {} builtin plugin(s)", BUILTIN_PLUGIN_NAMES.len());
        for name in BUILTIN_PLUGIN_NAMES {
            eprintln!("  - {}", name);
        }
    }

    // Load custom plugins if specified
    #[cfg(feature = "plugins")]
    if let Some(ref plugins_dir) = cli.plugins {
        use nginx_lint::plugin::PluginLoader;

        match PluginLoader::new() {
            Ok(loader) => match loader.load_plugins(plugins_dir) {
                Ok(plugins) => {
                    if cli.verbose {
                        eprintln!(
                            "Loaded {} plugin(s) from {}",
                            plugins.len(),
                            plugins_dir.display()
                        );
                    }
                    for plugin in plugins {
                        if cli.verbose {
                            eprintln!("  - {} ({})", plugin.name(), plugin.description());
                        }
                        linter.add_rule(Box::new(plugin));
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

    // 8. Branch: stdin mode vs file mode
    if let Some(ref content) = stdin_content {
        // --- stdin mode ---
        let stdin_path = Path::new("<stdin>");

        // Run pre-parse checks on the content
        let pre_parse_errors = pre_parse_checks_from_content(content, lint_config.as_ref());

        if pre_parse_errors
            .iter()
            .any(|e| e.severity == Severity::Error)
        {
            if cli.fix {
                let fixes: Vec<_> = pre_parse_errors
                    .iter()
                    .flat_map(|e| e.fixes.iter())
                    .collect();
                let (fixed_content, count) = apply_fixes_to_content(content, &fixes);
                if count > 0 {
                    print!("{}", fixed_content);
                } else {
                    reporter.report(&pre_parse_errors, stdin_path, 0);
                }
            } else {
                reporter.report(&pre_parse_errors, stdin_path, 0);
            }
            return ExitCode::from(1);
        }

        // Parse the content
        let mut parse_result = match parse_string(content) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Error parsing stdin: {}", e);
                return ExitCode::from(1);
            }
        };

        // Set context if specified
        if !initial_context.is_empty() {
            parse_result.include_context = initial_context;
        }

        let (errors, ignored_count) = linter.lint_with_content(&parse_result, stdin_path, content);

        if cli.fix {
            let fixes: Vec<_> = errors.iter().flat_map(|e| e.fixes.iter()).collect();
            let (fixed_content, count) = apply_fixes_to_content(content, &fixes);
            if count > 0 {
                print!("{}", fixed_content);
            } else {
                print!("{}", content);
            }
        } else {
            reporter.report(&errors, stdin_path, ignored_count);
        }

        let has_issues = if cli.no_fail_on_warnings {
            errors.iter().any(|e| e.severity == Severity::Error)
        } else {
            !errors.is_empty()
        };

        if has_issues {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        }
    } else {
        // --- file mode ---

        // Collect all files to lint (including files referenced by include directives)
        let mut seen_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        let mut included_files: Vec<IncludedFile> = Vec::new();

        for file_path in &file_paths {
            let files_for_path = if initial_context.is_empty() {
                collect_included_files(file_path, |path| {
                    parse_config(path).map_err(|e| e.to_string())
                })
            } else {
                collect_included_files_with_context(
                    file_path,
                    |path| parse_config(path).map_err(|e| e.to_string()),
                    initial_context.clone(),
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
        let results: Vec<FileResult> = if cli.fix || cli.profile {
            included_files
                .iter()
                .map(|inc| lint_file(inc, &linter, lint_config.as_ref(), cli.profile))
                .collect()
        } else {
            included_files
                .par_iter()
                .map(|inc| lint_file(inc, &linter, lint_config.as_ref(), false))
                .collect()
        };

        // Process results sequentially (for consistent output ordering)
        let mut all_errors = Vec::new();
        let mut has_fatal_error = false;
        let mut all_profiles: Vec<RuleProfile> = Vec::new();

        for result in results {
            match result {
                FileResult::PreParseErrors { path, errors } => {
                    if cli.fix {
                        match apply_fixes(&path, &errors) {
                            Ok(count) => {
                                if count > 0 {
                                    eprintln!("Applied {} fix(es) to {}", count, path.display());
                                }
                            }
                            Err(e) => {
                                eprintln!("Error applying fixes to {}: {}", path.display(), e);
                            }
                        }
                    } else {
                        reporter.report(&errors, &path, 0);
                    }
                    has_fatal_error = true;
                }
                FileResult::ParseError { path, error } => {
                    eprintln!("Error parsing {}: {}", path.display(), error);
                    has_fatal_error = true;
                }
                FileResult::LintErrors {
                    path,
                    errors,
                    ignored_count,
                    profiles,
                } => {
                    if cli.fix {
                        match apply_fixes(&path, &errors) {
                            Ok(count) => {
                                if count > 0 {
                                    eprintln!("Applied {} fix(es) to {}", count, path.display());
                                }
                            }
                            Err(e) => {
                                eprintln!("Error applying fixes to {}: {}", path.display(), e);
                            }
                        }
                    } else {
                        reporter.report(&errors, &path, ignored_count);
                    }
                    all_errors.extend(errors);
                    if let Some(p) = profiles {
                        all_profiles.extend(p);
                    }
                }
            }
        }

        // Display profile results if requested
        if cli.profile && !all_profiles.is_empty() {
            display_profile(&all_profiles);
        }

        if has_fatal_error {
            return ExitCode::from(1);
        }

        let has_issues = if cli.no_fail_on_warnings {
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
}
