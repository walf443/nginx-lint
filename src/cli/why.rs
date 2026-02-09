use std::process::ExitCode;

pub fn run_why(rule: Option<String>, list: bool) -> ExitCode {
    use colored::Colorize;

    if list {
        // List all rules
        eprintln!("{}", "Available rules:".bold());
        eprintln!();

        #[cfg(any(feature = "builtin-plugins", feature = "native-builtin-plugins"))]
        {
            use nginx_lint::docs::all_rule_docs_with_plugins;
            let docs = all_rule_docs_with_plugins();
            let mut by_category: std::collections::HashMap<&str, Vec<_>> =
                std::collections::HashMap::new();
            for doc in &docs {
                by_category
                    .entry(doc.category.as_str())
                    .or_default()
                    .push(doc);
            }

            for category in nginx_lint::RULE_CATEGORIES {
                if let Some(rules) = by_category.get(category) {
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
                }
            }
        }
        #[cfg(not(any(feature = "builtin-plugins", feature = "native-builtin-plugins")))]
        {
            use nginx_lint::docs::all_rule_docs;
            let docs = all_rule_docs();
            let mut by_category: std::collections::HashMap<&str, Vec<_>> =
                std::collections::HashMap::new();
            for doc in docs {
                by_category.entry(doc.category).or_default().push(doc);
            }

            for category in nginx_lint::RULE_CATEGORIES {
                if let Some(rules) = by_category.get(category) {
                    eprintln!("  {} {}", "▸".cyan(), category.bold());
                    for doc in rules {
                        eprintln!("    {} - {}", doc.name.yellow(), doc.description);
                    }
                    eprintln!();
                }
            }
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

    #[cfg(any(feature = "builtin-plugins", feature = "native-builtin-plugins"))]
    {
        use nginx_lint::docs::get_rule_doc_with_plugins;
        match get_rule_doc_with_plugins(&rule_name) {
            Some(doc) => {
                print_rule_doc_owned(&doc);
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
    #[cfg(not(any(feature = "builtin-plugins", feature = "native-builtin-plugins")))]
    {
        use nginx_lint::docs::get_rule_doc;
        match get_rule_doc(&rule_name) {
            Some(doc) => {
                print_rule_doc(doc);
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
}

#[cfg(not(any(feature = "builtin-plugins", feature = "native-builtin-plugins")))]
fn print_rule_doc(doc: &nginx_lint::docs::RuleDoc) {
    use colored::Colorize;

    eprintln!();
    eprintln!("{} {}", "Rule:".bold(), doc.name.yellow());
    eprintln!("{} {}", "Category:".bold(), doc.category);
    eprintln!("{} {}", "Severity:".bold(), doc.severity);
    eprintln!();
    eprintln!("{}", "Why:".bold());
    for line in doc.why.lines() {
        eprintln!("  {}", line);
    }
    eprintln!();
    eprintln!("{}", "Bad Example:".bold().red());
    eprintln!("{}", "─".repeat(60).dimmed());
    for line in doc.bad_example.lines() {
        eprintln!("  {}", line);
    }
    eprintln!("{}", "─".repeat(60).dimmed());
    eprintln!();
    eprintln!("{}", "Good Example:".bold().green());
    eprintln!("{}", "─".repeat(60).dimmed());
    for line in doc.good_example.lines() {
        eprintln!("  {}", line);
    }
    eprintln!("{}", "─".repeat(60).dimmed());

    if !doc.references.is_empty() {
        eprintln!();
        eprintln!("{}", "References:".bold());
        for reference in doc.references {
            eprintln!("  • {}", reference.cyan());
        }
    }
    eprintln!();
}

#[cfg(any(feature = "builtin-plugins", feature = "native-builtin-plugins"))]
fn print_rule_doc_owned(doc: &nginx_lint::docs::RuleDocOwned) {
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
