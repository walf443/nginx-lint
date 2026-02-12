use nginx_lint::{
    LintConfig, Linter, Severity, apply_fixes, parse_config, parse_string, pre_parse_checks,
};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tempfile::NamedTempFile;

/// Global cached linter with default rules (thread-safe)
fn get_default_linter() -> &'static Linter {
    static LINTER: OnceLock<Linter> = OnceLock::new();
    LINTER.get_or_init(Linter::with_default_rules)
}

/// Global cached linter with all rules enabled (thread-safe)
fn get_all_rules_linter() -> &'static Linter {
    static LINTER: OnceLock<Linter> = OnceLock::new();
    LINTER.get_or_init(|| {
        let config_toml = r#"
[rules.gzip-not-enabled]
enabled = true

[rules.missing-error-log]
enabled = true
"#;
        let config: LintConfig = LintConfig::parse(config_toml).unwrap();
        Linter::with_config(Some(&config))
    })
}

fn fixtures_base() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn parser_fixture(name: &str) -> PathBuf {
    fixtures_base().join("parser").join(name).join("nginx.conf")
}

fn misc_fixture(name: &str) -> PathBuf {
    fixtures_base().join(name).join("nginx.conf")
}

// ============================================================================
// Parser tests - test parsing of various config structures
// ============================================================================

#[test]
fn test_valid_config() {
    let path = parser_fixture("valid");
    let config = parse_config(&path).expect("Failed to parse valid config");
    let linter = get_default_linter();
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
fn test_minimal_config() {
    let path = parser_fixture("minimal");
    let config = parse_config(&path).expect("Failed to parse minimal config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, &path);

    // Should have no errors (gzip-not-enabled and missing-error-log
    // are disabled by default)
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
    let linter = get_default_linter();
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
    let linter = get_default_linter();
    let errors = linter.lint(&config, &path);

    // Main config has gzip and error_log, so no warnings for those
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
    let linter = get_default_linter();
    let errors = linter.lint(&config, &path);

    // Just verify it parses without errors
    let error_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Error)
        .count();
    assert_eq!(error_count, 0, "Expected no errors");
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

// ============================================================================
// Specific behavior tests - tests that check specific details
// ============================================================================

#[test]
fn test_multiple_issues_config() {
    let path = misc_fixture("multiple_issues");
    let config = parse_config(&path).expect("Failed to parse multiple_issues config");
    let linter = get_default_linter();
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
fn test_error_locations() {
    use nginx_lint::parse_string;

    // Test that error locations are correctly reported
    let config = parse_string(
        r#"# Test config
worker_processes auto;

http {
  server_tokens on;

  server {
    listen 80;
  }
}
"#,
    )
    .unwrap();

    let linter = get_default_linter();
    let errors = linter.lint(&config, Path::new("test.conf"));

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

// ============================================================================
// server_tokens include context tests
// ============================================================================

#[test]
fn test_server_tokens_warns_for_http_block_without_directive() {
    // File with http block but no server_tokens should warn
    use nginx_lint::parse_string;

    let config = parse_string(
        r#"
http {
    server {
        listen 80;
    }
}
"#,
    )
    .unwrap();

    let linter = get_default_linter();
    let errors = linter.lint(&config, Path::new("test.conf"));

    let server_tokens_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "server-tokens-enabled")
        .collect();

    assert_eq!(
        server_tokens_warnings.len(),
        1,
        "Expected 1 server-tokens-enabled warning for http block without server_tokens"
    );
    assert!(
        server_tokens_warnings[0]
            .message
            .contains("defaults to 'on'"),
        "Expected 'defaults to on' message"
    );
}

#[test]
fn test_server_tokens_no_warning_for_included_file() {
    // File included from http context (via include_context) should NOT warn
    use nginx_lint::parse_string;

    let mut config = parse_string(
        r#"
server {
    listen 80;
}
"#,
    )
    .unwrap();

    // Simulate being included from http context
    config.include_context = vec!["http".to_string()];

    let linter = get_default_linter();
    let errors = linter.lint(&config, Path::new("sites-available/example.conf"));

    let server_tokens_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "server-tokens-enabled")
        .collect();

    assert!(
        server_tokens_warnings.is_empty(),
        "Expected no server-tokens-enabled warning for included file, got: {:?}",
        server_tokens_warnings
    );
}

#[test]
fn test_server_tokens_warns_for_explicit_on_in_included_file() {
    // Explicit server_tokens on should always warn, even in included files
    use nginx_lint::parse_string;

    let mut config = parse_string(
        r#"
server {
    server_tokens on;
    listen 80;
}
"#,
    )
    .unwrap();

    // Simulate being included from http context
    config.include_context = vec!["http".to_string()];

    let linter = get_default_linter();
    let errors = linter.lint(&config, Path::new("sites-available/example.conf"));

    let server_tokens_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "server-tokens-enabled")
        .collect();

    assert_eq!(
        server_tokens_warnings.len(),
        1,
        "Expected 1 server-tokens-enabled warning for explicit 'on'"
    );
    assert!(
        server_tokens_warnings[0]
            .message
            .contains("should be 'off'"),
        "Expected 'should be off' message for explicit on"
    );
}

#[test]
fn test_server_tokens_no_warning_for_nested_include_context() {
    // File included from http > server context should NOT warn
    use nginx_lint::parse_string;

    let mut config = parse_string(
        r#"
location / {
    root /var/www;
}
"#,
    )
    .unwrap();

    // Simulate being included from http > server context
    config.include_context = vec!["http".to_string(), "server".to_string()];

    let linter = get_default_linter();
    let errors = linter.lint(&config, Path::new("snippets/location.conf"));

    let server_tokens_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "server-tokens-enabled")
        .collect();

    assert!(
        server_tokens_warnings.is_empty(),
        "Expected no server-tokens-enabled warning for nested include context, got: {:?}",
        server_tokens_warnings
    );
}

// ============================================================================
// Context comment tests
// ============================================================================

#[test]
fn test_context_comment_sets_include_context() {
    use nginx_lint::parse_context_comment;

    let content = "# nginx-lint:context http,server\nlocation / { root /var/www; }";
    let context = parse_context_comment(content);

    assert_eq!(
        context,
        Some(vec!["http".to_string(), "server".to_string()]),
        "Expected context to be parsed from comment"
    );
}

#[test]
fn test_context_comment_prevents_invalid_context_error() {
    use nginx_lint::collect_included_files;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Create a temp file with context comment
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "# nginx-lint:context http,server").unwrap();
    writeln!(file, "location / {{ root /var/www; }}").unwrap();
    file.flush().unwrap();

    // Collect files (this should pick up the context comment)
    let included_files = collect_included_files(file.path(), |path| {
        parse_config(path).map_err(|e| e.to_string())
    });

    assert_eq!(included_files.len(), 1);

    let config = included_files[0].config.as_ref().unwrap();
    assert_eq!(
        config.include_context,
        vec!["http".to_string(), "server".to_string()],
        "Expected include_context to be set from comment"
    );

    // Lint the file - should NOT have invalid-directive-context error
    let linter = get_default_linter();
    let errors = linter.lint(config, file.path());

    let context_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "invalid-directive-context")
        .collect();

    assert!(
        context_errors.is_empty(),
        "Expected no invalid-directive-context error with context comment, got: {:?}",
        context_errors
    );
}

#[test]
fn test_no_context_comment_causes_invalid_context_error() {
    use nginx_lint::collect_included_files;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Create a temp file WITHOUT context comment
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "location / {{ root /var/www; }}").unwrap();
    file.flush().unwrap();

    // Collect files
    let included_files = collect_included_files(file.path(), |path| {
        parse_config(path).map_err(|e| e.to_string())
    });

    assert_eq!(included_files.len(), 1);

    let config = included_files[0].config.as_ref().unwrap();

    // Lint the file - SHOULD have invalid-directive-context error
    let linter = get_default_linter();
    let errors = linter.lint(config, file.path());

    let context_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "invalid-directive-context")
        .collect();

    assert_eq!(
        context_errors.len(),
        1,
        "Expected invalid-directive-context error without context comment"
    );
}

#[test]
fn test_severity_counts() {
    let path = misc_fixture("multiple_issues");
    let config = parse_config(&path).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, &path);

    let error_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Error)
        .count();
    let warning_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Warning)
        .count();
    assert_eq!(error_count, 0, "Expected 0 errors");
    assert_eq!(warning_count, 5, "Expected 5 warnings");
    // Note: gzip-not-enabled and missing-error-log are disabled by default
    // Warnings: server-tokens-enabled x2, root-in-location x2, client-max-body-size-not-set x1
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

    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    // Should detect deprecated protocols
    let ssl_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "deprecated-ssl-protocol")
        .collect();

    assert_eq!(
        ssl_warnings.len(),
        2,
        "Expected 2 deprecated protocol warnings (SSLv3 and TLSv1)"
    );
}

#[test]
fn test_autoindex_enabled_detection() {
    use nginx_lint::parse_string;

    let config = parse_string(
        r#"
http {
    server {
        location /files {
            autoindex on;
        }
    }
}
"#,
    )
    .expect("Failed to parse autoindex");

    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    // Should detect autoindex enabled
    let autoindex_warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "autoindex-enabled")
        .collect();

    assert_eq!(autoindex_warnings.len(), 1, "Expected 1 autoindex warning");
}

// ============================================================================
// Automatic fixture discovery test
// This single test covers all rule fixtures (error detection, expected passing,
// and fix verification) by iterating over the fixtures/rules/ directory.
// ============================================================================

/// Get the rule name from a directory name (e.g., "server_tokens_enabled" -> "server-tokens-enabled")
fn dir_name_to_rule_name(dir_name: &str) -> String {
    dir_name.replace('_', "-")
}

/// Test case information for parallel execution
struct RuleTestCase {
    category: String,
    rule_dir_name: String,
    rule_name: String,
    case: String,
    error_path: PathBuf,
    expected_path: PathBuf,
}

/// Automatically discover and test all rule fixtures
/// This test iterates over all fixtures in tests/fixtures/rules/ and runs appropriate tests
#[test]
fn test_all_rule_fixtures() {
    use std::io::Write;

    let rules_dir = fixtures_base().join("rules");

    // Use cached linter with all rules enabled
    let linter = get_all_rules_linter();

    // Collect all test cases first
    let mut test_cases: Vec<RuleTestCase> = Vec::new();

    for category_entry in fs::read_dir(&rules_dir).expect("Failed to read rules directory") {
        let category_entry = category_entry.expect("Failed to read category entry");
        let category_path = category_entry.path();
        if !category_path.is_dir() {
            continue;
        }
        let category = category_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        for rule_entry in fs::read_dir(&category_path).expect("Failed to read category directory") {
            let rule_entry = rule_entry.expect("Failed to read rule entry");
            let rule_path = rule_entry.path();
            if !rule_path.is_dir() {
                continue;
            }
            let rule_dir_name = rule_path.file_name().unwrap().to_str().unwrap().to_string();
            let rule_name = dir_name_to_rule_name(&rule_dir_name);

            for case_entry in fs::read_dir(&rule_path).expect("Failed to read rule directory") {
                let case_entry = case_entry.expect("Failed to read case entry");
                let case_path = case_entry.path();
                if !case_path.is_dir() {
                    continue;
                }
                let case = case_path.file_name().unwrap().to_str().unwrap().to_string();

                test_cases.push(RuleTestCase {
                    category: category.clone(),
                    rule_dir_name: rule_dir_name.clone(),
                    rule_name: rule_name.clone(),
                    case,
                    error_path: case_path.join("error").join("nginx.conf"),
                    expected_path: case_path.join("expected").join("nginx.conf"),
                });
            }
        }
    }

    // Run all test cases in parallel, collecting failures
    let failures: Vec<String> = test_cases
        .par_iter()
        .flat_map(|tc| {
            let mut case_failures = Vec::new();

            // Parse error fixture once
            let error_config = if tc.error_path.exists() {
                parse_config(&tc.error_path).ok()
            } else {
                None
            };

            // Parse expected fixture once
            let expected_config = if tc.expected_path.exists() {
                parse_config(&tc.expected_path).ok()
            } else {
                None
            };

            // Test error fixture: should detect errors
            if tc.error_path.exists() {
                let mut errors = pre_parse_checks(&tc.error_path);

                if let Some(ref config) = error_config {
                    errors.extend(linter.lint(config, &tc.error_path));
                }

                let rule_errors: Vec<_> = errors
                    .iter()
                    .filter(|e| e.rule == tc.rule_name)
                    .collect();

                if rule_errors.is_empty() {
                    case_failures.push(format!(
                        "Expected {} errors in {}/{}/{}/error/nginx.conf, got none",
                        tc.rule_name, tc.category, tc.rule_dir_name, tc.case
                    ));
                }
            }

            // Test expected fixture: should have no errors for this rule
            if tc.expected_path.exists() {
                let mut errors = pre_parse_checks(&tc.expected_path);

                if let Some(ref config) = expected_config {
                    errors.extend(linter.lint(config, &tc.expected_path));
                }

                let rule_errors: Vec<_> = errors
                    .iter()
                    .filter(|e| e.rule == tc.rule_name)
                    .collect();

                if !rule_errors.is_empty() {
                    case_failures.push(format!(
                        "Expected no {} errors in {}/{}/{}/expected/nginx.conf, got: {:?}",
                        tc.rule_name, tc.category, tc.rule_dir_name, tc.case, rule_errors
                    ));
                }
            }

            // Test fix: if both error and expected exist, verify fix produces expected
            if tc.error_path.exists() && tc.expected_path.exists() && error_config.is_some() {
                if let Ok(error_content) = fs::read_to_string(&tc.error_path) {
                    if let Ok(mut temp_file) = NamedTempFile::new() {
                        if write!(temp_file, "{}", error_content).is_ok() {
                            let temp_path = temp_file.path();

                            let mut all_errors = pre_parse_checks(temp_path);
                            if let Ok(config) = parse_config(temp_path) {
                                all_errors.extend(linter.lint(&config, temp_path));
                            }

                            let rule_errors_with_fixes: Vec<_> = all_errors
                                .iter()
                                .filter(|e| e.rule == tc.rule_name && !e.fixes.is_empty())
                                .cloned()
                                .collect();

                            if !rule_errors_with_fixes.is_empty() {
                                if let Ok(fix_count) = apply_fixes(temp_path, &rule_errors_with_fixes) {
                                    if fix_count > 0 {
                                        if let (Ok(fixed_content), Ok(expected_content)) = (
                                            fs::read_to_string(temp_path),
                                            fs::read_to_string(&tc.expected_path),
                                        ) {
                                            if fixed_content.trim() != expected_content.trim() {
                                                case_failures.push(format!(
                                                    "Fix for {}/{}/{} did not produce expected output.\n\nFixed:\n{}\n\nExpected:\n{}",
                                                    tc.category, tc.rule_dir_name, tc.case, fixed_content, expected_content
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            case_failures
        })
        .collect();

    // Report all failures at once
    assert!(
        failures.is_empty(),
        "Test failures:\n{}",
        failures.join("\n\n")
    );
}

// ============================================================================
// Config validation tests
// ============================================================================

#[test]
fn test_config_validate_valid() {
    use std::io::Write;

    let config_content = r#"
[color]
ui = "auto"
error = "red"
warning = "yellow"

[rules.weak-ssl-ciphers]
enabled = true
weak_ciphers = ["RC4"]
required_exclusions = ["!RC4"]

[rules.indent]
enabled = true
indent_size = 2

[rules.deprecated-ssl-protocol]
enabled = false
allowed_protocols = ["TLSv1.2", "TLSv1.3"]
"#;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", config_content).unwrap();

    let errors = nginx_lint::LintConfig::validate_file(file.path()).unwrap();
    assert!(
        errors.is_empty(),
        "Expected no validation errors, got: {:?}",
        errors
    );
}

#[test]
fn test_config_validate_unknown_top_level_section() {
    use std::io::Write;

    let config_content = r#"
[color]
ui = "auto"

[unknown_section]
foo = "bar"
"#;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", config_content).unwrap();

    let errors = nginx_lint::LintConfig::validate_file(file.path()).unwrap();
    assert_eq!(errors.len(), 1);

    let error = &errors[0];
    let error_str = error.to_string();
    assert!(error_str.contains("unknown field 'unknown_section'"));
    assert!(error_str.contains("line 5"));
}

#[test]
fn test_config_validate_unknown_color_option() {
    use std::io::Write;

    let config_content = r#"
[color]
ui = "auto"
unknown_option = "value"
"#;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", config_content).unwrap();

    let errors = nginx_lint::LintConfig::validate_file(file.path()).unwrap();
    assert_eq!(errors.len(), 1);

    let error = &errors[0];
    let error_str = error.to_string();
    assert!(error_str.contains("unknown field 'color.unknown_option'"));
    assert!(error_str.contains("line 4"));
}

#[test]
fn test_config_validate_unknown_rule() {
    use std::io::Write;

    let config_content = r#"
[rules.nonexistent-rule]
enabled = true
"#;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", config_content).unwrap();

    let errors = nginx_lint::LintConfig::validate_file(file.path()).unwrap();
    assert_eq!(errors.len(), 1);

    let error = &errors[0];
    let error_str = error.to_string();
    assert!(error_str.contains("unknown rule 'nonexistent-rule'"));
    assert!(error_str.contains("line 2"));
}

#[test]
fn test_config_validate_unknown_rule_option() {
    use std::io::Write;

    let config_content = r#"
[rules.weak-ssl-ciphers]
enabled = true
unknown_option = "value"
"#;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", config_content).unwrap();

    let errors = nginx_lint::LintConfig::validate_file(file.path()).unwrap();
    assert_eq!(errors.len(), 1);

    let error = &errors[0];
    let error_str = error.to_string();
    assert!(error_str.contains("unknown option 'unknown_option' for rule 'weak-ssl-ciphers'"));
    assert!(error_str.contains("line 4"));
}

#[test]
fn test_config_validate_typo_suggestion() {
    use std::io::Write;

    let config_content = r#"
[rules.weak-ssl-ciphers]
enabled = true
weak_cipherz = ["RC4"]
"#;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", config_content).unwrap();

    let errors = nginx_lint::LintConfig::validate_file(file.path()).unwrap();
    assert_eq!(errors.len(), 1);

    let error = &errors[0];
    let error_str = error.to_string();
    assert!(error_str.contains("weak_cipherz"));
    assert!(error_str.contains("did you mean 'weak_ciphers'?"));
}

#[test]
fn test_config_validate_multiple_errors() {
    use std::io::Write;

    let config_content = r#"
[color]
ui = "auto"
bad_color = "red"

[rules.fake-rule]
enabled = true

[rules.weak-ssl-ciphers]
typo_option = "value"

[bad_section]
foo = "bar"
"#;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", config_content).unwrap();

    let errors = nginx_lint::LintConfig::validate_file(file.path()).unwrap();
    assert_eq!(errors.len(), 4);

    // Check that all error types are present
    let error_strs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
    assert!(error_strs.iter().any(|e| e.contains("bad_section")));
    assert!(error_strs.iter().any(|e| e.contains("bad_color")));
    assert!(error_strs.iter().any(|e| e.contains("fake-rule")));
    assert!(error_strs.iter().any(|e| e.contains("typo_option")));
}

#[test]
fn test_config_init_generates_valid_config() {
    use std::process::Command;

    // Create a temp directory for the test
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".nginx-lint.toml");

    // Run `nginx-lint config init -o <path>`
    let output = Command::new(env!("CARGO_BIN_EXE_nginx-lint"))
        .args(["config", "init", "-o", config_path.to_str().unwrap()])
        .output()
        .expect("Failed to run nginx-lint config init");

    assert!(
        output.status.success(),
        "config init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the file was created
    assert!(config_path.exists(), "Config file was not created");

    // Validate the generated config file
    let errors = nginx_lint::LintConfig::validate_file(&config_path).unwrap();
    assert!(
        errors.is_empty(),
        "Generated config has validation errors: {:?}",
        errors
    );

    // Also verify the config can be loaded
    let config = nginx_lint::LintConfig::from_file(&config_path);
    assert!(
        config.is_ok(),
        "Generated config failed to load: {:?}",
        config.err()
    );
}

// ============================================================================
// Ignore comment tests
// ============================================================================

#[test]
fn test_ignore_comment_suppresses_error() {
    use nginx_lint::IgnoreTracker;
    use nginx_lint::filter_errors;

    let content = r#"
http {
    server {
        # nginx-lint:ignore server-tokens-enabled 開発環境用
        server_tokens on;
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    // Build tracker and filter errors
    let (mut tracker, warnings) = IgnoreTracker::from_content(content);
    let result = filter_errors(errors, &mut tracker);

    // Verify no warnings from parsing the ignore comment
    assert!(warnings.is_empty(), "Unexpected warnings: {:?}", warnings);

    // Verify the error was ignored
    let server_tokens_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.rule == "server-tokens-enabled")
        .collect();
    assert!(
        server_tokens_errors.is_empty(),
        "Expected server-tokens-enabled error to be ignored, but got: {:?}",
        server_tokens_errors
    );
    assert_eq!(result.ignored_count, 1, "Expected 1 error to be ignored");
}

#[test]
fn test_ignore_comment_only_affects_next_line() {
    use nginx_lint::IgnoreTracker;
    use nginx_lint::filter_errors;

    let content = r#"
http {
    server {
        # nginx-lint:ignore server-tokens-enabled reason
        server_tokens on;
        server_tokens on;
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    // Build tracker and filter errors
    let (mut tracker, _) = IgnoreTracker::from_content(content);
    let result = filter_errors(errors, &mut tracker);

    // First server_tokens should be ignored, second should still be reported
    let server_tokens_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.rule == "server-tokens-enabled")
        .collect();
    assert_eq!(
        server_tokens_errors.len(),
        1,
        "Expected 1 server-tokens-enabled error (second occurrence), got: {:?}",
        server_tokens_errors
    );
    assert_eq!(result.ignored_count, 1, "Expected 1 error to be ignored");
}

#[test]
fn test_ignore_comment_missing_reason_warning() {
    use nginx_lint::IgnoreTracker;

    let content = r#"
# nginx-lint:ignore server-tokens-enabled
server_tokens on;
"#;

    let (_, warnings) = IgnoreTracker::from_content(content);

    assert_eq!(warnings.len(), 1, "Expected 1 warning");
    assert!(
        warnings[0].message.contains("requires a reason"),
        "Expected 'requires a reason' warning, got: {}",
        warnings[0].message
    );
}

#[test]
fn test_ignore_comment_missing_rule_name_warning() {
    use nginx_lint::IgnoreTracker;

    let content = r#"
# nginx-lint:ignore
server_tokens on;
"#;

    let (_, warnings) = IgnoreTracker::from_content(content);

    assert_eq!(warnings.len(), 1, "Expected 1 warning");
    assert!(
        warnings[0].message.contains("requires a rule name"),
        "Expected 'requires a rule name' warning, got: {}",
        warnings[0].message
    );
}

#[test]
fn test_ignore_comment_only_ignores_specified_rule() {
    use nginx_lint::IgnoreTracker;
    use nginx_lint::filter_errors;

    let content = r#"
http {
    server {
        # nginx-lint:ignore server-tokens-enabled reason
        server_tokens on;
        autoindex on;
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    // Build tracker and filter errors
    let (mut tracker, _) = IgnoreTracker::from_content(content);
    let result = filter_errors(errors, &mut tracker);

    // server-tokens-enabled should be ignored
    let server_tokens_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.rule == "server-tokens-enabled")
        .collect();
    assert!(
        server_tokens_errors.is_empty(),
        "Expected server-tokens-enabled to be ignored"
    );

    // autoindex-enabled should still be reported
    let autoindex_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.rule == "autoindex-enabled")
        .collect();
    assert_eq!(
        autoindex_errors.len(),
        1,
        "Expected autoindex-enabled error to still be reported"
    );
}

#[test]
fn test_lint_with_content_filters_ignored_errors() {
    let content = r#"
http {
    server {
        # nginx-lint:ignore server-tokens-enabled dev environment
        server_tokens on;
        autoindex on;
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let (errors, ignored_count) =
        linter.lint_with_content(&config, std::path::Path::new("test.conf"), content);

    // server-tokens-enabled should be ignored
    let server_tokens_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "server-tokens-enabled")
        .collect();
    assert!(
        server_tokens_errors.is_empty(),
        "Expected server-tokens-enabled to be ignored"
    );

    // autoindex-enabled should still be reported
    let autoindex_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "autoindex-enabled")
        .collect();
    assert_eq!(
        autoindex_errors.len(),
        1,
        "Expected autoindex-enabled error to still be reported"
    );

    assert_eq!(ignored_count, 1, "Expected 1 error to be ignored");
}

#[test]
fn test_inline_ignore_comment() {
    use nginx_lint::IgnoreTracker;
    use nginx_lint::filter_errors;

    let content = r#"
http {
    server {
        server_tokens on; # nginx-lint:ignore server-tokens-enabled dev environment
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    // Build tracker and filter errors
    let (mut tracker, warnings) = IgnoreTracker::from_content(content);
    let result = filter_errors(errors, &mut tracker);

    // Verify no warnings from parsing the ignore comment
    assert!(warnings.is_empty(), "Unexpected warnings: {:?}", warnings);

    // Verify the error was ignored
    let server_tokens_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.rule == "server-tokens-enabled")
        .collect();
    assert!(
        server_tokens_errors.is_empty(),
        "Expected server-tokens-enabled error to be ignored, but got: {:?}",
        server_tokens_errors
    );
    assert_eq!(result.ignored_count, 1, "Expected 1 error to be ignored");
}

#[test]
fn test_unused_inline_ignore_comment_fix() {
    use nginx_lint::IgnoreTracker;
    use nginx_lint::filter_errors;

    // Content with unused inline ignore comment (server_tokens off doesn't trigger the rule)
    let content = r#"
http {
    server {
        server_tokens off; # nginx-lint:ignore server-tokens-enabled reason
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    // Build tracker and filter errors
    let (mut tracker, _) = IgnoreTracker::from_content(content);
    let result = filter_errors(errors, &mut tracker);

    // Should have an unused warning with a fix
    assert_eq!(result.unused_warnings.len(), 1, "Expected 1 unused warning");
    assert!(
        result.unused_warnings[0]
            .message
            .contains("unused nginx-lint:ignore"),
        "Expected unused warning, got: {}",
        result.unused_warnings[0].message
    );

    // The fix should replace the line with just the directive (preserving indentation)
    let fix = result.unused_warnings[0]
        .fixes
        .first()
        .expect("Expected a fix");
    assert_eq!(fix.line, 4, "Fix should be on line 4");
    assert!(!fix.delete_line, "Should not delete entire line");
    assert_eq!(
        fix.new_text, "        server_tokens off;",
        "Fix should preserve indentation"
    );
}

// ============================================================================
// Invalid directive context tests
// ============================================================================

#[test]
fn test_invalid_directive_context_server_in_server() {
    let content = r#"
http {
    server {
        server {
            listen 80;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let context_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "invalid-directive-context")
        .collect();

    assert_eq!(
        context_errors.len(),
        1,
        "Expected 1 invalid-directive-context error, got: {:?}",
        context_errors
    );
    assert!(
        context_errors[0]
            .message
            .contains("'server' directive cannot be inside 'server'"),
        "Expected server in server error, got: {}",
        context_errors[0].message
    );
}

#[test]
fn test_invalid_directive_context_location_in_http() {
    let content = r#"
http {
    location / {
        root /var/www;
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let context_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "invalid-directive-context")
        .collect();

    assert_eq!(
        context_errors.len(),
        1,
        "Expected 1 invalid-directive-context error, got: {:?}",
        context_errors
    );
    assert!(
        context_errors[0]
            .message
            .contains("'location' directive cannot be inside 'http'"),
        "Expected location in http error, got: {}",
        context_errors[0].message
    );
}

#[test]
fn test_invalid_directive_context_http_not_at_root() {
    let content = r#"
http {
    server {
        http {
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let context_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "invalid-directive-context")
        .collect();

    assert!(
        context_errors.iter().any(|e| e
            .message
            .contains("'http' directive must be in main context")),
        "Expected http not at root error, got: {:?}",
        context_errors
    );
}

#[test]
fn test_invalid_directive_context_server_at_root() {
    let content = r#"
server {
    listen 80;
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let context_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "invalid-directive-context")
        .collect();

    assert_eq!(
        context_errors.len(),
        1,
        "Expected 1 invalid-directive-context error, got: {:?}",
        context_errors
    );
    assert!(
        context_errors[0]
            .message
            .contains("'server' directive must be inside one of:"),
        "Expected server at root error, got: {}",
        context_errors[0].message
    );
    assert!(
        context_errors[0].message.contains("not in main context"),
        "Expected 'not in main context' in error, got: {}",
        context_errors[0].message
    );
}

#[test]
fn test_valid_directive_context() {
    let content = r#"
events {
    worker_connections 1024;
}

http {
    upstream backend {
        server 127.0.0.1:8080;
    }

    server {
        listen 80;

        location / {
            root /var/www;

            if ($request_method = POST) {
                return 405;
            }
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let context_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "invalid-directive-context")
        .collect();

    assert!(
        context_errors.is_empty(),
        "Expected no invalid-directive-context errors, got: {:?}",
        context_errors
    );
}

#[test]
fn test_include_context_propagation() {
    // Simulate a config included from server context - location should be valid
    let mut config = parse_string(
        r#"
location / {
    root /var/www;
}
"#,
    )
    .expect("Failed to parse config");

    // Set include context as if this file was included from http { server { ... } }
    config.include_context = vec!["http".to_string(), "server".to_string()];

    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let context_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "invalid-directive-context")
        .collect();

    assert!(
        context_errors.is_empty(),
        "Expected no invalid-directive-context errors when included from server context, got: {:?}",
        context_errors
    );
}

#[test]
fn test_include_context_error() {
    // Simulate a config included from http context (not inside server) - location should error
    let mut config = parse_string(
        r#"
location / {
    root /var/www;
}
"#,
    )
    .expect("Failed to parse config");

    // Set include context as if this file was included from http { ... } (no server)
    config.include_context = vec!["http".to_string()];

    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let context_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "invalid-directive-context")
        .collect();

    assert_eq!(
        context_errors.len(),
        1,
        "Expected 1 invalid-directive-context error when included from http context, got: {:?}",
        context_errors
    );
    assert!(
        context_errors[0]
            .message
            .contains("'location' directive cannot be inside 'http'"),
        "Expected location in http error, got: {}",
        context_errors[0].message
    );
}

// ============================================================================
// proxy-pass-domain tests
// ============================================================================

#[test]
fn test_proxy_pass_domain_detects_domain() {
    let content = r#"
http {
    server {
        location / {
            proxy_pass http://api.example.com;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let proxy_pass_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "proxy-pass-domain")
        .collect();

    assert_eq!(
        proxy_pass_errors.len(),
        1,
        "Expected 1 proxy-pass-domain warning, got: {:?}",
        proxy_pass_errors
    );
    assert!(
        proxy_pass_errors[0].message.contains("api.example.com"),
        "Expected warning to mention the domain, got: {}",
        proxy_pass_errors[0].message
    );
}

#[test]
fn test_proxy_pass_domain_detects_localhost() {
    let content = r#"
http {
    server {
        location / {
            proxy_pass http://localhost:8080;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let proxy_pass_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "proxy-pass-domain")
        .collect();

    assert_eq!(
        proxy_pass_errors.len(),
        1,
        "Expected 1 proxy-pass-domain warning for localhost, got: {:?}",
        proxy_pass_errors
    );
    assert!(
        proxy_pass_errors[0].message.contains("localhost"),
        "Expected warning to mention localhost, got: {}",
        proxy_pass_errors[0].message
    );
}

#[test]
fn test_proxy_pass_domain_allows_ip_address() {
    let content = r#"
http {
    server {
        location / {
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let proxy_pass_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "proxy-pass-domain")
        .collect();

    assert!(
        proxy_pass_errors.is_empty(),
        "Expected no proxy-pass-domain warning for IP address, got: {:?}",
        proxy_pass_errors
    );
}

#[test]
fn test_proxy_pass_domain_allows_ipv6_address() {
    let content = r#"
http {
    server {
        location / {
            proxy_pass http://[::1]:8080;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let proxy_pass_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "proxy-pass-domain")
        .collect();

    assert!(
        proxy_pass_errors.is_empty(),
        "Expected no proxy-pass-domain warning for IPv6 address, got: {:?}",
        proxy_pass_errors
    );
}

#[test]
fn test_proxy_pass_domain_allows_variable() {
    let content = r#"
http {
    server {
        location / {
            set $backend "api.example.com";
            proxy_pass http://$backend;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let proxy_pass_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "proxy-pass-domain")
        .collect();

    assert!(
        proxy_pass_errors.is_empty(),
        "Expected no proxy-pass-domain warning for variable, got: {:?}",
        proxy_pass_errors
    );
}

#[test]
fn test_proxy_pass_domain_allows_upstream_name() {
    let content = r#"
http {
    upstream backend {
        server 127.0.0.1:8080;
    }
    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let proxy_pass_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "proxy-pass-domain")
        .collect();

    assert!(
        proxy_pass_errors.is_empty(),
        "Expected no proxy-pass-domain warning for upstream name, got: {:?}",
        proxy_pass_errors
    );
}

#[test]
fn test_proxy_pass_domain_allows_unix_socket() {
    let content = r#"
http {
    server {
        location / {
            proxy_pass http://unix:/var/run/app.sock;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let proxy_pass_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "proxy-pass-domain")
        .collect();

    assert!(
        proxy_pass_errors.is_empty(),
        "Expected no proxy-pass-domain warning for unix socket, got: {:?}",
        proxy_pass_errors
    );
}

#[test]
fn test_proxy_pass_domain_detects_multiple() {
    let content = r#"
http {
    server {
        location /api {
            proxy_pass http://api.example.com;
        }
        location /backend {
            proxy_pass http://backend.internal;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let proxy_pass_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "proxy-pass-domain")
        .collect();

    assert_eq!(
        proxy_pass_errors.len(),
        2,
        "Expected 2 proxy-pass-domain warnings, got: {:?}",
        proxy_pass_errors
    );
}

#[test]
fn test_upstream_server_domain_without_resolve() {
    let content = r#"
http {
    upstream backend {
        server api.example.com:80;
    }
    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let upstream_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "upstream-server-no-resolve")
        .collect();

    assert_eq!(
        upstream_errors.len(),
        1,
        "Expected 1 upstream-server-no-resolve warning for upstream without resolve, got: {:?}",
        upstream_errors
    );
    assert!(
        upstream_errors[0].message.contains("upstream server"),
        "Expected warning about upstream server, got: {}",
        upstream_errors[0].message
    );
    assert!(
        upstream_errors[0].message.contains("api.example.com"),
        "Expected warning to mention the domain, got: {}",
        upstream_errors[0].message
    );
}

#[test]
fn test_upstream_server_domain_with_resolve_and_zone() {
    let content = r#"
http {
    upstream backend {
        zone backend_zone 64k;
        server api.example.com:80 resolve;
    }
    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let upstream_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "upstream-server-no-resolve")
        .collect();

    assert!(
        upstream_errors.is_empty(),
        "Expected no upstream-server-no-resolve warning for upstream with resolve and zone, got: {:?}",
        upstream_errors
    );
}

#[test]
fn test_upstream_server_domain_with_resolve_but_no_zone() {
    let content = r#"
http {
    upstream backend {
        server api.example.com:80 resolve;
    }
    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let upstream_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "upstream-server-no-resolve")
        .collect();

    assert_eq!(
        upstream_errors.len(),
        1,
        "Expected 1 upstream-server-no-resolve warning for resolve without zone, got: {:?}",
        upstream_errors
    );
    assert!(
        upstream_errors[0].message.contains("zone"),
        "Expected warning about missing zone, got: {}",
        upstream_errors[0].message
    );
}

#[test]
fn test_upstream_server_ip_address() {
    let content = r#"
http {
    upstream backend {
        server 127.0.0.1:8080;
    }
    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let upstream_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "upstream-server-no-resolve")
        .collect();

    assert!(
        upstream_errors.is_empty(),
        "Expected no upstream-server-no-resolve warning for upstream with IP address, got: {:?}",
        upstream_errors
    );
}

#[test]
fn test_proxy_set_header_inheritance_missing_parent_headers() {
    let content = r#"
http {
    server {
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;

        location / {
            proxy_set_header X-Custom "value";
            proxy_pass http://backend;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let inheritance_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "directive-inheritance")
        .collect();

    assert_eq!(
        inheritance_errors.len(),
        1,
        "Expected 1 directive-inheritance warning, got: {:?}",
        inheritance_errors
    );
    assert!(
        inheritance_errors[0].message.contains("host"),
        "Expected warning to mention 'host', got: {}",
        inheritance_errors[0].message
    );
}

#[test]
fn test_proxy_set_header_inheritance_all_headers_included() {
    let content = r#"
http {
    server {
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;

        location / {
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Custom "value";
            proxy_pass http://backend;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let inheritance_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "directive-inheritance")
        .collect();

    assert!(
        inheritance_errors.is_empty(),
        "Expected no directive-inheritance warning when all headers included, got: {:?}",
        inheritance_errors
    );
}

#[test]
fn test_proxy_set_header_inheritance_no_child_headers() {
    let content = r#"
http {
    server {
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;

        location / {
            proxy_pass http://backend;
        }
    }
}
"#;
    let config = parse_string(content).expect("Failed to parse config");
    let linter = get_default_linter();
    let errors = linter.lint(&config, std::path::Path::new("test.conf"));

    let inheritance_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.rule == "directive-inheritance")
        .collect();

    assert!(
        inheritance_errors.is_empty(),
        "Expected no warning when child has no proxy_set_header, got: {:?}",
        inheritance_errors
    );
}
