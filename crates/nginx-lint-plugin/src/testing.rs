//! Testing utilities for plugin development
//!
//! This module provides helper functions for testing plugins with fixture files.

use super::types::{Config, Fix, LintError, Plugin, PluginSpec};
use std::path::{Path, PathBuf};

/// Macro to get the fixtures directory path relative to the plugin's Cargo.toml
///
/// Usage in plugin tests:
/// ```ignore
/// runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
/// ```
#[macro_export]
macro_rules! fixtures_dir {
    () => {
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")
    };
}

/// Test runner for plugins
pub struct PluginTestRunner<P: Plugin> {
    plugin: P,
}

impl<P: Plugin> PluginTestRunner<P> {
    /// Create a new test runner for a plugin
    pub fn new(plugin: P) -> Self {
        Self { plugin }
    }

    /// Get plugin spec
    pub fn spec(&self) -> PluginSpec {
        self.plugin.spec()
    }

    /// Run the plugin check on a config string
    pub fn check_string(&self, content: &str) -> Result<Vec<LintError>, String> {
        let config: Config = nginx_lint_common::parse_string(content)
            .map_err(|e| format!("Failed to parse config: {}", e))?;
        Ok(self.plugin.check(&config, "test.conf"))
    }

    /// Run the plugin check on a file
    pub fn check_file(&self, path: &Path) -> Result<Vec<LintError>, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;
        let config: Config = nginx_lint_common::parse_string(&content)
            .map_err(|e| format!("Failed to parse config: {}", e))?;
        Ok(self.plugin.check(&config, path.to_string_lossy().as_ref()))
    }

    /// Test all fixtures in a directory
    pub fn test_fixtures(&self, fixtures_dir: &str) {
        let fixtures_path = PathBuf::from(fixtures_dir);
        if !fixtures_path.exists() {
            panic!("Fixtures directory not found: {}", fixtures_dir);
        }

        let plugin_spec = self.plugin.spec();
        let rule_name = &plugin_spec.name;

        let entries = std::fs::read_dir(&fixtures_path)
            .unwrap_or_else(|e| panic!("Failed to read fixtures directory: {}", e));

        let mut tested_count = 0;

        for entry in entries {
            let entry = entry.expect("Failed to read directory entry");
            let case_path = entry.path();

            if !case_path.is_dir() {
                continue;
            }

            let case_name = case_path.file_name().unwrap().to_string_lossy();
            self.test_case(&case_path, rule_name, &case_name);
            tested_count += 1;
        }

        if tested_count == 0 {
            panic!("No test cases found in {}", fixtures_dir);
        }
    }

    /// Test a single fixture case
    fn test_case(&self, case_path: &Path, rule_name: &str, case_name: &str) {
        let error_path = case_path.join("error").join("nginx.conf");
        let expected_path = case_path.join("expected").join("nginx.conf");

        if error_path.exists() {
            let errors = self
                .check_file(&error_path)
                .unwrap_or_else(|e| panic!("Failed to check error fixture {}: {}", case_name, e));

            let rule_errors: Vec<_> = errors.iter().filter(|e| e.rule == rule_name).collect();

            assert!(
                !rule_errors.is_empty(),
                "Expected {} errors in {}/error/nginx.conf, got none",
                rule_name,
                case_name
            );
        }

        if expected_path.exists() {
            let errors = self.check_file(&expected_path).unwrap_or_else(|e| {
                panic!("Failed to check expected fixture {}: {}", case_name, e)
            });

            let rule_errors: Vec<_> = errors.iter().filter(|e| e.rule == rule_name).collect();

            assert!(
                rule_errors.is_empty(),
                "Expected no {} errors in {}/expected/nginx.conf, got: {:?}",
                rule_name,
                case_name,
                rule_errors
            );
        }
    }

    /// Assert that a config string produces specific errors
    pub fn assert_errors(&self, content: &str, expected_count: usize) {
        let errors = self.check_string(content).expect("Failed to check config");
        let plugin_spec = self.plugin.spec();
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_spec.name)
            .collect();

        assert_eq!(
            rule_errors.len(),
            expected_count,
            "Expected {} errors from {}, got {}: {:?}",
            expected_count,
            plugin_spec.name,
            rule_errors.len(),
            rule_errors
        );
    }

    /// Assert that a config string produces no errors
    pub fn assert_no_errors(&self, content: &str) {
        self.assert_errors(content, 0);
    }

    /// Assert that a config string produces at least one error
    pub fn assert_has_errors(&self, content: &str) {
        let errors = self.check_string(content).expect("Failed to check config");
        let plugin_spec = self.plugin.spec();
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_spec.name)
            .collect();

        assert!(
            !rule_errors.is_empty(),
            "Expected at least one error from {}, got none",
            plugin_spec.name
        );
    }

    /// Assert that a config string produces an error on a specific line
    pub fn assert_error_on_line(&self, content: &str, expected_line: usize) {
        let errors = self.check_string(content).expect("Failed to check config");
        let plugin_spec = self.plugin.spec();
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_spec.name)
            .collect();

        let has_error_on_line = rule_errors.iter().any(|e| e.line == Some(expected_line));

        assert!(
            has_error_on_line,
            "Expected error from {} on line {}, got errors on lines: {:?}",
            plugin_spec.name,
            expected_line,
            rule_errors.iter().map(|e| e.line).collect::<Vec<_>>()
        );
    }

    /// Assert that errors contain a specific message substring
    pub fn assert_error_message_contains(&self, content: &str, expected_substring: &str) {
        let errors = self.check_string(content).expect("Failed to check config");
        let plugin_spec = self.plugin.spec();
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_spec.name)
            .collect();

        let has_message = rule_errors
            .iter()
            .any(|e| e.message.contains(expected_substring));

        assert!(
            has_message,
            "Expected error message containing '{}' from {}, got messages: {:?}",
            expected_substring,
            plugin_spec.name,
            rule_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }

    /// Assert that errors have fixes
    pub fn assert_has_fix(&self, content: &str) {
        let errors = self.check_string(content).expect("Failed to check config");
        let plugin_spec = self.plugin.spec();
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_spec.name)
            .collect();

        let has_fix = rule_errors.iter().any(|e| !e.fixes.is_empty());

        assert!(
            has_fix,
            "Expected at least one error with fix from {}, got errors: {:?}",
            plugin_spec.name, rule_errors
        );
    }

    /// Assert that applying fixes produces the expected output
    pub fn assert_fix_produces(&self, content: &str, expected: &str) {
        let errors = self.check_string(content).expect("Failed to check config");
        let plugin_spec = self.plugin.spec();

        let fixes: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_spec.name)
            .flat_map(|e| e.fixes.iter())
            .collect();

        assert!(
            !fixes.is_empty(),
            "Expected at least one fix from {}, got none",
            plugin_spec.name
        );

        let result = apply_fixes(content, &fixes);
        let expected_normalized = expected.trim();
        let result_normalized = result.trim();

        assert_eq!(
            result_normalized, expected_normalized,
            "Fix did not produce expected output.\nExpected:\n{}\n\nGot:\n{}",
            expected_normalized, result_normalized
        );
    }

    /// Test using bad.conf and good.conf example content
    pub fn test_examples(&self, bad_conf: &str, good_conf: &str) {
        let plugin_spec = self.plugin.spec();

        let errors = self
            .check_string(bad_conf)
            .expect("Failed to parse bad.conf");
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_spec.name)
            .collect();
        assert!(
            !rule_errors.is_empty(),
            "bad.conf should produce at least one {} error, got none",
            plugin_spec.name
        );

        let errors = self
            .check_string(good_conf)
            .expect("Failed to parse good.conf");
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_spec.name)
            .collect();
        assert!(
            rule_errors.is_empty(),
            "good.conf should not produce {} errors, got: {:?}",
            plugin_spec.name,
            rule_errors
        );
    }

    /// Test using bad.conf and good.conf, and verify fix converts bad to good
    pub fn test_examples_with_fix(&self, bad_conf: &str, good_conf: &str) {
        let plugin_spec = self.plugin.spec();

        let errors = self
            .check_string(bad_conf)
            .expect("Failed to parse bad.conf");
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_spec.name)
            .collect();
        assert!(
            !rule_errors.is_empty(),
            "bad.conf should produce at least one {} error, got none",
            plugin_spec.name
        );

        let fixes: Vec<_> = rule_errors.iter().flat_map(|e| e.fixes.iter()).collect();
        assert!(
            !fixes.is_empty(),
            "bad.conf errors should have fixes, got none"
        );

        let errors = self
            .check_string(good_conf)
            .expect("Failed to parse good.conf");
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_spec.name)
            .collect();
        assert!(
            rule_errors.is_empty(),
            "good.conf should not produce {} errors, got: {:?}",
            plugin_spec.name,
            rule_errors
        );

        let fixed = apply_fixes(bad_conf, &fixes);
        assert_eq!(
            fixed.trim(),
            good_conf.trim(),
            "Applying fixes to bad.conf should produce good.conf.\nExpected:\n{}\n\nGot:\n{}",
            good_conf.trim(),
            fixed.trim()
        );
    }
}

/// Test builder for inline tests
pub struct TestCase {
    content: String,
    expected_error_count: Option<usize>,
    expected_lines: Vec<usize>,
    expected_message_contains: Vec<String>,
    expect_has_fix: bool,
    expected_fix_output: Option<String>,
    expected_fix_on_lines: Vec<usize>,
}

impl TestCase {
    /// Create a new test case with the given config content
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            expected_error_count: None,
            expected_lines: Vec::new(),
            expected_message_contains: Vec::new(),
            expect_has_fix: false,
            expected_fix_output: None,
            expected_fix_on_lines: Vec::new(),
        }
    }

    /// Expect a specific number of errors
    pub fn expect_error_count(mut self, count: usize) -> Self {
        self.expected_error_count = Some(count);
        self
    }

    /// Expect no errors
    pub fn expect_no_errors(self) -> Self {
        self.expect_error_count(0)
    }

    /// Expect at least one error on the given line
    pub fn expect_error_on_line(mut self, line: usize) -> Self {
        self.expected_lines.push(line);
        self
    }

    /// Expect error messages to contain the given substring
    pub fn expect_message_contains(mut self, substring: impl Into<String>) -> Self {
        self.expected_message_contains.push(substring.into());
        self
    }

    /// Expect at least one error to have a fix
    pub fn expect_has_fix(mut self) -> Self {
        self.expect_has_fix = true;
        self
    }

    /// Expect a fix on a specific line
    pub fn expect_fix_on_line(mut self, line: usize) -> Self {
        self.expected_fix_on_lines.push(line);
        self.expect_has_fix = true;
        self
    }

    /// Expect that applying all fixes produces the given output
    pub fn expect_fix_produces(mut self, expected: impl Into<String>) -> Self {
        self.expected_fix_output = Some(expected.into());
        self.expect_has_fix = true;
        self
    }

    /// Run the test case with the given plugin
    pub fn run<P: Plugin>(self, plugin: &P) {
        let config: Config = nginx_lint_common::parse_string(&self.content)
            .unwrap_or_else(|e| panic!("Failed to parse test config: {}", e));

        let errors = plugin.check(&config, "test.conf");
        let plugin_spec = plugin.spec();
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_spec.name)
            .collect();

        if let Some(expected_count) = self.expected_error_count {
            assert_eq!(
                rule_errors.len(),
                expected_count,
                "Expected {} errors, got {}: {:?}",
                expected_count,
                rule_errors.len(),
                rule_errors
            );
        }

        for expected_line in &self.expected_lines {
            let has_error = rule_errors.iter().any(|e| e.line == Some(*expected_line));
            assert!(
                has_error,
                "Expected error on line {}, got errors on lines: {:?}",
                expected_line,
                rule_errors.iter().map(|e| e.line).collect::<Vec<_>>()
            );
        }

        for expected_msg in &self.expected_message_contains {
            let has_message = rule_errors.iter().any(|e| e.message.contains(expected_msg));
            assert!(
                has_message,
                "Expected error message containing '{}', got: {:?}",
                expected_msg,
                rule_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
            );
        }

        if self.expect_has_fix {
            let has_fix = rule_errors.iter().any(|e| !e.fixes.is_empty());
            assert!(
                has_fix,
                "Expected at least one error with fix, got errors: {:?}",
                rule_errors
            );
        }

        for expected_line in &self.expected_fix_on_lines {
            let has_fix_on_line = rule_errors.iter().flat_map(|e| e.fixes.iter()).any(|f| {
                if f.is_range_based() {
                    let start = f.start_offset.unwrap_or(0);
                    let line = offset_to_line(&self.content, start);
                    line == *expected_line
                } else {
                    f.line == *expected_line
                }
            });
            assert!(
                has_fix_on_line,
                "Expected fix on line {}, got fixes on lines: {:?}",
                expected_line,
                rule_errors
                    .iter()
                    .flat_map(|e| e.fixes.iter().map(|f| {
                        if f.is_range_based() {
                            let start = f.start_offset.unwrap_or(0);
                            offset_to_line(&self.content, start)
                        } else {
                            f.line
                        }
                    }))
                    .collect::<Vec<_>>()
            );
        }

        if let Some(expected_output) = &self.expected_fix_output {
            let fixes: Vec<_> = rule_errors.iter().flat_map(|e| e.fixes.iter()).collect();

            assert!(
                !fixes.is_empty(),
                "Expected at least one fix to check output, got none"
            );

            let result = apply_fixes(&self.content, &fixes);
            let expected_normalized = expected_output.trim();
            let result_normalized = result.trim();

            assert_eq!(
                result_normalized, expected_normalized,
                "Fix did not produce expected output.\nExpected:\n{}\n\nGot:\n{}",
                expected_normalized, result_normalized
            );
        }
    }
}

/// Convert a byte offset to a 1-based line number
fn offset_to_line(content: &str, offset: usize) -> usize {
    let offset = offset.min(content.len());
    content[..offset].chars().filter(|&c| c == '\n').count() + 1
}

/// Apply fixes to content and return the result
fn apply_fixes(content: &str, fixes: &[&Fix]) -> String {
    let (range_fixes, line_fixes): (Vec<&&Fix>, Vec<&&Fix>) = fixes
        .iter()
        .partition(|f| f.start_offset.is_some() && f.end_offset.is_some());

    let mut result = content.to_string();

    if !range_fixes.is_empty() {
        let mut sorted_range_fixes = range_fixes;
        sorted_range_fixes.sort_by(|a, b| b.start_offset.unwrap().cmp(&a.start_offset.unwrap()));

        let mut applied_ranges: Vec<(usize, usize)> = Vec::new();

        for fix in sorted_range_fixes {
            let start = fix.start_offset.unwrap();
            let end = fix.end_offset.unwrap();

            let overlaps = applied_ranges.iter().any(|(s, e)| start < *e && end > *s);
            if overlaps {
                continue;
            }

            if start <= result.len() && end <= result.len() && start <= end {
                result.replace_range(start..end, &fix.new_text);
                applied_ranges.push((start, start + fix.new_text.len()));
            }
        }
    }

    if !line_fixes.is_empty() {
        let mut lines: Vec<String> = result.lines().map(|l| l.to_string()).collect();

        let mut sorted_line_fixes = line_fixes;
        sorted_line_fixes.sort_by(|a, b| b.line.cmp(&a.line));

        for fix in sorted_line_fixes {
            let line_idx = fix.line.saturating_sub(1);

            if fix.delete_line {
                if line_idx < lines.len() {
                    lines.remove(line_idx);
                }
            } else if fix.insert_after {
                if line_idx < lines.len() {
                    lines.insert(line_idx + 1, fix.new_text.clone());
                }
            } else if let Some(old_text) = &fix.old_text {
                if line_idx < lines.len() {
                    lines[line_idx] = lines[line_idx].replace(old_text, &fix.new_text);
                }
            } else if line_idx < lines.len() {
                lines[line_idx] = fix.new_text.clone();
            }
        }

        result = lines.join("\n");
    }

    result
}
