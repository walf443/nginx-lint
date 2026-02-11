pub mod config;
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
    #[arg(short = 'o', long, value_enum, default_value = "text")]
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

    /// Show profiling information (time spent per rule)
    #[arg(long)]
    pub profile: bool,
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
    Text,
    Json,
    GithubActions,
}

impl From<Format> for OutputFormat {
    fn from(f: Format) -> Self {
        match f {
            Format::Text => OutputFormat::Text,
            Format::Json => OutputFormat::Json,
            Format::GithubActions => OutputFormat::GithubActions,
        }
    }
}
