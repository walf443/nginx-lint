use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "nginx-lint-plugin-sdk",
    version,
    about = "Builds nginx-lint plugins from Lua scripts"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build a plugin component from a Lua script
    Build {
        /// The plugin script
        script: PathBuf,
        /// Where to write the component (default: the script's name with a .wasm extension)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Print the licenses of the third-party code embedded in the Lua runtime
    License,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Build { script, output } => {
            let source = std::fs::read(&script)
                .with_context(|| format!("failed to read {}", script.display()))?;
            let name = script
                .file_name()
                .and_then(|name| name.to_str())
                .with_context(|| format!("{} has no usable file name", script.display()))?;
            let component = nginx_lint_plugin_sdk::build_plugin(name, &source)?;
            let output = output.unwrap_or_else(|| script.with_extension("wasm"));
            std::fs::write(&output, component)
                .with_context(|| format!("failed to write {}", output.display()))?;
            println!("Wrote {}", output.display());
            Ok(())
        }
        Command::License => {
            print!("{}", nginx_lint_plugin_sdk::licenses::render());
            Ok(())
        }
    }
}
