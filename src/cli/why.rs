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
fn find_rule_doc(name: &str, cli: &Cli) -> Option<RuleDocOwned> {
    if let Some(doc) = nginx_lint::docs::get_rule_doc(name) {
        return Some(doc.into());
    }

    #[cfg(any(feature = "wasm-builtin-plugins", feature = "native-builtin-plugins"))]
    if let Some(doc) = nginx_lint::docs::get_rule_doc_with_plugins(name) {
        return Some(doc);
    }

    #[cfg(feature = "plugins")]
    if let Some(doc) = external_plugin_docs(cli)
        .into_iter()
        .find(|d| d.name == name)
    {
        return Some(doc);
    }

    let _ = cli;
    None
}

/// Collect the documentation for every rule this invocation can see.
///
/// Same precedence as [`find_rule_doc`]; used by `--list`, which needs all
/// of them anyway.
fn collect_rule_docs(cli: &Cli) -> Vec<RuleDocOwned> {
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
    docs.extend(external_plugin_docs(cli));

    let _ = cli;
    docs
}

/// Resolve the compilation cache from the CLI flags.
///
/// Only the flags are honoured; unlike `lint`, `why` does not read
/// .nginx-lint.toml, so cache_dir from the config file does not apply.
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

#[cfg(feature = "plugins")]
fn external_plugin_docs(cli: &Cli) -> Vec<RuleDocOwned> {
    use colored::Colorize;

    let Some(ref dir) = cli.plugins else {
        return Vec::new();
    };

    match nginx_lint::docs::external_plugin_docs(dir, cache_config(cli)) {
        Ok(docs) => docs,
        Err(e) => {
            eprintln!(
                "{} failed to load plugins from {}: {}",
                "Warning:".yellow().bold(),
                dir.display(),
                e
            );
            Vec::new()
        }
    }
}

pub fn run_why(rule: Option<String>, list: bool, cli: &Cli) -> ExitCode {
    use colored::Colorize;

    // The builtin WASM plugins are compiled through a process-global
    // loader, so the cache flags have to reach it before anything loads
    // them (mirrors run_lint).
    #[cfg(all(feature = "wasm-builtin-plugins", feature = "plugins"))]
    nginx_lint::plugin::builtin::configure_builtin_plugin_cache(cache_config(cli));

    if list {
        let docs = collect_rule_docs(cli);
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

    match find_rule_doc(&rule_name, cli) {
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
