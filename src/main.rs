use clap::Parser;
use colored::control;
use nginx_lint::{
    apply_fixes, parse_config, pre_parse_checks, ColorMode, LintConfig, Linter, OutputFormat,
    Reporter, Severity,
};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "nginx-lint")]
#[command(author, version, about = "Lint nginx configuration files", long_about = None)]
struct Cli {
    /// Path to nginx configuration file
    #[arg(value_name = "FILE")]
    file: PathBuf,

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

fn main() -> ExitCode {
    let cli = Cli::parse();
    let reporter = Reporter::new(cli.format.into());

    // If a directory is specified, look for nginx.conf inside it
    let file_path = if cli.file.is_dir() {
        let nginx_conf = cli.file.join("nginx.conf");
        if !nginx_conf.exists() {
            eprintln!(
                "Error: nginx.conf not found in directory {}",
                cli.file.display()
            );
            return ExitCode::from(2);
        }
        nginx_conf
    } else {
        cli.file.clone()
    };

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
        // Try to find .nginx-lint.toml in file's directory or current directory
        let search_dir = file_path.parent().unwrap_or(std::path::Path::new("."));
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

    if cli.verbose {
        eprintln!("Linting: {}", file_path.display());
    }

    // Run pre-parse checks first (these work even if parsing fails)
    let pre_parse_errors = pre_parse_checks(&file_path);

    // If there are pre-parse errors (like unmatched braces), handle them
    // This avoids cryptic parser error messages
    if pre_parse_errors.iter().any(|e| e.severity == Severity::Error) {
        if cli.fix {
            // Apply fixes for pre-parse errors
            match apply_fixes(&file_path, &pre_parse_errors) {
                Ok(count) => {
                    if count > 0 {
                        eprintln!("Applied {} fix(es) to {}", count, file_path.display());
                    } else {
                        eprintln!("No automatic fixes available");
                    }
                }
                Err(e) => {
                    eprintln!("Error applying fixes: {}", e);
                    return ExitCode::from(2);
                }
            }
        } else {
            reporter.report(&pre_parse_errors, &file_path);
        }
        return ExitCode::from(1);
    }

    let config = match parse_config(&file_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error: {}", e);
            return ExitCode::from(2);
        }
    };

    let linter = Linter::with_config(lint_config.as_ref());
    let errors = linter.lint(&config, &file_path);

    if cli.fix {
        // Apply fixes
        match apply_fixes(&file_path, &errors) {
            Ok(count) => {
                if count > 0 {
                    eprintln!("Applied {} fix(es) to {}", count, file_path.display());
                } else {
                    eprintln!("No automatic fixes available");
                }
            }
            Err(e) => {
                eprintln!("Error applying fixes: {}", e);
                return ExitCode::from(2);
            }
        }
    } else {
        reporter.report(&errors, &file_path);
    }

    let has_errors = errors.iter().any(|e| e.severity == Severity::Error);
    if has_errors {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
