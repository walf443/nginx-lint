use nginx_lint::{parse_config, Linter, Severity};
use std::path::PathBuf;

fn fixtures_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
        .join("nginx.conf")
}

#[test]
fn test_valid_config() {
    let path = fixtures_path("valid");
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
fn test_warnings_config() {
    let path = fixtures_path("warnings");
    let config = parse_config(&path).expect("Failed to parse warnings config");
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
    let path = fixtures_path("multiple_issues");
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
    let path = fixtures_path("minimal");
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
    let path = fixtures_path("with_ssl");
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
    let path = fixtures_path("with_include");
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
    let path = fixtures_path("with_nested_include");
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
    let path = fixtures_path("warnings");
    let config = parse_config(&path).expect("Failed to parse warnings config");
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
    let path = fixtures_path("multiple_issues");
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
fn test_bad_indentation_config() {
    let path = fixtures_path("bad_indentation");
    let config = parse_config(&path).expect("Failed to parse bad_indentation config");
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
