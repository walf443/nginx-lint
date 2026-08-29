mod cli;

use clap::Parser;
use cli::{Cli, Commands};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();

    // clap's conflicts_with does not fire when the two flags straddle the
    // subcommand (`--cache-dir X why --no-cache`), which `global = true`
    // makes expressible, so enforce it here as well.
    #[cfg(feature = "plugins")]
    if cli.no_cache && cli.cache_dir.is_some() {
        eprintln!("error: the argument '--cache-dir <DIR>' cannot be used with '--no-cache'");
        return ExitCode::from(2);
    }

    match &cli.command {
        Some(Commands::Config { command }) => cli::config::run_config(command),
        Some(Commands::Guide) => cli::guide::run_guide(),
        Some(Commands::Web { port, open }) => cli::web::run_web(*port, *open),
        Some(Commands::Why { rule, list }) => cli::why::run_why(rule.clone(), *list, &cli),
        None => cli::lint::run_lint(cli),
    }
}
