mod cli;

use clap::Parser;
use cli::{Cli, Commands};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Config { command }) => cli::config::run_config(command),
        Some(Commands::Web { port, open }) => cli::web::run_web(*port, *open),
        Some(Commands::Why { rule, list }) => cli::why::run_why(rule.clone(), *list),
        None => cli::lint::run_lint(cli),
    }
}
