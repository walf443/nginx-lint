use clap::{Parser, Subcommand};
use colored::control;
use nginx_lint::{
    apply_fixes, collect_included_files, parse_config, pre_parse_checks_with_config, ColorMode,
    IncludedFile, LintConfig, LintError, Linter, OutputFormat, Reporter, Severity,
};
use rayon::prelude::*;
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
    /// Start a web server to try nginx-lint in the browser
    Web {
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// Open browser automatically
        #[arg(long)]
        open: bool,
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

[rules.indent]
enabled = true
# Indentation size (default: 2)
indent_size = 2

# =============================================================================
# Best Practices
# =============================================================================

[rules.gzip-not-enabled]
enabled = true

[rules.missing-error-log]
enabled = true

# =============================================================================
# Parser Settings
# =============================================================================

[parser]
# Additional block directives for extension modules
# These are added to the built-in list (http, server, location, etc.)
# block_directives = ["my_custom_block", "another_block"]
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
    },
}

/// Lint a single included file and return the result
fn lint_file(
    included: &IncludedFile,
    linter: &Linter,
    lint_config: Option<&LintConfig>,
) -> FileResult {
    let path = &included.path;

    // Run pre-parse checks first
    let pre_parse_errors = pre_parse_checks_with_config(path, lint_config);

    // If there are pre-parse errors (like unmatched braces), return them
    if pre_parse_errors.iter().any(|e| e.severity == Severity::Error) {
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
        let (errors, ignored_count) = linter.lint_with_content(config, path, &content);
        FileResult::LintErrors {
            path: path.clone(),
            errors,
            ignored_count,
        }
    } else {
        FileResult::LintErrors {
            path: path.clone(),
            errors: Vec::new(),
            ignored_count: 0,
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

    // Collect all files to lint (including files referenced by include directives)
    let included_files = collect_included_files(&file_path, |path| {
        parse_config(path).map_err(|e| e.to_string())
    });

    if cli.verbose {
        eprintln!(
            "Linting {} file(s): {}",
            included_files.len(),
            file_path.display()
        );
        for inc in &included_files[1..] {
            eprintln!("  - {}", inc.path.display());
        }
    }

    let linter = Linter::with_config(lint_config.as_ref());

    // Lint files (parallel when not fixing, sequential when fixing)
    let results: Vec<FileResult> = if cli.fix {
        // Sequential processing for fix mode (file modifications should not be parallel)
        included_files
            .iter()
            .map(|inc| lint_file(inc, &linter, lint_config.as_ref()))
            .collect()
    } else {
        // Parallel processing for lint-only mode
        included_files
            .par_iter()
            .map(|inc| lint_file(inc, &linter, lint_config.as_ref()))
            .collect()
    };

    // Process results sequentially (for consistent output ordering)
    let mut all_errors = Vec::new();
    let mut has_fatal_error = false;

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
            }
        }
    }

    if has_fatal_error {
        return ExitCode::from(1);
    }

    let has_issues = if cli.no_fail_on_warnings {
        // Only fail on errors
        all_errors.iter().any(|e| e.severity == Severity::Error)
    } else {
        // Default: fail on any issue (errors, warnings, or info)
        !all_errors.is_empty()
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
        Some(Commands::Web { port, open }) => run_web(*port, *open),
        None => run_lint(cli),
    }
}

#[cfg(feature = "web-server")]
fn run_web(port: u16, open_browser: bool) -> ExitCode {
    use tiny_http::{Response, Server};

    // Embedded demo HTML
    const INDEX_HTML: &str = include_str!("../demo/index.html");

    // When web-server-embed-wasm feature is enabled, embed the WASM files
    #[cfg(feature = "web-server-embed-wasm")]
    const NGINX_LINT_JS: &str = include_str!("../demo/pkg/nginx_lint.js");
    #[cfg(feature = "web-server-embed-wasm")]
    const NGINX_LINT_WASM: &[u8] = include_bytes!("../demo/pkg/nginx_lint_bg.wasm");

    // Check if pkg directory exists (only when not embedding)
    #[cfg(not(feature = "web-server-embed-wasm"))]
    {
        let pkg_dir = std::path::Path::new("pkg");
        if !pkg_dir.exists() {
            eprintln!("Error: pkg/ directory not found.");
            eprintln!();
            eprintln!("Please build the WASM package first:");
            eprintln!("  wasm-pack build --target web --out-dir pkg --no-default-features --features wasm");
            eprintln!();
            eprintln!("Or rebuild with embedded WASM:");
            eprintln!("  wasm-pack build --target web --out-dir demo/pkg --no-default-features --features wasm");
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
    eprintln!("Starting nginx-lint web demo at {}", url);
    #[cfg(feature = "web-server-embed-wasm")]
    eprintln!("(WASM embedded in binary)");
    eprintln!("Press Ctrl+C to stop");

    if open_browser {
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&url).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("cmd").args(["/C", "start", &url]).spawn();
    }

    for request in server.incoming_requests() {
        let url = request.url();
        let response = match url {
            "/" | "/index.html" => {
                Response::from_string(INDEX_HTML)
                    .with_header(
                        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap()
                    )
            }
            "/pkg/nginx_lint.js" => {
                #[cfg(feature = "web-server-embed-wasm")]
                {
                    Response::from_string(NGINX_LINT_JS)
                        .with_header(
                            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/javascript"[..]).unwrap()
                        )
                }
                #[cfg(not(feature = "web-server-embed-wasm"))]
                {
                    serve_file_from_disk("./pkg/nginx_lint.js", "application/javascript")
                }
            }
            "/pkg/nginx_lint_bg.wasm" => {
                #[cfg(feature = "web-server-embed-wasm")]
                {
                    Response::from_data(NGINX_LINT_WASM.to_vec())
                        .with_header(
                            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/wasm"[..]).unwrap()
                        )
                }
                #[cfg(not(feature = "web-server-embed-wasm"))]
                {
                    serve_file_from_disk("./pkg/nginx_lint_bg.wasm", "application/wasm")
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
                    let file_path = format!(".{}", path);
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
fn serve_file_from_disk(file_path: &str, content_type: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    use tiny_http::Response;
    match std::fs::read(file_path) {
        Ok(content) => {
            Response::from_data(content)
                .with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap()
                )
        }
        Err(_) => Response::from_string("Not Found").with_status_code(404).into(),
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
