use nginx_lint::{apply_fixes, parse_config, pre_parse_checks, Linter, Severity};
use std::fs;
use std::path::PathBuf;
use tempfile::NamedTempFile;

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

/// Find the first case directory for a rule (sorted alphabetically)
fn find_first_case(category: &str, rule: &str) -> Option<String> {
    let rule_dir = fixtures_base().join("rules").join(category).join(rule);
    let mut cases: Vec<_> = fs::read_dir(&rule_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect();
    cases.sort();
    cases.into_iter().next()
}

fn rule_error_fixture(category: &str, rule: &str) -> PathBuf {
    let case = find_first_case(category, rule)
        .unwrap_or_else(|| panic!("No case directory found for {}/{}", category, rule));
    rule_case_error_fixture(category, rule, &case)
}

fn rule_expected_fixture(category: &str, rule: &str) -> PathBuf {
    let case = find_first_case(category, rule)
        .unwrap_or_else(|| panic!("No case directory found for {}/{}", category, rule));
    rule_case_expected_fixture(category, rule, &case)
}

fn rule_case_error_fixture(category: &str, rule: &str, case: &str) -> PathBuf {
    fixtures_base()
        .join("rules")
        .join(category)
        .join(rule)
        .join(case)
        .join("error")
        .join("nginx.conf")
}

fn rule_case_expected_fixture(category: &str, rule: &str, case: &str) -> PathBuf {
    fixtures_base()
        .join("rules")
        .join(category)
        .join(rule)
        .join(case)
        .join("expected")
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
fn test_server_tokens_enabled_error() {
    let path = rule_error_fixture("security", "server_tokens_enabled");
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
fn test_server_tokens_enabled_expected() {
    let path = rule_expected_fixture("security", "server_tokens_enabled");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    // Should NOT have server_tokens warning
    let server_tokens_warning = errors
        .iter()
        .find(|e| e.rule == "server-tokens-enabled");

    assert!(
        server_tokens_warning.is_none(),
        "Expected no server-tokens-enabled warning, got: {:?}",
        server_tokens_warning
    );
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
    let path = rule_error_fixture("security", "server_tokens_enabled");
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
        5,
        "Expected warning on line 5"
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
fn test_inconsistent_indentation_error() {
    let path = rule_error_fixture("style", "inconsistent_indentation");
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
fn test_inconsistent_indentation_expected() {
    let path = rule_expected_fixture("style", "inconsistent_indentation");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    // Should NOT have indentation warnings
    let indentation_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "inconsistent-indentation")
        .collect();

    assert!(
        indentation_warnings.is_empty(),
        "Expected no inconsistent-indentation warnings, got: {:?}",
        indentation_warnings
    );
}

#[test]
fn test_unmatched_braces_error() {
    let path = rule_error_fixture("syntax", "unmatched_braces");

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
fn test_unmatched_braces_expected() {
    let path = rule_expected_fixture("syntax", "unmatched_braces");

    // Pre-parse checks should NOT detect unmatched braces
    let errors = pre_parse_checks(&path);

    let brace_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "unmatched-braces")
        .collect();

    assert!(
        brace_errors.is_empty(),
        "Expected no unmatched-braces errors, got: {:?}",
        brace_errors
    );

    // Parsing should succeed
    assert!(
        parse_config(&path).is_ok(),
        "Expected successful parse"
    );
}

#[test]
fn test_missing_semicolon_error() {
    let path = rule_error_fixture("syntax", "missing_semicolon");

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
fn test_missing_semicolon_expected() {
    let path = rule_expected_fixture("syntax", "missing_semicolon");

    // Pre-parse checks should NOT detect missing semicolons
    let errors = pre_parse_checks(&path);

    let semicolon_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "missing-semicolon")
        .collect();

    assert!(
        semicolon_errors.is_empty(),
        "Expected no missing-semicolon errors, got: {:?}",
        semicolon_errors
    );

    // Parsing should succeed
    assert!(
        parse_config(&path).is_ok(),
        "Expected successful parse"
    );
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

// Best practices: gzip_not_enabled
#[test]
fn test_gzip_not_enabled_error() {
    let path = rule_error_fixture("best_practices", "gzip_not_enabled");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let gzip_info = errors.iter().find(|e| e.rule == "gzip-not-enabled");
    assert!(gzip_info.is_some(), "Expected gzip-not-enabled info");
    assert_eq!(gzip_info.unwrap().severity, Severity::Info);
}

#[test]
fn test_gzip_not_enabled_expected() {
    let path = rule_expected_fixture("best_practices", "gzip_not_enabled");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let gzip_info = errors.iter().find(|e| e.rule == "gzip-not-enabled");
    assert!(gzip_info.is_none(), "Expected no gzip-not-enabled info");
}

// Best practices: missing_error_log
#[test]
fn test_missing_error_log_error() {
    let path = rule_error_fixture("best_practices", "missing_error_log");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let error_log_info = errors.iter().find(|e| e.rule == "missing-error-log");
    assert!(error_log_info.is_some(), "Expected missing-error-log info");
    assert_eq!(error_log_info.unwrap().severity, Severity::Info);
}

#[test]
fn test_missing_error_log_expected() {
    let path = rule_expected_fixture("best_practices", "missing_error_log");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let error_log_info = errors.iter().find(|e| e.rule == "missing-error-log");
    assert!(error_log_info.is_none(), "Expected no missing-error-log info");
}

// Security: autoindex_enabled
#[test]
fn test_autoindex_enabled_error() {
    let path = rule_error_fixture("security", "autoindex_enabled");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let autoindex_warning = errors.iter().find(|e| e.rule == "autoindex-enabled");
    assert!(autoindex_warning.is_some(), "Expected autoindex-enabled warning");
    assert_eq!(autoindex_warning.unwrap().severity, Severity::Warning);
}

#[test]
fn test_autoindex_enabled_expected() {
    let path = rule_expected_fixture("security", "autoindex_enabled");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let autoindex_warning = errors.iter().find(|e| e.rule == "autoindex-enabled");
    assert!(autoindex_warning.is_none(), "Expected no autoindex-enabled warning");
}

// Security: deprecated_ssl_protocol
#[test]
fn test_deprecated_ssl_protocol_error() {
    let path = rule_error_fixture("security", "deprecated_ssl_protocol");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let ssl_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "deprecated-ssl-protocol")
        .collect();

    assert!(
        !ssl_warnings.is_empty(),
        "Expected deprecated-ssl-protocol warnings"
    );
}

#[test]
fn test_deprecated_ssl_protocol_expected() {
    let path = rule_expected_fixture("security", "deprecated_ssl_protocol");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let ssl_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "deprecated-ssl-protocol")
        .collect();

    assert!(
        ssl_warnings.is_empty(),
        "Expected no deprecated-ssl-protocol warnings, got: {:?}",
        ssl_warnings
    );
}

// Syntax: duplicate_directive
#[test]
fn test_duplicate_directive_error() {
    let path = rule_error_fixture("syntax", "duplicate_directive");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let duplicate_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "duplicate-directive")
        .collect();

    assert!(
        !duplicate_warnings.is_empty(),
        "Expected duplicate-directive warnings"
    );
}

#[test]
fn test_duplicate_directive_expected() {
    let path = rule_expected_fixture("syntax", "duplicate_directive");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let duplicate_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "duplicate-directive")
        .collect();

    assert!(
        duplicate_warnings.is_empty(),
        "Expected no duplicate-directive warnings, got: {:?}",
        duplicate_warnings
    );
}

// Syntax: unclosed_quote
#[test]
fn test_unclosed_quote_error() {
    use nginx_lint::parse_string;

    let path = rule_error_fixture("syntax", "unclosed_quote");
    let linter = Linter::with_default_rules();

    // Use a minimal config since unclosed_quote reads from file directly
    let config = parse_string("").unwrap();
    let errors = linter.lint(&config, &path);

    let quote_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "unclosed-quote")
        .collect();

    assert!(
        !quote_errors.is_empty(),
        "Expected unclosed-quote errors"
    );
}

#[test]
fn test_unclosed_quote_expected() {
    let path = rule_expected_fixture("syntax", "unclosed_quote");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, &path);

    let quote_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "unclosed-quote")
        .collect();

    assert!(
        quote_errors.is_empty(),
        "Expected no unclosed-quote errors, got: {:?}",
        quote_errors
    );
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

// ============================================================================
// Fix tests - verify that applying fixes to error produces expected output
// ============================================================================

/// Helper function to test that applying fixes to an error fixture produces the expected fixture
fn test_fix_produces_expected(category: &str, rule: &str) {
    test_fix_case_produces_expected(category, rule, "001_basic");
}

fn test_fix_case_produces_expected(category: &str, rule: &str, case: &str) {
    use std::io::Write;

    let error_path = rule_case_error_fixture(category, rule, case);
    let expected_path = rule_case_expected_fixture(category, rule, case);

    // Read error content
    let error_content = fs::read_to_string(&error_path)
        .unwrap_or_else(|e| panic!("Failed to read error fixture for {}/{}/{}: {}", category, rule, case, e));

    // Create a temp file with error content
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", error_content).expect("Failed to write temp file");
    let temp_path = temp_file.path();

    // Parse and get errors with fixes
    let config = parse_config(temp_path)
        .unwrap_or_else(|e| panic!("Failed to parse error fixture for {}/{}/{}: {}", category, rule, case, e));
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, temp_path);

    // Apply fixes
    let fix_count = apply_fixes(temp_path, &errors)
        .unwrap_or_else(|e| panic!("Failed to apply fixes for {}/{}/{}: {}", category, rule, case, e));

    assert!(fix_count > 0, "Expected at least one fix to be applied for {}/{}/{}", category, rule, case);

    // Read the fixed content
    let fixed_content = fs::read_to_string(temp_path)
        .unwrap_or_else(|e| panic!("Failed to read fixed temp file for {}/{}/{}: {}", category, rule, case, e));

    // Read expected content
    let expected_content = fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("Failed to read expected fixture for {}/{}/{}: {}", category, rule, case, e));

    // Compare
    assert_eq!(
        fixed_content.trim(),
        expected_content.trim(),
        "Fixed content for {}/{}/{} does not match expected.\n\nFixed:\n{}\n\nExpected:\n{}",
        category,
        rule,
        case,
        fixed_content,
        expected_content
    );
}

/// Helper function for rules that read from file directly (can't parse normally)
fn test_fix_produces_expected_with_dummy_config(category: &str, rule: &str) {
    test_fix_case_produces_expected_with_dummy_config(category, rule, "001_basic");
}

fn test_fix_case_produces_expected_with_dummy_config(category: &str, rule: &str, case: &str) {
    use nginx_lint::parse_string;
    use std::io::Write;

    let error_path = rule_case_error_fixture(category, rule, case);
    let expected_path = rule_case_expected_fixture(category, rule, case);

    // Read error content
    let error_content = fs::read_to_string(&error_path)
        .unwrap_or_else(|e| panic!("Failed to read error fixture for {}/{}/{}: {}", category, rule, case, e));

    // Create a temp file with error content
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", error_content).expect("Failed to write temp file");
    let temp_path = temp_file.path();

    // Use dummy config since the rule reads from file directly
    let config = parse_string("").unwrap();
    let linter = Linter::with_default_rules();
    let errors = linter.lint(&config, temp_path);

    // Apply fixes
    let fix_count = apply_fixes(temp_path, &errors)
        .unwrap_or_else(|e| panic!("Failed to apply fixes for {}/{}/{}: {}", category, rule, case, e));

    assert!(fix_count > 0, "Expected at least one fix to be applied for {}/{}/{}", category, rule, case);

    // Read the fixed content
    let fixed_content = fs::read_to_string(temp_path)
        .unwrap_or_else(|e| panic!("Failed to read fixed temp file for {}/{}/{}: {}", category, rule, case, e));

    // Read expected content
    let expected_content = fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("Failed to read expected fixture for {}/{}/{}: {}", category, rule, case, e));

    // Compare
    assert_eq!(
        fixed_content.trim(),
        expected_content.trim(),
        "Fixed content for {}/{}/{} does not match expected.\n\nFixed:\n{}\n\nExpected:\n{}",
        category,
        rule,
        case,
        fixed_content,
        expected_content
    );
}

#[test]
fn test_fix_server_tokens_enabled() {
    test_fix_produces_expected("security", "server_tokens_enabled");
}

#[test]
fn test_fix_autoindex_enabled() {
    test_fix_produces_expected("security", "autoindex_enabled");
}

#[test]
fn test_fix_duplicate_directive() {
    test_fix_produces_expected("syntax", "duplicate_directive");
}

#[test]
fn test_fix_missing_semicolon() {
    test_fix_produces_expected_with_dummy_config("syntax", "missing_semicolon");
}

#[test]
fn test_fix_inconsistent_indentation() {
    test_fix_produces_expected_with_dummy_config("style", "inconsistent_indentation");
}

#[test]
fn test_fix_unmatched_braces() {
    test_fix_produces_expected_with_dummy_config("syntax", "unmatched_braces");
}

// ============================================================================
// Automatic fixture discovery tests
// ============================================================================

/// Get the rule name from a directory name (e.g., "server_tokens_enabled" -> "server-tokens-enabled")
fn dir_name_to_rule_name(dir_name: &str) -> String {
    dir_name.replace('_', "-")
}

/// Automatically discover and test all rule fixtures
/// This test iterates over all fixtures in tests/fixtures/rules/ and runs appropriate tests
#[test]
fn test_all_rule_fixtures() {
    use nginx_lint::parse_string;
    use std::io::Write;

    let rules_dir = fixtures_base().join("rules");

    // Iterate over categories (security, syntax, style, best_practices)
    for category_entry in fs::read_dir(&rules_dir).expect("Failed to read rules directory") {
        let category_entry = category_entry.expect("Failed to read category entry");
        let category_path = category_entry.path();
        if !category_path.is_dir() {
            continue;
        }
        let category = category_path.file_name().unwrap().to_str().unwrap();

        // Iterate over rules in this category
        for rule_entry in fs::read_dir(&category_path).expect("Failed to read category directory") {
            let rule_entry = rule_entry.expect("Failed to read rule entry");
            let rule_path = rule_entry.path();
            if !rule_path.is_dir() {
                continue;
            }
            let rule_dir_name = rule_path.file_name().unwrap().to_str().unwrap();
            let rule_name = dir_name_to_rule_name(rule_dir_name);

            // Iterate over test cases for this rule
            for case_entry in fs::read_dir(&rule_path).expect("Failed to read rule directory") {
                let case_entry = case_entry.expect("Failed to read case entry");
                let case_path = case_entry.path();
                if !case_path.is_dir() {
                    continue;
                }
                let case = case_path.file_name().unwrap().to_str().unwrap();

                let error_path = case_path.join("error").join("nginx.conf");
                let expected_path = case_path.join("expected").join("nginx.conf");

                // Test error fixture: should detect errors
                if error_path.exists() {
                    // Try to parse - some syntax error fixtures can't be parsed
                    let can_parse = parse_config(&error_path).is_ok();

                    if can_parse {
                        let config = parse_config(&error_path).unwrap();
                        let linter = Linter::with_default_rules();
                        let errors = linter.lint(&config, &error_path);

                        let rule_errors: Vec<_> = errors
                            .iter()
                            .filter(|e| e.rule == rule_name)
                            .collect();

                        assert!(
                            !rule_errors.is_empty(),
                            "Expected {} errors in {}/{}/{}/error/nginx.conf, got none",
                            rule_name, category, rule_dir_name, case
                        );
                    } else {
                        // For unparseable files, use pre_parse_checks
                        let errors = pre_parse_checks(&error_path);
                        let rule_errors: Vec<_> = errors
                            .iter()
                            .filter(|e| e.rule == rule_name)
                            .collect();

                        assert!(
                            !rule_errors.is_empty(),
                            "Expected {} errors in {}/{}/{}/error/nginx.conf (pre-parse), got none",
                            rule_name, category, rule_dir_name, case
                        );
                    }
                }

                // Test expected fixture: should have no errors for this rule
                if expected_path.exists() {
                    let can_parse = parse_config(&expected_path).is_ok();

                    if can_parse {
                        let config = parse_config(&expected_path).unwrap();
                        let linter = Linter::with_default_rules();
                        let errors = linter.lint(&config, &expected_path);

                        let rule_errors: Vec<_> = errors
                            .iter()
                            .filter(|e| e.rule == rule_name)
                            .collect();

                        assert!(
                            rule_errors.is_empty(),
                            "Expected no {} errors in {}/{}/{}/expected/nginx.conf, got: {:?}",
                            rule_name, category, rule_dir_name, case, rule_errors
                        );
                    } else {
                        // For unparseable expected files, use pre_parse_checks
                        let errors = pre_parse_checks(&expected_path);
                        let rule_errors: Vec<_> = errors
                            .iter()
                            .filter(|e| e.rule == rule_name)
                            .collect();

                        assert!(
                            rule_errors.is_empty(),
                            "Expected no {} errors in {}/{}/{}/expected/nginx.conf (pre-parse), got: {:?}",
                            rule_name, category, rule_dir_name, case, rule_errors
                        );
                    }
                }

                // Test fix: if both error and expected exist, verify fix produces expected
                // Only test if this rule has fixes (filter to just this rule's errors)
                if error_path.exists() && expected_path.exists() {
                    let error_content = fs::read_to_string(&error_path)
                        .expect("Failed to read error fixture");

                    // Create temp file with error content
                    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
                    write!(temp_file, "{}", error_content).expect("Failed to write temp file");
                    let temp_path = temp_file.path();

                    // Get all errors (try parsing first, fall back to dummy config)
                    let all_errors = if let Ok(config) = parse_config(temp_path) {
                        let linter = Linter::with_default_rules();
                        linter.lint(&config, temp_path)
                    } else {
                        let config = parse_string("").unwrap();
                        let linter = Linter::with_default_rules();
                        linter.lint(&config, temp_path)
                    };

                    // Filter to only this rule's errors with fixes
                    let rule_errors_with_fixes: Vec<_> = all_errors
                        .iter()
                        .filter(|e| e.rule == rule_name && e.fix.is_some())
                        .cloned()
                        .collect();

                    // Skip if this rule has no fixes
                    if rule_errors_with_fixes.is_empty() {
                        continue;
                    }

                    // Apply only this rule's fixes
                    let fix_count = apply_fixes(temp_path, &rule_errors_with_fixes)
                        .expect("Failed to apply fixes");

                    if fix_count == 0 {
                        continue; // Skip if no fixes were applied
                    }

                    // Read fixed content and expected content
                    let fixed_content = fs::read_to_string(temp_path)
                        .expect("Failed to read fixed file");
                    let expected_content = fs::read_to_string(&expected_path)
                        .expect("Failed to read expected file");

                    assert_eq!(
                        fixed_content.trim(),
                        expected_content.trim(),
                        "Fix for {}/{}/{} did not produce expected output.\n\nFixed:\n{}\n\nExpected:\n{}",
                        category, rule_dir_name, case, fixed_content, expected_content
                    );
                }
            }
        }
    }
}
