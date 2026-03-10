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
    /// Output configuration schema
    Schema {
        /// Output format: "json" for JSON Schema, "markdown" for human-readable documentation
        #[arg(short, long, default_value = "json")]
        format: SchemaFormat,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum SchemaFormat {
    Json,
    Markdown,
}

pub fn run_config(command: &ConfigCommands) -> ExitCode {
    match command {
        ConfigCommands::Init { output, force } => run_init(output.clone(), *force),
        ConfigCommands::Validate { config } => run_validate(config.clone()),
        ConfigCommands::Schema { format } => run_schema(*format),
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

fn run_schema(format: SchemaFormat) -> ExitCode {
    let mut schema = LintConfig::json_schema();
    enrich_schema_with_rule_descriptions(&mut schema);

    match format {
        SchemaFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&schema).unwrap());
        }
        SchemaFormat::Markdown => {
            let descriptions = get_rule_descriptions();
            print!("{}", generate_schema_markdown(&schema, &descriptions));
        }
    }
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

/// Generate Markdown documentation from JSON Schema and rule descriptions
fn generate_schema_markdown(
    schema: &serde_json::Value,
    descriptions: &std::collections::HashMap<String, String>,
) -> String {
    let mut out = String::new();
    out.push_str("# nginx-lint Configuration Reference\n\n");
    out.push_str("Configuration file: `.nginx-lint.toml`\n\n");

    // [color] section
    if let Some(color) = schema.pointer("/properties/color") {
        out.push_str("## `[color]`\n\n");
        out.push_str("Color output settings.\n\n");
        write_properties_table(&mut out, color, schema);
    }

    // [include] section
    if let Some(include) = schema.pointer("/properties/include") {
        out.push_str("## `[include]`\n\n");
        out.push_str("Include directive resolution settings.\n\n");
        write_properties_table(&mut out, include, schema);

        // path_map sub-items (resolve through $ref)
        let include_resolved = resolve_schema_object(include, schema);
        if let Some(items) = include_resolved.pointer("/properties/path_map/items") {
            let items_resolved = resolve_schema_object(items, schema);
            out.push_str("### `[[include.path_map]]`\n\n");
            write_properties_table(&mut out, items_resolved, schema);
        }
    }

    // [parser] section
    if let Some(parser) = schema.pointer("/properties/parser") {
        out.push_str("## `[parser]`\n\n");
        out.push_str("Parser-level settings.\n\n");
        write_properties_table(&mut out, parser, schema);
    }

    // [rules.*] section
    out.push_str("## `[rules.<name>]`\n\n");
    out.push_str(
        "Per-rule configuration. Each rule has an `enabled` field (default: `true`) \
         and may have rule-specific options.\n\n",
    );

    // Collect rule options from RuleConfig definition
    if let Some(rule_config) = schema.pointer("/definitions/RuleConfig/properties") {
        // Show common field
        out.push_str("### Common options\n\n");
        out.push_str("| Option | Type | Default | Description |\n");
        out.push_str("|--------|------|---------|-------------|\n");
        if let Some(enabled) = rule_config.get("enabled") {
            write_property_row(&mut out, "enabled", enabled, schema);
        }
        out.push('\n');

        // Show rule-specific options
        out.push_str("### Rule-specific options\n\n");
        out.push_str("| Option | Type | Default | Description |\n");
        out.push_str("|--------|------|---------|-------------|\n");
        let rule_config_obj = rule_config.as_object().unwrap();
        let mut keys: Vec<&String> = rule_config_obj.keys().collect();
        keys.sort();
        for key in keys {
            if key == "enabled" {
                continue;
            }
            write_property_row(&mut out, key, &rule_config_obj[key], schema);
        }
        out.push('\n');
    }

    // Rule list with descriptions
    out.push_str("### Available rules\n\n");
    out.push_str("| Rule | Enabled by default | Description |\n");
    out.push_str("|------|-------------------|-------------|\n");

    let disabled_by_default = LintConfig::DISABLED_BY_DEFAULT;
    let mut rules: Vec<(&String, &String)> = descriptions.iter().collect();
    rules.sort_by_key(|(name, _)| name.to_string());
    for (name, desc) in rules {
        let enabled = if disabled_by_default.contains(&name.as_str()) {
            "No"
        } else {
            "Yes"
        };
        out.push_str(&format!("| `{}` | {} | {} |\n", name, enabled, desc));
    }
    out.push('\n');

    out
}

/// Write a Markdown table of properties from a JSON Schema object
fn write_properties_table(out: &mut String, obj: &serde_json::Value, schema: &serde_json::Value) {
    // Resolve through $ref and allOf
    let resolved = resolve_schema_object(obj, schema);

    let props = match resolved.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return,
    };

    out.push_str("| Option | Type | Default | Description |\n");
    out.push_str("|--------|------|---------|-------------|\n");

    let mut keys: Vec<&String> = props.keys().collect();
    keys.sort();
    for key in keys {
        write_property_row(out, key, &props[key], schema);
    }
    out.push('\n');
}

/// Write a single property row in a Markdown table
fn write_property_row(
    out: &mut String,
    name: &str,
    prop: &serde_json::Value,
    schema: &serde_json::Value,
) {
    // Resolve $ref / allOf for type info, but keep original for description
    let resolved = resolve_schema_object(prop, schema);

    let type_str = format_type(resolved, schema);

    // Description: prefer original prop (may have description alongside $ref),
    // fall back to resolved definition
    let desc = prop
        .get("description")
        .or_else(|| resolved.get("description"))
        .or_else(|| resolved.pointer("/metadata/description"))
        .and_then(|d| d.as_str())
        .unwrap_or("-");
    // Trim to first line for table readability
    let desc_short = desc.split('\n').next().unwrap_or(desc);

    let default_str = prop
        .get("default")
        .or_else(|| resolved.get("default"))
        .or_else(|| resolved.pointer("/metadata/default"))
        .map(|d| format!("`{}`", d))
        .unwrap_or_else(|| "-".to_string());

    out.push_str(&format!(
        "| `{}` | {} | {} | {} |\n",
        name, type_str, default_str, desc_short
    ));
}

/// Resolve a schema object through $ref and allOf indirections
fn resolve_schema_object<'a>(
    value: &'a serde_json::Value,
    schema: &'a serde_json::Value,
) -> &'a serde_json::Value {
    // Direct $ref
    if let Some(resolved) = value
        .get("$ref")
        .and_then(|r| r.as_str())
        .and_then(|r| schema.pointer(r.trim_start_matches('#')))
    {
        return resolved;
    }
    // allOf with a single $ref
    if let Some(all_of) = value.get("allOf").and_then(|a| a.as_array()) {
        for item in all_of {
            if let Some(resolved) = item
                .get("$ref")
                .and_then(|r| r.as_str())
                .and_then(|r| schema.pointer(r.trim_start_matches('#')))
            {
                return resolved;
            }
        }
    }
    value
}

/// Format a JSON Schema type as a readable string
fn format_type(prop: &serde_json::Value, schema: &serde_json::Value) -> String {
    let prop = resolve_schema_object(prop, schema);

    // Check for enum values
    if let Some(enum_values) = prop.get("enum").and_then(|e| e.as_array()) {
        let values: Vec<String> = enum_values
            .iter()
            .filter_map(|v| v.as_str().map(|s| format!("`\"{}\"`", s)))
            .collect();
        if values.len() <= 5 {
            return values.join(" \\| ");
        }
        return format!("{} \\| ...", values[..3].join(" \\| "));
    }

    // Check for oneOf
    if let Some(one_of) = prop.get("oneOf").and_then(|o| o.as_array()) {
        let types: Vec<String> = one_of.iter().map(|s| format_type(s, schema)).collect();
        return types.join(" \\| ");
    }

    // Check for items (array)
    if let Some(type_val) = prop.get("type").and_then(|t| t.as_str()) {
        if type_val == "array" {
            if let Some(items) = prop.get("items") {
                return format!("{}[]", format_type(items, schema));
            }
            return "array".to_string();
        }
        return format!("`{}`", type_val);
    }

    "-".to_string()
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
