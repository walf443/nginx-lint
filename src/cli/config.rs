use clap::Subcommand;
use nginx_lint::LintConfig;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Generate a default .nginx-lint.toml configuration file
    Init {
        /// Output path for the configuration file
        #[arg(short, long, default_value = ".nginx-lint.toml")]
        output: PathBuf,

        /// Overwrite existing file
        #[arg(long)]
        force: bool,
    },
    /// Validate configuration file for unknown fields
    Validate {
        /// Path to the configuration file to validate
        #[arg(short, long, default_value = ".nginx-lint.toml")]
        config: PathBuf,
    },
    /// Output JSON Schema for .nginx-lint.toml configuration file
    Schema,
}

pub fn run_config(command: &ConfigCommands) -> ExitCode {
    match command {
        ConfigCommands::Init { output, force } => run_init(output.clone(), *force),
        ConfigCommands::Validate { config } => run_validate(config.clone()),
        ConfigCommands::Schema => run_schema(),
    }
}

fn generate_default_config() -> String {
    nginx_lint::config::DEFAULT_CONFIG_TEMPLATE.to_string()
}

fn run_init(output: PathBuf, force: bool) -> ExitCode {
    if output.exists() && !force {
        eprintln!(
            "Error: {} already exists. Use --force to overwrite.",
            output.display()
        );
        return ExitCode::from(1);
    }

    let config_content = generate_default_config();

    match fs::write(&output, config_content) {
        Ok(()) => {
            eprintln!("Created {}", output.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error writing {}: {}", output.display(), e);
            ExitCode::from(2)
        }
    }
}

fn run_validate(config_path: PathBuf) -> ExitCode {
    if !config_path.exists() {
        eprintln!("Error: {} not found", config_path.display());
        return ExitCode::from(2);
    }

    match LintConfig::validate_file(&config_path) {
        Ok(errors) => {
            if errors.is_empty() {
                eprintln!("{}: OK", config_path.display());
                ExitCode::SUCCESS
            } else {
                eprintln!("{}:", config_path.display());
                for error in &errors {
                    eprintln!("  - {}", error);
                }
                eprintln!("\nFound {} error(s)", errors.len());
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::from(2)
        }
    }
}

fn run_schema() -> ExitCode {
    let mut schema = LintConfig::json_schema();

    // Enrich with rule descriptions from docs/plugins
    enrich_schema_with_rule_descriptions(&mut schema);

    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    ExitCode::SUCCESS
}

/// Add rule descriptions from docs and plugin metadata to the schema
fn enrich_schema_with_rule_descriptions(schema: &mut serde_json::Value) {
    let descriptions = get_rule_descriptions();

    let rules_path = if schema.pointer("/properties/rules/additionalProperties").is_some() {
        "/properties/rules/additionalProperties"
    } else {
        "/properties/rules/properties"
    };
    if let Some(serde_json::Value::Object(obj)) = schema.pointer_mut(rules_path)
        && !descriptions.is_empty()
    {
        let desc_value = serde_json::to_value(&descriptions).unwrap();
        obj.insert("x-rule-descriptions".to_string(), desc_value);
    }

    // Add title and description to the root schema
    if let serde_json::Value::Object(obj) = schema {
        obj.insert(
            "title".to_string(),
            serde_json::json!("nginx-lint configuration"),
        );
        obj.insert(
            "description".to_string(),
            serde_json::json!("Configuration file schema for nginx-lint (.nginx-lint.toml). See https://github.com/walf443/nginx-lint for details."),
        );
    }
}

/// Get rule descriptions from docs and plugin metadata
fn get_rule_descriptions() -> std::collections::HashMap<String, String> {
    let mut descriptions = std::collections::HashMap::new();

    // Native rule docs
    for doc in nginx_lint::docs::all_rule_docs() {
        descriptions.insert(doc.name.to_string(), doc.description.to_string());
    }

    // Plugin docs (when available)
    #[cfg(any(feature = "wasm-builtin-plugins", feature = "native-builtin-plugins"))]
    {
        for doc in nginx_lint::docs::all_rule_docs_with_plugins() {
            descriptions
                .entry(doc.name.clone())
                .or_insert(doc.description);
        }
    }

    descriptions
}
