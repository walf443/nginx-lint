use nginx_lint::{parse_config, pre_parse_checks, Linter, Severity};
use std::path::PathBuf;

fn fixtures_base() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn parser_fixture(name: &str) -> PathBuf {
    fixtures_base()
        .join("parser")
        .join(name)
        .join("nginx.conf")
}

fn rule_fixture(category: &str, rule: &str) -> PathBuf {
    fixtures_base()
        .join("rules")
        .join(category)
        .join(rule)
        .join("nginx.conf")
}

fn misc_fixture(name: &str) -> PathBuf {
    fixtures_base()
        .join(name)
        .join("nginx.conf")
}

#[test]
fn test_valid_config() {
    let path = parser_fixture("valid");
    let config = parse_config(&path).expect("Failed to parse valid config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    // valid config should have no errors or warnings
    let errors_and_warnings: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e.severity, Severity::Error | Severity::Warning))
        .collect();

    assert!(
        errors_and_warnings.is_empty(),
        "Expected no errors or warnings, got: {:?}",
        errors_and_warnings
    );
}

#[test]
fn test_server_tokens_enabled() {
    let path = rule_fixture("security", "server_tokens_enabled");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    // Should have server_tokens warning
    let server_tokens_warning = errors
        .iter()
        .find(|e| e.rule == "server-tokens-enabled");

    assert!(
        server_tokens_warning.is_some(),
        "Expected server-tokens-enabled warning"
    );
    assert_eq!(server_tokens_warning.unwrap().severity, Severity::Warning);
}

#[test]
fn test_multiple_issues_config() {
    let path = misc_fixture("multiple_issues");
    let config = parse_config(&path).expect("Failed to parse multiple_issues config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    // Should have multiple server_tokens warnings
    let server_tokens_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "server-tokens-enabled")
        .collect();

    assert_eq!(
        server_tokens_warnings.len(),
        2,
        "Expected 2 server-tokens-enabled warnings, got {}",
        server_tokens_warnings.len()
    );
}

#[test]
fn test_minimal_config() {
    let path = parser_fixture("minimal");
    let config = parse_config(&path).expect("Failed to parse minimal config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    // Minimal config should have info-level suggestions
    let gzip_info = errors.iter().find(|e| e.rule == "gzip-not-enabled");
    let error_log_info = errors.iter().find(|e| e.rule == "missing-error-log");

    assert!(gzip_info.is_some(), "Expected gzip-not-enabled info");
    assert!(error_log_info.is_some(), "Expected missing-error-log info");

    // Should have no errors
    let error_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Error)
        .count();
    assert_eq!(error_count, 0, "Expected no errors");
}

#[test]
fn test_with_ssl_config() {
    let path = parser_fixture("with_ssl");
    let config = parse_config(&path).expect("Failed to parse with_ssl config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    // SSL config should have no errors or warnings
    let errors_and_warnings: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e.severity, Severity::Error | Severity::Warning))
        .collect();

    assert!(
        errors_and_warnings.is_empty(),
        "Expected no errors or warnings, got: {:?}",
        errors_and_warnings
    );
}

#[test]
fn test_with_include_config() {
    let path = parser_fixture("with_include");
    let config = parse_config(&path).expect("Failed to parse with_include config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    // Main config has gzip and error_log, so no info messages for those
    // But includes are not resolved, so we just check it parses
    let error_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Error)
        .count();
    assert_eq!(error_count, 0, "Expected no errors");
}

#[test]
fn test_with_nested_include_config() {
    let path = parser_fixture("with_nested_include");
    let config = parse_config(&path).expect("Failed to parse with_nested_include config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    // Just verify it parses without errors
    let error_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Error)
        .count();
    assert_eq!(error_count, 0, "Expected no errors");
}

#[test]
fn test_error_locations() {
    let path = rule_fixture("security", "server_tokens_enabled");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let server_tokens_warning = errors
        .iter()
        .find(|e| e.rule == "server-tokens-enabled")
        .expect("Expected server-tokens-enabled warning");

    // Check that line number is reported
    assert!(
        server_tokens_warning.line.is_some(),
        "Expected line number to be reported"
    );
    assert_eq!(
        server_tokens_warning.line.unwrap(),
        6,
        "Expected warning on line 6"
    );
}

#[test]
fn test_severity_counts() {
    let path = misc_fixture("multiple_issues");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let error_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Error)
        .count();
    let warning_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Warning)
        .count();
    let info_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Info)
        .count();

    assert_eq!(error_count, 0, "Expected 0 errors");
    assert_eq!(warning_count, 2, "Expected 2 warnings");
    assert_eq!(info_count, 2, "Expected 2 infos");
}

#[test]
fn test_inconsistent_indentation() {
    let path = rule_fixture("style", "inconsistent_indentation");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    // Should have indentation warnings
    let indentation_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "inconsistent-indentation")
        .collect();

    assert!(
        !indentation_warnings.is_empty(),
        "Expected inconsistent-indentation warnings"
    );

    // All indentation issues should be warnings
    for warning in &indentation_warnings {
        assert_eq!(warning.severity, Severity::Warning);
    }
}

#[test]
fn test_unmatched_braces() {
    let path = rule_fixture("syntax", "unmatched_braces");

    // Pre-parse checks should detect unmatched braces
    let errors = pre_parse_checks(&path);

    // Should have unmatched braces errors
    let brace_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "unmatched-braces")
        .collect();

    assert!(
        !brace_errors.is_empty(),
        "Expected unmatched-braces errors"
    );

    // All brace issues should be errors (not warnings)
    for error in &brace_errors {
        assert_eq!(error.severity, Severity::Error);
    }

    // Parsing should fail for this file
    assert!(
        parse_config(&path).is_err(),
        "Expected parse error for unmatched braces"
    );
}

#[test]
fn test_missing_semicolon() {
    let path = rule_fixture("syntax", "missing_semicolon");

    // Pre-parse checks should detect missing semicolons
    let errors = pre_parse_checks(&path);

    // Should have missing semicolon errors
    let semicolon_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "missing-semicolon")
        .collect();

    assert!(
        !semicolon_errors.is_empty(),
        "Expected missing-semicolon errors"
    );

    // All semicolon issues should be errors
    for error in &semicolon_errors {
        assert_eq!(error.severity, Severity::Error);
    }
}

#[test]
fn test_extension_module_directives() {
    // Test that extension module directives can be parsed
    use nginx_lint::parse_string;

    let config = parse_string(
        r#"
http {
    server {
        more_set_headers "Server: Custom";
        lua_code_cache on;
        gzip_types text/plain application/json;
        ssl_protocols TLSv1.2 TLSv1.3;
        autoindex off;
    }
}
"#,
    )
    .expect("Failed to parse extension module directives");

    // Verify all directives were parsed
    let directives: Vec<_> = config.all_directives().collect();
    let names: Vec<&str> = directives.iter().map(|d| d.name.as_str()).collect();

    assert!(names.contains(&"more_set_headers"));
    assert!(names.contains(&"lua_code_cache"));
    assert!(names.contains(&"gzip_types"));
    assert!(names.contains(&"ssl_protocols"));
    assert!(names.contains(&"autoindex"));
}

#[test]
fn test_deprecated_ssl_protocol_detection() {
    use nginx_lint::parse_string;

    let config = parse_string(
        r#"
server {
    ssl_protocols SSLv3 TLSv1 TLSv1.2;
}
"#,
    )
    .expect("Failed to parse ssl_protocols");

    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    // Should detect deprecated protocols
    let ssl_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "deprecated-ssl-protocol")
        .collect();

    assert_eq!(ssl_warnings.len(), 2, "Expected 2 deprecated protocol warnings (SSLv3 and TLSv1)");
}

#[test]
fn test_autoindex_enabled_detection() {
    use nginx_lint::parse_string;

    let config = parse_string(
        r#"
server {
    location /files {
        autoindex on;
    }
}
"#,
    )
    .expect("Failed to parse autoindex");

    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    // Should detect autoindex enabled
    let autoindex_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "autoindex-enabled")
        .collect();

    assert_eq!(autoindex_warnings.len(), 1, "Expected 1 autoindex warning");
}

#[test]
fn test_generated_fixtures_parse_without_errors() {
    use std::fs;

    let test_generated_dir = fixtures_base().join("test_generated");

    // Skip if directory doesn't exist
    if !test_generated_dir.exists() {
        return;
    }

    let entries = fs::read_dir(&test_generated_dir).expect("Failed to read test_generated directory");

    let mut tested_count = 0;
    for entry in entries {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();

        // Only test .conf files
        if path.extension().is_some_and(|ext| ext == "conf") {
            // First run pre-parse checks
            let pre_errors = pre_parse_checks(&path);
            let pre_errors_critical: Vec<_> = pre_errors
                .iter()
                .filter(|e| e.severity == Severity::Error)
                .collect();

            assert!(
                pre_errors_critical.is_empty(),
                "Pre-parse errors in {}: {:?}",
                path.display(),
                pre_errors_critical
            );

            // Then parse and lint
            let config = parse_config(&path).unwrap_or_else(|e| {
                panic!("Failed to parse {}: {}", path.display(), e)
            });

            let linter = Linter::with_default_rules();
            let errors = linter.lint(&config, &path);

            // Should have no errors (warnings and info are OK)
            let error_count = errors
                .iter()
                .filter(|e| e.severity == Severity::Error)
                .count();

            assert_eq!(
                error_count, 0,
                "Expected no errors in {}, got: {:?}",
                path.display(),
                errors.iter().filter(|e| e.severity == Severity::Error).collect::<Vec<_>>()
            );

            tested_count += 1;
        }
    }

    // Ensure we actually tested some files
    assert!(
        tested_count > 0,
        "No .conf files found in test_generated directory"
    );
}
