use clap::Parser;
use nginx_lint::{parse_config, pre_parse_checks, Linter, OutputFormat, Reporter, Severity};
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
    #[arg(short, long, value_enum, default_value = "text")]
    format: Format,

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

    if cli.verbose {
        eprintln!("Linting: {}", file_path.display());
    }

    // Run pre-parse checks first (these work even if parsing fails)
    let pre_parse_errors = pre_parse_checks(&file_path);

    // If there are pre-parse errors (like unmatched braces), report them and exit
    // This avoids cryptic parser error messages
    if pre_parse_errors.iter().any(|e| e.severity == Severity::Error) {
        reporter.report(&pre_parse_errors, &file_path);
        return ExitCode::from(1);
    }

    let config = match parse_config(&file_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error: {}", e);
            return ExitCode::from(2);
        }
    };

    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &file_path);

    reporter.report(&errors, &file_path);

    let has_errors = errors.iter().any(|e| e.severity == Severity::Error);
    if has_errors {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
