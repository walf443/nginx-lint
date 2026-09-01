//! Resolving the plugin-related options that `why` and `test-plugins` share.
//!
//! `lint` resolves the same things, but does it inline against a
//! configuration file it has already loaded for the linting itself; these two
//! subcommands have no file to lint, so they resolve them here.

use crate::Cli;

/// Whether plugins that import WASI may be loaded.
///
/// The configuration file is read for this one setting and nothing else,
/// because it decides whether a plugin loads at all rather than where its
/// artifacts go: a project whose plugins need WASI would otherwise have
/// `lint` working from the configuration file and these subcommands failing
/// on the same directory. The search starts at the current directory, having
/// no file to start from.
///
/// Returns `Err` after reporting the failure, so the caller exits 2.
pub fn allow_wasi(cli: &Cli) -> Result<bool, ()> {
    use nginx_lint_common::config::LintConfig;

    if cli.allow_wasi_plugins {
        return Ok(true);
    }

    let config = match cli.config {
        // An explicitly passed file that does not parse fails the command, as
        // it does for `lint`: continuing would deny WASI and surface as an
        // unrelated plugin-loading failure on the next line, with nothing
        // pointing at the configuration file. A discovered file stays silent,
        // which is what find_and_load does for `lint` too.
        Some(ref path) => match LintConfig::from_file(path) {
            Ok(config) => Some(config),
            Err(e) => {
                eprintln!("Error: {}", e);
                return Err(());
            }
        },
        None => LintConfig::find_and_load(std::path::Path::new(".")).map(|(cfg, _)| cfg),
    };

    Ok(config.is_some_and(|cfg| cfg.plugins.allow_wasi_plugins))
}

/// Resolve the compilation cache from the CLI flags.
///
/// Only the flags are honoured; cache_dir is not read from the configuration
/// file, so a plugin compiled here may not be the copy `lint` cached.
pub fn cache_config(cli: &Cli) -> nginx_lint::plugin::CompilationCache {
    use nginx_lint::plugin::CompilationCache;

    if cli.no_cache {
        CompilationCache::Disabled
    } else if let Some(ref cache_dir) = cli.cache_dir {
        CompilationCache::Directory(cache_dir.clone())
    } else {
        CompilationCache::Default
    }
}
