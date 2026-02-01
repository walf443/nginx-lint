use clap::{Parser, Subcommand};
use colored::control;
use nginx_lint::{
    apply_fixes, parse_config, pre_parse_checks, ColorMode, LintConfig, Linter, OutputFormat,
    Reporter, Severity,
};
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "nginx-lint")]
#[command(author, version, about = "Lint nginx configuration files", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to nginx configuration file
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,

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

    /// Do not exit with non-zero code on warnings and info (only fail on errors)
    #[arg(long)]
    no_fail_on_warnings: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Configuration file management
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
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
    r#"# nginx-lint configuration file
# See https://github.com/walf443/nginx-lint for documentation

# Color output settings
[color]
# Color mode: "auto", "always", or "never"
ui = "auto"
# Severity colors (available: black, red, green, yellow, blue, magenta, cyan, white,
#                  bright_black, bright_red, bright_green, bright_yellow, bright_blue,
#                  bright_magenta, bright_cyan, bright_white)
error = "red"
warning = "yellow"
info = "blue"

# =============================================================================
# Syntax Rules
# =============================================================================

[rules.duplicate-directive]
enabled = true

[rules.unmatched-braces]
enabled = true

[rules.unclosed-quote]
enabled = true

[rules.missing-semicolon]
enabled = true

# =============================================================================
# Security Rules
# =============================================================================

[rules.deprecated-ssl-protocol]
enabled = true
# Allowed protocols for auto-fix (default: ["TLSv1.2", "TLSv1.3"])
allowed_protocols = ["TLSv1.2", "TLSv1.3"]

[rules.server-tokens-enabled]
enabled = true

[rules.autoindex-enabled]
enabled = true

[rules.weak-ssl-ciphers]
enabled = true
# Weak cipher patterns to detect
weak_ciphers = [
    "NULL",
    "EXPORT",
    "DES",
    "RC4",
    "MD5",
    "aNULL",
    "eNULL",
    "ADH",
    "AECDH",
    "PSK",
    "SRP",
    "CAMELLIA",
]
# Required exclusion patterns
required_exclusions = ["!aNULL", "!eNULL", "!EXPORT", "!DES", "!RC4", "!MD5"]

# =============================================================================
# Style Rules
# =============================================================================

[rules.inconsistent-indentation]
enabled = true
# Indentation size (default: 4)
indent_size = 4

# =============================================================================
# Best Practices
# =============================================================================

[rules.gzip-not-enabled]
enabled = true

[rules.missing-error-log]
enabled = true
"#
    .to_string()
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

fn run_lint(cli: Cli) -> ExitCode {
    let file = match cli.file {
        Some(f) => f,
        None => {
            eprintln!("Error: FILE argument is required");
            eprintln!("Usage: nginx-lint <FILE>");
            eprintln!("       nginx-lint config init");
            eprintln!("       nginx-lint config validate");
            return ExitCode::from(2);
        }
    };

    // If a directory is specified, look for nginx.conf inside it
    let file_path = if file.is_dir() {
        let nginx_conf = file.join("nginx.conf");
        if !nginx_conf.exists() {
            eprintln!(
                "Error: nginx.conf not found in directory {}",
                file.display()
            );
            return ExitCode::from(2);
        }
        nginx_conf
    } else {
        file.clone()
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

    // Create reporter with color configuration
    let color_config = lint_config
        .as_ref()
        .map(|c| c.color.clone())
        .unwrap_or_default();
    let reporter = Reporter::with_colors(cli.format.into(), color_config);

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

    let has_issues = if cli.no_fail_on_warnings {
        // Only fail on errors
        errors.iter().any(|e| e.severity == Severity::Error)
    } else {
        // Default: fail on any issue (errors, warnings, or info)
        !errors.is_empty()
    };

    if has_issues {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Config { command }) => match command {
            ConfigCommands::Init { output, force } => run_init(output.clone(), *force),
            ConfigCommands::Validate { config } => run_validate(config.clone()),
        },
        None => run_lint(cli),
    }
}
