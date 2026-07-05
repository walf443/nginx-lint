pub mod config;
pub mod guide;
pub mod lint;
pub mod web;
pub mod why;

use clap::{Parser, Subcommand};
use nginx_lint::OutputFormat;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "nginx-lint")]
#[command(author, version, about = "Lint nginx configuration files", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to nginx configuration file(s)
    #[arg(value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// Output format
    #[arg(short = 'o', long, value_enum, default_value = "errorformat")]
    pub format: Format,

    /// Automatically fix problems
    #[arg(long)]
    pub fix: bool,

    /// Path to configuration file
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Force colored output
    #[arg(long, conflicts_with = "no_color")]
    pub color: bool,

    /// Disable colored output
    #[arg(long)]
    pub no_color: bool,

    /// Show verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Do not exit with non-zero code on warnings (only fail on errors)
    #[arg(long)]
    pub no_fail_on_warnings: bool,

    /// Specify parent context for files not included from a parent config.
    /// Comma-separated list of block names (e.g., "http,server" for sites-available files).
    /// This enables context-aware rules like server_tokens detection.
    #[arg(long, value_name = "CONTEXT")]
    pub context: Option<String>,

    /// Directory containing WASM plugins for custom lint rules (requires plugins feature)
    #[cfg(feature = "plugins")]
    #[arg(long, value_name = "DIR")]
    pub plugins: Option<PathBuf>,

    /// Directory for the WASM plugin compilation cache. Defaults to wasmtime's
    /// per-user cache directory (e.g. ~/.cache/wasmtime on Linux).
    #[cfg(feature = "plugins")]
    #[arg(long, value_name = "DIR", conflicts_with = "no_plugin_cache")]
    pub plugin_cache_dir: Option<PathBuf>,

    /// Disable the WASM plugin compilation cache and compile plugins on every run
    #[cfg(feature = "plugins")]
    #[arg(long)]
    pub no_plugin_cache: bool,

    /// Show profiling information (time spent per rule)
    #[arg(long)]
    pub profile: bool,

    /// Base directory for resolving relative include paths (similar to nginx -p prefix).
    /// Overrides include.prefix in .nginx-lint.toml.
    #[arg(short = 'p', long, value_name = "DIR")]
    pub prefix: Option<PathBuf>,

    /// Run only the specified rule(s). Other rules (including those enabled via
    /// .nginx-lint.toml) are disabled for this invocation. Useful for evaluating a
    /// new plugin or applying --fix for a single rule. Can be repeated or
    /// comma-separated, e.g. `--rule-only indent` or `--rule-only indent,gzip-not-enabled`.
    #[arg(long, value_name = "RULE", value_delimiter = ',')]
    pub rule_only: Vec<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Configuration file management
    Config {
        #[command(subcommand)]
        command: config::ConfigCommands,
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
    /// Show getting started guide (installation, usage, configuration)
    Guide,
    /// Show detailed documentation for a rule
    Why {
        /// Rule name (e.g., "server-tokens-enabled")
        rule: Option<String>,

        /// List all available rules
        #[arg(short, long)]
        list: bool,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Format {
    Errorformat,
    Json,
    GithubActions,
}

impl From<Format> for OutputFormat {
    fn from(f: Format) -> Self {
        match f {
            Format::Errorformat => OutputFormat::ErrorFormat,
            Format::Json => OutputFormat::Json,
            Format::GithubActions => OutputFormat::GithubActions,
        }
    }
}
