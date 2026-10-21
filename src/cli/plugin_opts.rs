//! Resolving the plugin-related options shared by the subcommands.
//!
//! [`cache_config`] and [`warn_ignored_cache_dir`] serve every subcommand.
//! `lint` resolves WASI inline against a configuration file it has already
//! loaded for the linting itself; `why`, `test-plugins` and `config` have no
//! file to lint, so they resolve it here with [`allow_wasi`].

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
    allow_wasi_from(cli, cli.config.as_deref())
}

/// [`allow_wasi`] reading the setting from `config_path` rather than the
/// global `--config`: `config validate` has a `--config` of its own, naming
/// the file it validates, and that file is the one whose setting decides
/// what its `[rules.<name>]` sections can refer to.
pub fn allow_wasi_from(cli: &Cli, config_path: Option<&std::path::Path>) -> Result<bool, ()> {
    use nginx_lint_common::config::LintConfig;

    if cli.allow_wasi_plugins {
        return Ok(true);
    }

    let config = match config_path {
        // An explicitly passed file that does not parse fails the command, as
        // it does for `lint`: continuing would deny WASI and surface as an
        // unrelated plugin-loading failure on the next line, with nothing
        // pointing at the configuration file. A discovered file stays silent,
        // which is what find_and_load does for `lint` too.
        Some(path) => match LintConfig::from_file(path) {
            Ok(config) => Some((config, path.to_path_buf())),
            Err(e) => {
                eprintln!("Error: {}", e);
                return Err(());
            }
        },
        None => LintConfig::find_and_load(std::path::Path::new(".")),
    };

    if let Some((ref config, ref path)) = config {
        warn_ignored_cache_dir(cli, config, path);
    }
    Ok(config.is_some_and(|(cfg, _)| cfg.plugins.allow_wasi_plugins))
}

/// Resolve the compilation cache from the CLI flags: `--no-cache` >
/// `--cache-dir` > the per-user default.
///
/// `cache_dir` in the configuration file is not read. The cache holds
/// precompiled native code that wasmtime loads after only an
/// engine-compatibility check, with no integrity check, and the
/// configuration file is usually found in the tree being linted, so it must
/// not choose where that code comes from.
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

/// Warn that `cache_dir` in the configuration file at `path` is ignored (see
/// [`cache_config`]). Silent when `--no-cache` or `--cache-dir` is given,
/// since the file's value never applied then.
pub fn warn_ignored_cache_dir(
    cli: &Cli,
    config: &nginx_lint_common::config::LintConfig,
    path: &std::path::Path,
) {
    if cli.no_cache || cli.cache_dir.is_some() || config.cache_dir().is_none() {
        return;
    }
    eprintln!(
        "Warning: cache_dir in {} is ignored; use --cache-dir to choose the cache directory",
        nginx_lint::reporter::one_line_path(path)
    );
}
