use crate::Cli;
use nginx_lint::docs::RuleDocOwned;
use nginx_lint_common::nginx_version::format_range;
use std::process::ExitCode;

/// Look up one rule, loading plugins only if the name is not a native rule.
///
/// Native rules first, then builtin plugins, then the plugins in a
/// `--plugins` directory, so a name defined twice resolves to the earlier
/// one. Checking native rules first also keeps `why <native-rule>` from
/// compiling and instantiating every builtin WASM component for a name
/// they cannot supply.
fn find_rule_doc(name: &str, cli: &Cli) -> Result<Option<RuleDocOwned>, ()> {
    if let Some(doc) = nginx_lint::docs::get_rule_doc(name) {
        return Ok(Some(doc.into()));
    }

    #[cfg(any(feature = "wasm-builtin-plugins", feature = "native-builtin-plugins"))]
    if let Some(doc) = nginx_lint::docs::get_rule_doc_with_plugins(name) {
        return Ok(Some(doc));
    }

    #[cfg(feature = "plugins")]
    if let Some(doc) = external_plugin_docs(cli)?
        .into_iter()
        .find(|d| d.name == name)
    {
        return Ok(Some(doc));
    }

    let _ = cli;
    Ok(None)
}

/// Collect the documentation for every rule this invocation can see.
///
/// Same precedence as [`find_rule_doc`]; used by `--list`, which needs all
/// of them anyway.
fn collect_rule_docs(cli: &Cli) -> Result<Vec<RuleDocOwned>, ()> {
    // Only the plugin features below extend this
    #[allow(unused_mut)]
    let mut docs: Vec<RuleDocOwned> = nginx_lint::docs::all_rule_docs()
        .iter()
        .map(|doc| (*doc).into())
        .collect();

    #[cfg(any(feature = "wasm-builtin-plugins", feature = "native-builtin-plugins"))]
    {
        // all_rule_docs_with_plugins() also includes the native rules, so
        // take only the plugin half to avoid listing them twice.
        docs.extend(
            nginx_lint::docs::all_rule_docs_with_plugins()
                .into_iter()
                .filter(|doc| doc.is_plugin),
        );
    }

    #[cfg(feature = "plugins")]
    docs.extend(external_plugin_docs(cli)?);

    let _ = cli;
    Ok(docs)
}

/// Whether plugins that import WASI may be loaded.
///
/// `why` reads .nginx-lint.toml for this one setting, unlike cache_dir just
/// below, because it decides whether a plugin loads at all rather than where
/// its artifacts go: a project whose plugins need WASI would otherwise have
/// `lint` working from the config file and `why` failing on the same
/// directory. The search starts at the current directory, `why` having no
/// file to start from.
#[cfg(feature = "plugins")]
fn allow_wasi(cli: &Cli) -> bool {
    use nginx_lint_common::config::LintConfig;

    if cli.allow_wasi_plugins {
        return true;
    }

    let config = match cli.config {
        // An explicitly passed file that does not parse is reported rather
        // than treated as absent: falling back to the default would deny WASI
        // and surface as an unrelated plugin-loading failure, with nothing
        // pointing at the config file. A discovered file stays silent, which
        // is what find_and_load does for `lint` too.
        Some(ref path) => match LintConfig::from_file(path) {
            Ok(config) => Some(config),
            Err(e) => {
                eprintln!("Warning: {}", e);
                None
            }
        },
        None => LintConfig::find_and_load(std::path::Path::new(".")).map(|(cfg, _)| cfg),
    };

    config.is_some_and(|cfg| cfg.plugins.allow_wasi_plugins)
}

/// Resolve the compilation cache from the CLI flags.
///
/// Only the flags are honoured; `why` does not read cache_dir from
/// .nginx-lint.toml, so a plugin it compiles may not be the copy `lint`
/// cached.
#[cfg(feature = "plugins")]
fn cache_config(cli: &Cli) -> nginx_lint::plugin::CompilationCache {
    use nginx_lint::plugin::CompilationCache;

    if cli.no_cache {
        CompilationCache::Disabled
    } else if let Some(ref cache_dir) = cli.cache_dir {
        CompilationCache::Directory(cache_dir.clone())
    } else {
        CompilationCache::Default
    }
}

/// Load the docs for a `--plugins` directory, if one was given.
///
/// Returns Err after reporting the failure. `lint` exits 2 when a plugin
/// directory cannot be loaded, and `why` says nothing useful about a rule
/// it never managed to load, so it fails the same way rather than
/// pretending the directory contributed nothing.
#[cfg(feature = "plugins")]
fn external_plugin_docs(cli: &Cli) -> Result<Vec<RuleDocOwned>, ()> {
    let Some(ref dir) = cli.plugins else {
        return Ok(Vec::new());
    };

    nginx_lint::docs::external_plugin_docs(dir, cache_config(cli), allow_wasi(cli)).map_err(|e| {
        eprintln!("Error loading plugins: {}", e);
    })
}

pub fn run_why(rule: Option<String>, list: bool, cli: &Cli) -> ExitCode {
    use colored::Colorize;

    // Validate the plugin options before anything else: looking a native
    // rule up never loads plugins, so without this the same unusable
    // --plugins or --cache-dir would be reported for one rule name and
    // silently ignored for another.
    #[cfg(feature = "plugins")]
    if let Some(ref dir) = cli.plugins {
        if !dir.is_dir() {
            eprintln!(
                "Error loading plugins: Plugin directory not found: {}",
                dir.display()
            );
            return ExitCode::from(2);
        }
        // Building the loader validates the cache configuration; it stops
        // short of compiling any plugin.
        if let Err(e) = nginx_lint::plugin::PluginLoader::new_with_cache(cache_config(cli)) {
            eprintln!("Error loading plugins: {}", e);
            return ExitCode::from(2);
        }
    }

    // The builtin WASM plugins are compiled through a process-global
    // loader, so the cache flags have to reach it before anything loads
    // them (mirrors run_lint).
    #[cfg(all(feature = "wasm-builtin-plugins", feature = "plugins"))]
    nginx_lint::plugin::builtin::configure_builtin_plugin_cache(cache_config(cli));

    if list {
        let Ok(docs) = collect_rule_docs(cli) else {
            return ExitCode::from(2);
        };
        eprintln!("{}", "Available rules:".bold());
        eprintln!();

        let mut by_category: std::collections::BTreeMap<&str, Vec<&RuleDocOwned>> =
            std::collections::BTreeMap::new();
        for doc in &docs {
            by_category
                .entry(doc.category.as_str())
                .or_default()
                .push(doc);
        }

        let print_category = |category: &str, rules: &[&RuleDocOwned]| {
            eprintln!("  {} {}", "▸".cyan(), category.bold());
            for doc in rules {
                let suffix = if doc.is_plugin { " (plugin)" } else { "" };
                eprintln!(
                    "    {} - {}{}",
                    doc.name.yellow(),
                    doc.description,
                    suffix.dimmed()
                );
            }
            eprintln!();
        };

        for category in nginx_lint::RULE_CATEGORIES {
            if let Some(rules) = by_category.remove(category) {
                print_category(category, &rules);
            }
        }
        // A plugin may declare a category of its own; print what is left so
        // it is listed rather than silently dropped.
        for (category, rules) in by_category {
            print_category(category, &rules);
        }

        eprintln!(
            "Use {} to see detailed documentation.",
            "nginx-lint why <rule-name>".cyan()
        );
        return ExitCode::SUCCESS;
    }

    let rule_name = match rule {
        Some(name) => name,
        None => {
            eprintln!("Usage: nginx-lint why <rule-name>");
            eprintln!("       nginx-lint why --list");
            eprintln!();
            eprintln!("Use {} to see all available rules.", "--list".cyan());
            return ExitCode::from(1);
        }
    };

    let found = match find_rule_doc(&rule_name, cli) {
        Ok(found) => found,
        Err(()) => return ExitCode::from(2),
    };

    match found {
        Some(doc) => {
            print_rule_doc(&doc);
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("{} Unknown rule: {}", "Error:".red().bold(), rule_name);
            eprintln!();
            eprintln!(
                "Use {} to see all available rules.",
                "nginx-lint why --list".cyan()
            );
            ExitCode::from(1)
        }
    }
}

fn print_rule_doc(doc: &RuleDocOwned) {
    use colored::Colorize;

    eprintln!();
    eprintln!(
        "{} {}{}",
        "Rule:".bold(),
        doc.name.yellow(),
        if doc.is_plugin {
            " (plugin)".dimmed().to_string()
        } else {
            "".to_string()
        }
    );
    eprintln!("{} {}", "Category:".bold(), doc.category);
    eprintln!("{} {}", "Severity:".bold(), doc.severity);
    if let Some(range) = format_range(
        doc.min_nginx_version.as_deref(),
        doc.max_nginx_version.as_deref(),
    ) {
        eprintln!("{} {}", "Applies to:".bold(), range);
    }
    eprintln!();
    if !doc.why.is_empty() {
        eprintln!("{}", "Why:".bold());
        for line in doc.why.lines() {
            eprintln!("  {}", line);
        }
        eprintln!();
    }
    if !doc.bad_example.is_empty() {
        eprintln!("{}", "Bad Example:".bold().red());
        eprintln!("{}", "─".repeat(60).dimmed());
        for line in doc.bad_example.lines() {
            eprintln!("  {}", line);
        }
        eprintln!("{}", "─".repeat(60).dimmed());
        eprintln!();
    }
    if !doc.good_example.is_empty() {
        eprintln!("{}", "Good Example:".bold().green());
        eprintln!("{}", "─".repeat(60).dimmed());
        for line in doc.good_example.lines() {
            eprintln!("  {}", line);
        }
        eprintln!("{}", "─".repeat(60).dimmed());
    }

    if !doc.references.is_empty() {
        eprintln!();
        eprintln!("{}", "References:".bold());
        for reference in &doc.references {
            eprintln!("  • {}", reference.cyan());
        }
    }
    eprintln!();
}
