//! Loading, parsing and validating configuration files, by feature.

use super::*;
use std::collections::HashSet;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_default_config() {
    let config = LintConfig::default();
    assert!(config.is_rule_enabled("any-rule"));
}

#[test]
fn test_disabled_by_default_rules() {
    let config = LintConfig::default();
    // These rules should be disabled by default
    assert!(!config.is_rule_enabled("gzip-not-enabled"));
    assert!(!config.is_rule_enabled("missing-error-log"));
    // Other rules should still be enabled by default
    assert!(config.is_rule_enabled("server-tokens-enabled"));
}

#[test]
fn test_parse_config() {
    let toml_content = r#"
[rules.indent]
enabled = true
indent_size = 2

[rules.server-tokens-enabled]
enabled = false
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let config = LintConfig::from_file(file.path()).unwrap();

    assert!(config.is_rule_enabled("indent"));
    assert!(!config.is_rule_enabled("server-tokens-enabled"));
    assert!(config.is_rule_enabled("unknown-rule"));

    let indent_config = config.get_rule_config("indent").unwrap();
    assert_eq!(indent_config.indent_size, Some(IndentSize::Fixed(2)));
}

#[test]
fn test_empty_config() {
    let toml_content = "";
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let config = LintConfig::from_file(file.path()).unwrap();
    assert!(config.is_rule_enabled("any-rule"));
}

#[test]
fn test_indent_size_auto() {
    let toml_content = r#"
[rules.indent]
enabled = true
indent_size = "auto"
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let config = LintConfig::from_file(file.path()).unwrap();
    let indent_config = config.get_rule_config("indent").unwrap();
    assert_eq!(indent_config.indent_size, Some(IndentSize::Auto));
}

#[test]
fn test_plugins_config_defaults_to_denying_wasi() {
    // The default has to stay false: it is the sandbox guarantee, and a
    // config file with no [plugins] section must not weaken it.
    assert!(!LintConfig::default().plugins.allow_wasi_plugins);

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "[color]\nui = \"auto\"\n").unwrap();
    let config = LintConfig::from_file(file.path()).unwrap();
    assert!(!config.plugins.allow_wasi_plugins);
}

#[test]
fn test_plugins_allow_wasi_plugins() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "[plugins]\nallow_wasi_plugins = true\n").unwrap();

    let config = LintConfig::from_file(file.path()).unwrap();
    assert!(config.plugins.allow_wasi_plugins);
}

#[test]
fn test_validate_accepts_the_plugins_section() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "[plugins]\nallow_wasi_plugins = true\n").unwrap();

    let errors = LintConfig::validate_file(file.path()).unwrap();
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn test_validate_accepts_extra_rules_only_when_given() {
    // A `[rules.<name>]` section for a plugin's rule is unknown to the
    // builtin catalog; the caller that loaded the plugin passes its name.
    let mut file = NamedTempFile::new().unwrap();
    write!(
        file,
        "[rules.my-plugin-rule]\nenabled = false\nno_such_option = 1\n"
    )
    .unwrap();

    let errors = LintConfig::validate_file(file.path()).unwrap();
    assert!(
        matches!(&errors[..], [ValidationError::UnknownRule { name, .. }] if name == "my-plugin-rule"),
        "expected the rule to be unknown, got: {errors:?}"
    );

    let extra: HashSet<String> = ["my-plugin-rule".to_string()].into_iter().collect();
    let errors = LintConfig::validate_file_with_rules(file.path(), &extra).unwrap();
    // Known now, and its options are validated like any rule's
    assert!(
        matches!(&errors[..], [ValidationError::UnknownRuleOption { rule, option, .. }]
            if rule == "my-plugin-rule" && option == "no_such_option"),
        "expected only the option to be rejected, got: {errors:?}"
    );
}

#[test]
fn test_validate_rejects_unknown_plugins_key() {
    // Shortening the key to `allow_wasi` inside a `[plugins]` section is
    // the likely mistake, and the suggestion is what makes the error
    // useful.
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "[plugins]\nallow_wasi = true\n").unwrap();

    let errors = LintConfig::validate_file(file.path()).unwrap();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error, got: {errors:?}"
    );
    match &errors[0] {
        ValidationError::UnknownField {
            path, suggestion, ..
        } => {
            assert_eq!(path, "plugins.allow_wasi");
            assert_eq!(suggestion.as_deref(), Some("allow_wasi_plugins"));
        }
        other => panic!("expected UnknownField, got: {other:?}"),
    }
}

#[test]
fn test_color_config_default() {
    let config = LintConfig::default();
    assert_eq!(config.color_mode(), ColorMode::Auto);
}

#[test]
fn test_color_config_auto() {
    let toml_content = r#"
[color]
ui = "auto"
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let config = LintConfig::from_file(file.path()).unwrap();
    assert_eq!(config.color_mode(), ColorMode::Auto);
}

#[test]
fn test_color_config_never() {
    let toml_content = r#"
[color]
ui = "never"
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let config = LintConfig::from_file(file.path()).unwrap();
    assert_eq!(config.color_mode(), ColorMode::Never);
}

#[test]
fn test_color_config_always() {
    let toml_content = r#"
[color]
ui = "always"
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let config = LintConfig::from_file(file.path()).unwrap();
    assert_eq!(config.color_mode(), ColorMode::Always);
}

#[test]
fn test_color_config_default_colors() {
    let config = LintConfig::default();
    assert_eq!(config.color.error, Color::Red);
    assert_eq!(config.color.warning, Color::Yellow);
}

#[test]
fn test_color_config_custom_colors() {
    let toml_content = r#"
[color]
error = "magenta"
warning = "cyan"
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let config = LintConfig::from_file(file.path()).unwrap();
    assert_eq!(config.color.error, Color::Magenta);
    assert_eq!(config.color.warning, Color::Cyan);
}

#[test]
fn test_color_config_bright_colors() {
    let toml_content = r#"
[color]
error = "bright_red"
warning = "bright_yellow"
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let config = LintConfig::from_file(file.path()).unwrap();
    assert_eq!(config.color.error, Color::BrightRed);
    assert_eq!(config.color.warning, Color::BrightYellow);
}

#[test]
fn test_block_lines_max_block_lines_parsing() {
    let toml_content = r#"
[rules.block-lines]
enabled = true
max_block_lines = 50
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let config = LintConfig::from_file(file.path()).unwrap();
    assert!(config.is_rule_enabled("block-lines"));
    let rule_config = config.get_rule_config("block-lines").unwrap();
    assert_eq!(rule_config.max_block_lines, Some(50));
}

#[test]
fn test_block_lines_default_no_max() {
    let toml_content = r#"
[rules.block-lines]
enabled = true
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let config = LintConfig::from_file(file.path()).unwrap();
    let rule_config = config.get_rule_config("block-lines").unwrap();
    assert_eq!(rule_config.max_block_lines, None);
}

#[test]
fn test_block_lines_validation_rejects_unknown_option() {
    let toml_content = r#"
[rules.block-lines]
enabled = true
unknown_option = 42
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let errors = LintConfig::validate_file(file.path()).unwrap();
    assert_eq!(errors.len(), 1);
    match &errors[0] {
        ValidationError::UnknownRuleOption { rule, option, .. } => {
            assert_eq!(rule, "block-lines");
            assert_eq!(option, "unknown_option");
        }
        other => panic!("expected UnknownRuleOption, got: {:?}", other),
    }
}

#[test]
fn test_include_path_map_empty_by_default() {
    let config = LintConfig::default();
    assert!(config.include_path_mappings().is_empty());
}

#[test]
fn test_include_path_map_single_entry() {
    let toml_content = r#"
[[include.path_map]]
from = "sites-enabled"
to   = "sites-available"
"#;
    let config = LintConfig::parse(toml_content).unwrap();
    let mappings = config.include_path_mappings();
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].from, "sites-enabled");
    assert_eq!(mappings[0].to, "sites-available");
}

#[test]
fn test_include_path_map_multiple_entries_preserve_order() {
    let toml_content = r#"
[[include.path_map]]
from = "sites-enabled"
to   = "sites-available"

[[include.path_map]]
from = "/etc/nginx"
to   = "/usr/local/nginx"
"#;
    let config = LintConfig::parse(toml_content).unwrap();
    let mappings = config.include_path_mappings();
    assert_eq!(mappings.len(), 2);
    assert_eq!(mappings[0].from, "sites-enabled");
    assert_eq!(mappings[0].to, "sites-available");
    assert_eq!(mappings[1].from, "/etc/nginx");
    assert_eq!(mappings[1].to, "/usr/local/nginx");
}

#[test]
fn test_include_validation_rejects_unknown_field() {
    let toml_content = r#"
[include]
unknown_key = "value"
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let errors = LintConfig::validate_file(file.path()).unwrap();
    assert_eq!(errors.len(), 1);
    match &errors[0] {
        ValidationError::UnknownField { path, .. } => {
            assert_eq!(path, "include.unknown_key");
        }
        other => panic!("expected UnknownField, got: {:?}", other),
    }
}

#[test]
fn test_include_prefix_none_by_default() {
    let config = LintConfig::default();
    assert!(config.include_prefix().is_none());
}

#[test]
fn test_cache_dir_none_by_default() {
    let config = LintConfig::default();
    assert!(config.cache_dir().is_none());
}

#[test]
fn test_cache_dir_parsed() {
    let config = LintConfig::parse(r#"cache_dir = ".nginx-lint-cache""#).unwrap();
    assert_eq!(config.cache_dir(), Some(".nginx-lint-cache"));
}

#[test]
fn test_cache_dir_validation_accepted() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "cache_dir = \"/var/cache/nginx-lint\"").unwrap();

    let errors = LintConfig::validate_file(file.path()).unwrap();
    assert!(
        errors.is_empty(),
        "cache_dir should be a valid top-level field, got errors: {:?}",
        errors
    );
}

#[test]
fn test_include_prefix_parsed() {
    let toml_content = r#"
[include]
prefix = "/etc/nginx"
"#;
    let config = LintConfig::parse(toml_content).unwrap();
    assert_eq!(config.include_prefix(), Some("/etc/nginx"));
}

#[test]
fn test_include_prefix_with_path_map() {
    let toml_content = r#"
[include]
prefix = "."

[[include.path_map]]
from = "sites-enabled"
to   = "sites-available"
"#;
    let config = LintConfig::parse(toml_content).unwrap();
    assert_eq!(config.include_prefix(), Some("."));
    assert_eq!(config.include_path_mappings().len(), 1);
}

#[test]
fn test_include_prefix_validation_accepted() {
    let toml_content = r#"
[include]
prefix = "/etc/nginx"
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let errors = LintConfig::validate_file(file.path()).unwrap();
    assert!(
        errors.is_empty(),
        "prefix should be a valid include field, got errors: {:?}",
        errors
    );
}

#[test]
fn test_json_schema_is_valid() {
    let schema = LintConfig::json_schema();

    // Should have $schema key
    assert_eq!(
        schema.get("$schema").and_then(|v| v.as_str()),
        Some("https://json-schema.org/draft/2020-12/schema")
    );

    // Should have properties for all top-level config sections
    let props = schema.get("properties").unwrap().as_object().unwrap();
    assert!(props.contains_key("rules"), "missing 'rules' property");
    assert!(props.contains_key("color"), "missing 'color' property");
    assert!(props.contains_key("parser"), "missing 'parser' property");
    assert!(props.contains_key("include"), "missing 'include' property");
}

/// Regression test for https://github.com/walf443/nginx-lint/issues/172:
/// previously, `config validate` rejected configs that referenced real builtin
/// plugins whose names were missing from the validator's `known_rules`
/// whitelist. These names must remain recognised.
#[test]
fn test_validate_accepts_previously_drifted_builtin_plugins() {
    // Names that were drifted out of `known_rules` before the fix for #172.
    let previously_missing = [
        "client-max-body-size-not-set",
        "listen-http2-deprecated",
        "map-missing-default",
        "proxy-missing-host-header",
        "ssl-on-deprecated",
        "unreachable-location",
    ];

    for rule_name in previously_missing {
        let toml_content = format!("[rules.{rule_name}]\nenabled = false\n");
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", toml_content).unwrap();

        let errors = LintConfig::validate_file(file.path()).unwrap();
        assert!(
            errors.is_empty(),
            "rule '{rule_name}' should be a known builtin plugin name, \
             but `validate_file` reported errors: {errors:?}"
        );
    }
}

/// Sanity check that `KNOWN_RULE_NAMES` actually drives the validator
/// (rather than an inline list that can drift again).
#[test]
fn test_known_rules_constant_drives_validator() {
    for rule_name in LintConfig::KNOWN_RULE_NAMES {
        let toml_content = format!("[rules.{rule_name}]\nenabled = true\n");
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", toml_content).unwrap();

        let errors = LintConfig::validate_file(file.path()).unwrap();
        assert!(
            errors.is_empty(),
            "rule '{rule_name}' is listed in KNOWN_RULE_NAMES but the validator \
             rejected it: {errors:?}"
        );
    }
}

/// `NATIVE_RULE_NAMES` must be a subset of `KNOWN_RULE_NAMES`: editing either
/// list independently would mean the validator forgets about a native rule.
#[test]
fn test_native_rule_names_subset_of_known_rules() {
    let known: HashSet<&str> = LintConfig::KNOWN_RULE_NAMES.iter().copied().collect();
    let missing: Vec<&str> = LintConfig::NATIVE_RULE_NAMES
        .iter()
        .copied()
        .filter(|name| !known.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "NATIVE_RULE_NAMES entries missing from KNOWN_RULE_NAMES: {missing:?}"
    );
}

/// Negative case: a rule name that is in neither `NATIVE_RULE_NAMES` nor
/// `BUILTIN_PLUGIN_NAMES` must still produce an `UnknownRule` error —
/// otherwise the whitelist would silently degrade to "accept anything".
#[test]
fn test_validate_rejects_unknown_rule_name() {
    let toml_content = "[rules.no-such-rule-zzz]\nenabled = true\n";
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let errors = LintConfig::validate_file(file.path()).unwrap();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error, got: {errors:?}"
    );
    match &errors[0] {
        ValidationError::UnknownRule { name, .. } => {
            assert_eq!(name, "no-such-rule-zzz");
        }
        other => panic!("expected UnknownRule, got: {other:?}"),
    }
}

/// `KNOWN_RULE_NAMES` must not contain duplicate entries — a duplicated name
/// would silently mask a typo when the list is edited.
#[test]
fn test_known_rules_has_no_duplicates() {
    let mut seen: HashSet<&str> = HashSet::new();
    for name in LintConfig::KNOWN_RULE_NAMES {
        assert!(
            seen.insert(name),
            "duplicate entry in KNOWN_RULE_NAMES: '{name}'"
        );
    }
}

#[test]
fn test_json_schema_rule_config_has_all_fields() {
    let schema = LintConfig::json_schema();

    // RuleConfig definition should contain all fields from the struct
    // schemars v1 uses "$defs" instead of "definitions"
    let rule_config_def = schema
        .pointer("/$defs/RuleConfig")
        .expect("RuleConfig definition missing from schema");

    let props = rule_config_def
        .get("properties")
        .unwrap()
        .as_object()
        .unwrap();

    let expected_fields = [
        "enabled",
        "skip_version_check",
        "indent_size",
        "allowed_protocols",
        "weak_ciphers",
        "required_exclusions",
        "additional_contexts",
        "max_block_lines",
        "excluded_directives",
        "additional_directives",
    ];

    for field in &expected_fields {
        assert!(
            props.contains_key(*field),
            "RuleConfig schema missing field '{field}'"
        );
    }
}

#[test]
fn test_target_nginx_version_parsed() {
    let toml_content = r#"
target_nginx_version = "1.31.0"
"#;
    let config = LintConfig::parse(toml_content).unwrap();
    assert_eq!(config.target_nginx_version(), Some("1.31.0"));
}

#[test]
fn test_target_nginx_version_default_none() {
    let config = LintConfig::default();
    assert!(config.target_nginx_version().is_none());
}

#[test]
fn test_skip_version_check_per_rule() {
    let toml_content = r#"
[rules.nginx-rift]
enabled = true
skip_version_check = true
"#;
    let config = LintConfig::parse(toml_content).unwrap();
    assert!(config.rule_skip_version_check("nginx-rift"));
    assert!(!config.rule_skip_version_check("server-tokens-enabled"));
}

#[test]
fn test_rule_explicitly_configured() {
    let toml_content = r#"
[rules.indent]
enabled = true
"#;
    let config = LintConfig::parse(toml_content).unwrap();
    assert!(config.rule_explicitly_configured("indent"));
    assert!(!config.rule_explicitly_configured("server-tokens-enabled"));
}

#[test]
fn test_validator_accepts_target_nginx_version() {
    let toml_content = r#"
target_nginx_version = "1.31.0"
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let errors = LintConfig::validate_file(file.path()).unwrap();
    assert!(
        errors.is_empty(),
        "target_nginx_version should be a valid top-level field, got: {errors:?}"
    );
}

#[test]
fn test_validator_accepts_skip_version_check() {
    for rule_name in LintConfig::KNOWN_RULE_NAMES {
        let toml_content =
            format!("[rules.{rule_name}]\nenabled = true\nskip_version_check = true\n");
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", toml_content).unwrap();

        let errors = LintConfig::validate_file(file.path()).unwrap();
        assert!(
            errors.is_empty(),
            "skip_version_check should be valid for rule '{rule_name}', got: {errors:?}"
        );
    }
}
