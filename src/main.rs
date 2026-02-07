use clap::{CommandFactory, Parser, Subcommand};
use colored::control;
#[cfg(feature = "plugins")]
use nginx_lint::linter::LintRule;
use nginx_lint::{
    ColorMode, IncludedFile, LintConfig, LintError, Linter, OutputFormat, Reporter, RuleProfile,
    Severity, apply_fixes, apply_fixes_to_content, collect_included_files,
    collect_included_files_with_context, parse_config, parse_string, pre_parse_checks_from_content,
    pre_parse_checks_with_config,
};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "nginx-lint")]
#[command(author, version, about = "Lint nginx configuration files", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to nginx configuration file(s)
    #[arg(value_name = "FILE")]
    files: Vec<PathBuf>,

    /// Output format
    #[arg(short = 'o', long, value_enum, default_value = "text")]
    format: Format,

    /// Automatically fix problems
    #[arg(long)]
    fix: bool,

    /// Path to configuration file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Force colored output
    #[arg(long, conflicts_with = "no_color")]
    color: bool,

    /// Disable colored output
    #[arg(long)]
    no_color: bool,

    /// Show verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Do not exit with non-zero code on warnings (only fail on errors)
    #[arg(long)]
    no_fail_on_warnings: bool,

    /// Specify parent context for files not included from a parent config.
    /// Comma-separated list of block names (e.g., "http,server" for sites-available files).
    /// This enables context-aware rules like server_tokens detection.
    #[arg(long, value_name = "CONTEXT")]
    context: Option<String>,

    /// Directory containing WASM plugins for custom lint rules (requires plugins feature)
    #[cfg(feature = "plugins")]
    #[arg(long, value_name = "DIR")]
    plugins: Option<PathBuf>,

    /// Show profiling information (time spent per rule)
    #[arg(long)]
    profile: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Configuration file management
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Start a web server to try nginx-lint in the browser
    Web {
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// Open browser automatically
        #[arg(long)]
        open: bool,
    },
    /// Show detailed documentation for a rule
    Why {
        /// Rule name (e.g., "server-tokens-enabled")
        rule: Option<String>,

        /// List all available rules
        #[arg(short, long)]
        list: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Generate a default .nginx-lint.toml configuration file
    Init {
        /// Output path for the configuration file
        #[arg(short, long, default_value = ".nginx-lint.toml")]
        output: PathBuf,

        /// Overwrite existing file
        #[arg(long)]
        force: bool,
    },
    /// Validate configuration file for unknown fields
    Validate {
        /// Path to the configuration file to validate
        #[arg(short, long, default_value = ".nginx-lint.toml")]
        config: PathBuf,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Format {
    Text,
    Json,
}

impl From<Format> for OutputFormat {
    fn from(f: Format) -> Self {
        match f {
            Format::Text => OutputFormat::Text,
            Format::Json => OutputFormat::Json,
        }
    }
}

fn generate_default_config() -> String {
    nginx_lint::config::DEFAULT_CONFIG_TEMPLATE.to_string()
}

fn run_init(output: PathBuf, force: bool) -> ExitCode {
    if output.exists() && !force {
        eprintln!(
            "Error: {} already exists. Use --force to overwrite.",
            output.display()
        );
        return ExitCode::from(1);
    }

    let config_content = generate_default_config();

    match fs::write(&output, config_content) {
        Ok(()) => {
            eprintln!("Created {}", output.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error writing {}: {}", output.display(), e);
            ExitCode::from(2)
        }
    }
}

fn run_validate(config_path: PathBuf) -> ExitCode {
    if !config_path.exists() {
        eprintln!("Error: {} not found", config_path.display());
        return ExitCode::from(2);
    }

    match LintConfig::validate_file(&config_path) {
        Ok(errors) => {
            if errors.is_empty() {
                eprintln!("{}: OK", config_path.display());
                ExitCode::SUCCESS
            } else {
                eprintln!("{}:", config_path.display());
                for error in &errors {
                    eprintln!("  - {}", error);
                }
                eprintln!("\nFound {} error(s)", errors.len());
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::from(2)
        }
    }
}

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

/// Check if the CLI arguments indicate stdin mode (single "-" argument)
fn is_stdin_mode(files: &[PathBuf]) -> bool {
    files.len() == 1 && files[0].as_os_str() == "-"
}

/// Lint content from stdin
fn run_lint_stdin(cli: &Cli) -> ExitCode {
    let mut content = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut content) {
        eprintln!("Error reading from stdin: {}", e);
        return ExitCode::from(2);
    }

    // Load configuration
    let lint_config = if let Some(config_path) = &cli.config {
        match LintConfig::from_file(config_path) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                eprintln!("Error: {}", e);
                return ExitCode::from(2);
            }
        }
    } else {
        LintConfig::find_and_load(Path::new("."))
    };

    // Configure color output
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

    let color_config = lint_config
        .as_ref()
        .map(|c| c.color.clone())
        .unwrap_or_default();
    let reporter = Reporter::with_colors(cli.format.into(), color_config);

    let stdin_path = Path::new("<stdin>");

    // Parse context option
    let initial_context: Vec<String> = cli
        .context
        .as_ref()
        .map(|s| s.split(',').map(|c| c.trim().to_string()).collect())
        .unwrap_or_default();

    // Run pre-parse checks on the content
    let pre_parse_errors = pre_parse_checks_from_content(&content, lint_config.as_ref());

    if pre_parse_errors
        .iter()
        .any(|e| e.severity == Severity::Error)
    {
        if cli.fix {
            let fixes: Vec<_> = pre_parse_errors
                .iter()
                .flat_map(|e| e.fixes.iter())
                .collect();
            let (fixed_content, count) = apply_fixes_to_content(&content, &fixes);
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
    let mut parse_result = match parse_string(&content) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error parsing stdin: {}", e);
            return ExitCode::from(1);
        }
    };

    // Set context if specified (same approach as collect_included_files_with_context)
    if !initial_context.is_empty() {
        parse_result.include_context = initial_context;
    }

    // Lint the parsed config
    #[allow(unused_mut)]
    let mut linter = Linter::with_config(lint_config.as_ref());

    #[cfg(feature = "plugins")]
    if let Some(ref plugins_dir) = cli.plugins {
        use nginx_lint::plugin::PluginLoader;
        match PluginLoader::new() {
            Ok(loader) => match loader.load_plugins(plugins_dir) {
                Ok(plugins) => {
                    for plugin in plugins {
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

    let (errors, ignored_count) = linter.lint_with_content(&parse_result, stdin_path, &content);

    if cli.fix {
        let fixes: Vec<_> = errors.iter().flat_map(|e| e.fixes.iter()).collect();
        let (fixed_content, count) = apply_fixes_to_content(&content, &fixes);
        if count > 0 {
            print!("{}", fixed_content);
        } else {
            // No fixes to apply, output original content
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
}

fn run_lint(cli: Cli) -> ExitCode {
    // Handle stdin mode
    if is_stdin_mode(&cli.files) {
        return run_lint_stdin(&cli);
    }

    if cli.files.is_empty() {
        let _ = Cli::command().print_help();
        eprintln!();
        return ExitCode::from(2);
    }

    // Resolve file paths (handle directories by looking for nginx.conf inside)
    let mut file_paths: Vec<PathBuf> = Vec::new();
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
            file_paths.push(nginx_conf);
        } else {
            file_paths.push(file.clone());
        }
    }

    // Load configuration
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
        // Try to find .nginx-lint.toml in first file's directory or current directory
        let search_dir = file_paths
            .first()
            .and_then(|p| p.parent())
            .unwrap_or(std::path::Path::new("."));
        let config = LintConfig::find_and_load(search_dir);
        if cli.verbose && config.is_some() {
            eprintln!("Found .nginx-lint.toml");
        }
        config
    };

    // Configure color output (CLI flags take precedence over config)
    if cli.color {
        control::set_override(true);
    } else if cli.no_color {
        control::set_override(false);
    } else if let Some(ref config) = lint_config {
        match config.color_mode() {
            ColorMode::Always => control::set_override(true),
            ColorMode::Never => control::set_override(false),
            ColorMode::Auto => {} // Let colored crate decide
        }
    }

    // Create reporter with color configuration
    let color_config = lint_config
        .as_ref()
        .map(|c| c.color.clone())
        .unwrap_or_default();
    let reporter = Reporter::with_colors(cli.format.into(), color_config);

    // Parse context option if specified (comma-separated list of block names)
    let initial_context: Vec<String> = cli
        .context
        .as_ref()
        .map(|s| s.split(',').map(|c| c.trim().to_string()).collect())
        .unwrap_or_default();

    if cli.verbose && !initial_context.is_empty() {
        eprintln!("Using context: {}", initial_context.join(" > "));
    }

    // Collect all files to lint (including files referenced by include directives)
    // Use a set to avoid processing the same file multiple times
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
            // Canonicalize path to detect duplicates
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

    // Lint files (parallel when not fixing and not profiling, sequential otherwise)
    // Profiling requires sequential execution for accurate timing
    let results: Vec<FileResult> = if cli.fix || cli.profile {
        // Sequential processing for fix mode or profile mode
        included_files
            .iter()
            .map(|inc| lint_file(inc, &linter, lint_config.as_ref(), cli.profile))
            .collect()
    } else {
        // Parallel processing for lint-only mode
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
        // Only fail on errors
        all_errors.iter().any(|e| e.severity == Severity::Error)
    } else {
        // Default: fail on any issue (errors or warnings)
        !all_errors.is_empty()
    };

    if has_issues {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn run_why(rule: Option<String>, list: bool) -> ExitCode {
    use colored::Colorize;

    if list {
        // List all rules
        eprintln!("{}", "Available rules:".bold());
        eprintln!();

        #[cfg(feature = "builtin-plugins")]
        {
            use nginx_lint::docs::all_rule_docs_with_plugins;
            let docs = all_rule_docs_with_plugins();
            let mut by_category: std::collections::HashMap<&str, Vec<_>> =
                std::collections::HashMap::new();
            for doc in &docs {
                by_category
                    .entry(doc.category.as_str())
                    .or_default()
                    .push(doc);
            }

            for category in nginx_lint::RULE_CATEGORIES {
                if let Some(rules) = by_category.get(category) {
                    eprintln!("  {} {}", "▸".cyan(), category.bold());
                    for doc in rules {
                        let suffix = if doc.is_plugin { " (plugin)" } else { "" };
                        eprintln!(
                            "    {} - {}{}",
                            doc.name.yellow(),
                            doc.description,
                            suffix.dimmed()
                        );
                    }
                    eprintln!();
                }
            }
        }
        #[cfg(not(feature = "builtin-plugins"))]
        {
            use nginx_lint::docs::all_rule_docs;
            let docs = all_rule_docs();
            let mut by_category: std::collections::HashMap<&str, Vec<_>> =
                std::collections::HashMap::new();
            for doc in docs {
                by_category.entry(doc.category).or_default().push(doc);
            }

            for category in nginx_lint::RULE_CATEGORIES {
                if let Some(rules) = by_category.get(category) {
                    eprintln!("  {} {}", "▸".cyan(), category.bold());
                    for doc in rules {
                        eprintln!("    {} - {}", doc.name.yellow(), doc.description);
                    }
                    eprintln!();
                }
            }
        }

        eprintln!(
            "Use {} to see detailed documentation.",
            "nginx-lint why <rule-name>".cyan()
        );
        return ExitCode::SUCCESS;
    }

    let rule_name = match rule {
        Some(name) => name,
        None => {
            eprintln!("Usage: nginx-lint why <rule-name>");
            eprintln!("       nginx-lint why --list");
            eprintln!();
            eprintln!("Use {} to see all available rules.", "--list".cyan());
            return ExitCode::from(1);
        }
    };

    #[cfg(feature = "builtin-plugins")]
    {
        use nginx_lint::docs::get_rule_doc_with_plugins;
        match get_rule_doc_with_plugins(&rule_name) {
            Some(doc) => {
                print_rule_doc_owned(&doc);
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("{} Unknown rule: {}", "Error:".red().bold(), rule_name);
                eprintln!();
                eprintln!(
                    "Use {} to see all available rules.",
                    "nginx-lint why --list".cyan()
                );
                ExitCode::from(1)
            }
        }
    }
    #[cfg(not(feature = "builtin-plugins"))]
    {
        use nginx_lint::docs::get_rule_doc;
        match get_rule_doc(&rule_name) {
            Some(doc) => {
                print_rule_doc(doc);
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("{} Unknown rule: {}", "Error:".red().bold(), rule_name);
                eprintln!();
                eprintln!(
                    "Use {} to see all available rules.",
                    "nginx-lint why --list".cyan()
                );
                ExitCode::from(1)
            }
        }
    }
}

#[cfg(not(feature = "builtin-plugins"))]
fn print_rule_doc(doc: &nginx_lint::docs::RuleDoc) {
    use colored::Colorize;

    eprintln!();
    eprintln!("{} {}", "Rule:".bold(), doc.name.yellow());
    eprintln!("{} {}", "Category:".bold(), doc.category);
    eprintln!("{} {}", "Severity:".bold(), doc.severity);
    eprintln!();
    eprintln!("{}", "Why:".bold());
    for line in doc.why.lines() {
        eprintln!("  {}", line);
    }
    eprintln!();
    eprintln!("{}", "Bad Example:".bold().red());
    eprintln!("{}", "─".repeat(60).dimmed());
    for line in doc.bad_example.lines() {
        eprintln!("  {}", line);
    }
    eprintln!("{}", "─".repeat(60).dimmed());
    eprintln!();
    eprintln!("{}", "Good Example:".bold().green());
    eprintln!("{}", "─".repeat(60).dimmed());
    for line in doc.good_example.lines() {
        eprintln!("  {}", line);
    }
    eprintln!("{}", "─".repeat(60).dimmed());

    if !doc.references.is_empty() {
        eprintln!();
        eprintln!("{}", "References:".bold());
        for reference in doc.references {
            eprintln!("  • {}", reference.cyan());
        }
    }
    eprintln!();
}

#[cfg(feature = "builtin-plugins")]
fn print_rule_doc_owned(doc: &nginx_lint::docs::RuleDocOwned) {
    use colored::Colorize;

    eprintln!();
    eprintln!(
        "{} {}{}",
        "Rule:".bold(),
        doc.name.yellow(),
        if doc.is_plugin {
            " (plugin)".dimmed().to_string()
        } else {
            "".to_string()
        }
    );
    eprintln!("{} {}", "Category:".bold(), doc.category);
    eprintln!("{} {}", "Severity:".bold(), doc.severity);
    eprintln!();
    if !doc.why.is_empty() {
        eprintln!("{}", "Why:".bold());
        for line in doc.why.lines() {
            eprintln!("  {}", line);
        }
        eprintln!();
    }
    if !doc.bad_example.is_empty() {
        eprintln!("{}", "Bad Example:".bold().red());
        eprintln!("{}", "─".repeat(60).dimmed());
        for line in doc.bad_example.lines() {
            eprintln!("  {}", line);
        }
        eprintln!("{}", "─".repeat(60).dimmed());
        eprintln!();
    }
    if !doc.good_example.is_empty() {
        eprintln!("{}", "Good Example:".bold().green());
        eprintln!("{}", "─".repeat(60).dimmed());
        for line in doc.good_example.lines() {
            eprintln!("  {}", line);
        }
        eprintln!("{}", "─".repeat(60).dimmed());
    }

    if !doc.references.is_empty() {
        eprintln!();
        eprintln!("{}", "References:".bold());
        for reference in &doc.references {
            eprintln!("  • {}", reference.cyan());
        }
    }
    eprintln!();
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Config { command }) => match command {
            ConfigCommands::Init { output, force } => run_init(output.clone(), *force),
            ConfigCommands::Validate { config } => run_validate(config.clone()),
        },
        Some(Commands::Web { port, open }) => run_web(*port, *open),
        Some(Commands::Why { rule, list }) => run_why(rule.clone(), *list),
        None => run_lint(cli),
    }
}

#[cfg(feature = "web-server")]
fn run_web(port: u16, open_browser: bool) -> ExitCode {
    use tiny_http::{Response, Server};

    // Embedded web HTML
    const INDEX_HTML: &str = include_str!("../web/index.html");
    const RULES_HTML: &str = include_str!("../web/rules.html");

    // When web-server-embed-wasm feature is enabled, embed the WASM files
    #[cfg(feature = "web-server-embed-wasm")]
    const NGINX_LINT_JS: &str = include_str!("../web/pkg/nginx_lint.js");
    #[cfg(feature = "web-server-embed-wasm")]
    const NGINX_LINT_WASM: &[u8] = include_bytes!("../web/pkg/nginx_lint_bg.wasm");

    // Check if pkg directory exists (only when not embedding)
    #[cfg(not(feature = "web-server-embed-wasm"))]
    {
        let pkg_dir = std::path::Path::new("pkg");
        if !pkg_dir.exists() {
            eprintln!("Error: pkg/ directory not found.");
            eprintln!();
            eprintln!("Please build the WASM package first:");
            eprintln!(
                "  wasm-pack build --target web --out-dir pkg --no-default-features --features wasm"
            );
            eprintln!();
            eprintln!("Or rebuild with embedded WASM:");
            eprintln!(
                "  wasm-pack build --target web --out-dir web/pkg --no-default-features --features wasm"
            );
            eprintln!("  cargo build --features web-server-embed-wasm");
            return ExitCode::from(2);
        }
    }

    let addr = format!("0.0.0.0:{}", port);
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error starting server: {}", e);
            return ExitCode::from(2);
        }
    };

    let url = format!("http://localhost:{}", port);
    eprintln!("Starting nginx-lint web server at {}", url);
    #[cfg(feature = "web-server-embed-wasm")]
    eprintln!("(WASM embedded in binary)");
    eprintln!("Press Ctrl+C to stop");

    if open_browser {
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&url).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", &url])
            .spawn();
    }

    for request in server.incoming_requests() {
        let url = request.url();
        let response = match url {
            "/" | "/index.html" => Response::from_string(INDEX_HTML).with_header(
                tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/html; charset=utf-8"[..],
                )
                .unwrap(),
            ),
            "/rules" | "/rules.html" => Response::from_string(RULES_HTML).with_header(
                tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/html; charset=utf-8"[..],
                )
                .unwrap(),
            ),
            "/pkg/nginx_lint.js" => {
                #[cfg(feature = "web-server-embed-wasm")]
                {
                    Response::from_string(NGINX_LINT_JS).with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/javascript"[..],
                        )
                        .unwrap(),
                    )
                }
                #[cfg(not(feature = "web-server-embed-wasm"))]
                {
                    serve_file_from_disk("./web/pkg/nginx_lint.js", "application/javascript")
                }
            }
            "/pkg/nginx_lint_bg.wasm" => {
                #[cfg(feature = "web-server-embed-wasm")]
                {
                    Response::from_data(NGINX_LINT_WASM.to_vec()).with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/wasm"[..],
                        )
                        .unwrap(),
                    )
                }
                #[cfg(not(feature = "web-server-embed-wasm"))]
                {
                    serve_file_from_disk("./web/pkg/nginx_lint_bg.wasm", "application/wasm")
                }
            }
            path if path.starts_with("/pkg/") => {
                #[cfg(feature = "web-server-embed-wasm")]
                {
                    // Other pkg files not embedded
                    Response::from_string("Not Found").with_status_code(404)
                }
                #[cfg(not(feature = "web-server-embed-wasm"))]
                {
                    let file_path = format!("./web{}", path);
                    let content_type = if path.ends_with(".js") {
                        "application/javascript"
                    } else if path.ends_with(".wasm") {
                        "application/wasm"
                    } else if path.ends_with(".d.ts") {
                        "application/typescript"
                    } else {
                        "application/octet-stream"
                    };
                    serve_file_from_disk(&file_path, content_type)
                }
            }
            _ => Response::from_string("Not Found").with_status_code(404),
        };

        let _ = request.respond(response);
    }

    ExitCode::SUCCESS
}

#[cfg(all(feature = "web-server", not(feature = "web-server-embed-wasm")))]
fn serve_file_from_disk(
    file_path: &str,
    content_type: &str,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    use tiny_http::Response;
    match std::fs::read(file_path) {
        Ok(content) => Response::from_data(content).with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap(),
        ),
        Err(_) => Response::from_string("Not Found")
            .with_status_code(404)
            .into(),
    }
}

#[cfg(not(feature = "web-server"))]
fn run_web(_port: u16, _open_browser: bool) -> ExitCode {
    eprintln!("Error: Web server feature is not enabled.");
    eprintln!();
    eprintln!("Rebuild with the web-server feature:");
    eprintln!("  cargo build --features web-server");
    ExitCode::from(2)
}
