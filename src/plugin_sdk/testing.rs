//! Testing utilities for plugin development
//!
//! This module provides helper functions for testing plugins with fixture files.
//!
//! # Directory Structure
//!
//! Test fixtures should be organized as follows:
//! ```text
//! tests/fixtures/
//! └── my_plugin/
//!     └── 001_basic/
//!         ├── error/
//!         │   └── nginx.conf    # Config that should trigger errors
//!         └── expected/
//!             └── nginx.conf    # Expected output (no errors for this rule)
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use nginx_lint::plugin_sdk::testing::PluginTestRunner;
//! use nginx_lint::plugin_sdk::prelude::*;
//!
//! #[derive(Default)]
//! struct MyPlugin;
//!
//! impl Plugin for MyPlugin {
//!     // ... implementation ...
//! }
//!
//! #[test]
//! fn test_my_plugin() {
//!     let runner = PluginTestRunner::new(MyPlugin);
//!     runner.test_fixtures("tests/fixtures/my_plugin");
//! }
//! ```

use super::types::{Config, Fix, LintError, Plugin, PluginInfo};
use std::path::{Path, PathBuf};

/// Test runner for plugins
pub struct PluginTestRunner<P: Plugin> {
    plugin: P,
}

impl<P: Plugin> PluginTestRunner<P> {
    /// Create a new test runner for a plugin
    pub fn new(plugin: P) -> Self {
        Self { plugin }
    }

    /// Get plugin info
    pub fn info(&self) -> PluginInfo {
        self.plugin.info()
    }

    /// Run the plugin check on a config string
    pub fn check_string(&self, content: &str) -> Result<Vec<LintError>, String> {
        let config: Config =
            crate::parse_string(content).map_err(|e| format!("Failed to parse config: {}", e))?;
        Ok(self.plugin.check(&config, "test.conf"))
    }

    /// Run the plugin check on a file
    pub fn check_file(&self, path: &Path) -> Result<Vec<LintError>, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;
        let config: Config =
            crate::parse_string(&content).map_err(|e| format!("Failed to parse config: {}", e))?;
        Ok(self.plugin.check(&config, path.to_string_lossy().as_ref()))
    }

    /// Test all fixtures in a directory
    ///
    /// The directory should contain subdirectories for each test case,
    /// with `error/nginx.conf` and optionally `expected/nginx.conf`.
    pub fn test_fixtures(&self, fixtures_dir: &str) {
        let fixtures_path = PathBuf::from(fixtures_dir);
        if !fixtures_path.exists() {
            panic!("Fixtures directory not found: {}", fixtures_dir);
        }

        let plugin_info = self.plugin.info();
        let rule_name = &plugin_info.name;

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

        // Test error fixture: should detect errors
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

        // Test expected fixture: should have no errors for this rule
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
        let errors = self
            .check_string(content)
            .expect("Failed to check config");
        let plugin_info = self.plugin.info();
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_info.name)
            .collect();

        assert_eq!(
            rule_errors.len(),
            expected_count,
            "Expected {} errors from {}, got {}: {:?}",
            expected_count,
            plugin_info.name,
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
        let errors = self
            .check_string(content)
            .expect("Failed to check config");
        let plugin_info = self.plugin.info();
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_info.name)
            .collect();

        assert!(
            !rule_errors.is_empty(),
            "Expected at least one error from {}, got none",
            plugin_info.name
        );
    }

    /// Assert that a config string produces an error on a specific line
    pub fn assert_error_on_line(&self, content: &str, expected_line: usize) {
        let errors = self
            .check_string(content)
            .expect("Failed to check config");
        let plugin_info = self.plugin.info();
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_info.name)
            .collect();

        let has_error_on_line = rule_errors.iter().any(|e| e.line == Some(expected_line));

        assert!(
            has_error_on_line,
            "Expected error from {} on line {}, got errors on lines: {:?}",
            plugin_info.name,
            expected_line,
            rule_errors.iter().map(|e| e.line).collect::<Vec<_>>()
        );
    }

    /// Assert that errors contain a specific message substring
    pub fn assert_error_message_contains(&self, content: &str, expected_substring: &str) {
        let errors = self
            .check_string(content)
            .expect("Failed to check config");
        let plugin_info = self.plugin.info();
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_info.name)
            .collect();

        let has_message = rule_errors
            .iter()
            .any(|e| e.message.contains(expected_substring));

        assert!(
            has_message,
            "Expected error message containing '{}' from {}, got messages: {:?}",
            expected_substring,
            plugin_info.name,
            rule_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }

    /// Assert that errors have fixes
    pub fn assert_has_fix(&self, content: &str) {
        let errors = self
            .check_string(content)
            .expect("Failed to check config");
        let plugin_info = self.plugin.info();
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_info.name)
            .collect();

        let has_fix = rule_errors.iter().any(|e| e.fix.is_some());

        assert!(
            has_fix,
            "Expected at least one error with fix from {}, got errors: {:?}",
            plugin_info.name,
            rule_errors
        );
    }

    /// Assert that applying fixes produces the expected output
    pub fn assert_fix_produces(&self, content: &str, expected: &str) {
        let errors = self
            .check_string(content)
            .expect("Failed to check config");
        let plugin_info = self.plugin.info();

        let fixes: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_info.name)
            .filter_map(|e| e.fix.as_ref())
            .collect();

        assert!(
            !fixes.is_empty(),
            "Expected at least one fix from {}, got none",
            plugin_info.name
        );

        let result = apply_fixes(content, &fixes);
        let expected_normalized = expected.trim();
        let result_normalized = result.trim();

        assert_eq!(
            result_normalized,
            expected_normalized,
            "Fix did not produce expected output.\nExpected:\n{}\n\nGot:\n{}",
            expected_normalized,
            result_normalized
        );
    }

    /// Test using bad.conf and good.conf example content
    ///
    /// This method verifies:
    /// 1. bad_conf produces at least one error for this plugin
    /// 2. good_conf produces no errors for this plugin
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let runner = PluginTestRunner::new(MyPlugin);
    /// runner.test_examples(
    ///     include_str!("../examples/bad.conf"),
    ///     include_str!("../examples/good.conf"),
    /// );
    /// ```
    pub fn test_examples(&self, bad_conf: &str, good_conf: &str) {
        let plugin_info = self.plugin.info();

        // Test bad.conf - should produce errors
        let errors = self
            .check_string(bad_conf)
            .expect("Failed to parse bad.conf");
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_info.name)
            .collect();
        assert!(
            !rule_errors.is_empty(),
            "bad.conf should produce at least one {} error, got none",
            plugin_info.name
        );

        // Test good.conf - should not produce errors
        let errors = self
            .check_string(good_conf)
            .expect("Failed to parse good.conf");
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_info.name)
            .collect();
        assert!(
            rule_errors.is_empty(),
            "good.conf should not produce {} errors, got: {:?}",
            plugin_info.name,
            rule_errors
        );
    }

    /// Test using bad.conf and good.conf, and verify fix converts bad to good
    ///
    /// This method verifies:
    /// 1. bad_conf produces at least one error with a fix
    /// 2. good_conf produces no errors
    /// 3. Applying fixes to bad_conf produces good_conf
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let runner = PluginTestRunner::new(MyPlugin);
    /// runner.test_examples_with_fix(
    ///     include_str!("../examples/bad.conf"),
    ///     include_str!("../examples/good.conf"),
    /// );
    /// ```
    pub fn test_examples_with_fix(&self, bad_conf: &str, good_conf: &str) {
        let plugin_info = self.plugin.info();

        // Test bad.conf - should produce errors with fixes
        let errors = self
            .check_string(bad_conf)
            .expect("Failed to parse bad.conf");
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_info.name)
            .collect();
        assert!(
            !rule_errors.is_empty(),
            "bad.conf should produce at least one {} error, got none",
            plugin_info.name
        );

        let fixes: Vec<_> = rule_errors
            .iter()
            .filter_map(|e| e.fix.as_ref())
            .collect();
        assert!(
            !fixes.is_empty(),
            "bad.conf errors should have fixes, got none"
        );

        // Test good.conf - should not produce errors
        let errors = self
            .check_string(good_conf)
            .expect("Failed to parse good.conf");
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_info.name)
            .collect();
        assert!(
            rule_errors.is_empty(),
            "good.conf should not produce {} errors, got: {:?}",
            plugin_info.name,
            rule_errors
        );

        // Apply fixes to bad.conf and compare with good.conf
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
///
/// # Example
///
/// ```rust,ignore
/// use nginx_lint::plugin_sdk::testing::TestCase;
///
/// TestCase::new("server_tokens on;")
///     .expect_error_count(1)
///     .expect_error_on_line(1)
///     .expect_fix_produces("server_tokens off;")
///     .run(&MyPlugin);
/// ```
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
        let config: Config = crate::parse_string(&self.content)
            .unwrap_or_else(|e| panic!("Failed to parse test config: {}", e));

        let errors = plugin.check(&config, "test.conf");
        let plugin_info = plugin.info();
        let rule_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.rule == plugin_info.name)
            .collect();

        // Check error count
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

        // Check expected lines
        for expected_line in &self.expected_lines {
            let has_error = rule_errors.iter().any(|e| e.line == Some(*expected_line));
            assert!(
                has_error,
                "Expected error on line {}, got errors on lines: {:?}",
                expected_line,
                rule_errors.iter().map(|e| e.line).collect::<Vec<_>>()
            );
        }

        // Check message content
        for expected_msg in &self.expected_message_contains {
            let has_message = rule_errors.iter().any(|e| e.message.contains(expected_msg));
            assert!(
                has_message,
                "Expected error message containing '{}', got: {:?}",
                expected_msg,
                rule_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
            );
        }

        // Check fix presence
        if self.expect_has_fix {
            let has_fix = rule_errors.iter().any(|e| e.fix.is_some());
            assert!(
                has_fix,
                "Expected at least one error with fix, got errors: {:?}",
                rule_errors
            );
        }

        // Check fix on specific lines
        for expected_line in &self.expected_fix_on_lines {
            let has_fix_on_line = rule_errors
                .iter()
                .filter_map(|e| e.fix.as_ref())
                .any(|f| f.line == *expected_line);
            assert!(
                has_fix_on_line,
                "Expected fix on line {}, got fixes on lines: {:?}",
                expected_line,
                rule_errors
                    .iter()
                    .filter_map(|e| e.fix.as_ref().map(|f| f.line))
                    .collect::<Vec<_>>()
            );
        }

        // Check fix output
        if let Some(expected_output) = &self.expected_fix_output {
            let fixes: Vec<_> = rule_errors
                .iter()
                .filter_map(|e| e.fix.as_ref())
                .collect();

            assert!(
                !fixes.is_empty(),
                "Expected at least one fix to check output, got none"
            );

            let result = apply_fixes(&self.content, &fixes);
            let expected_normalized = expected_output.trim();
            let result_normalized = result.trim();

            assert_eq!(
                result_normalized,
                expected_normalized,
                "Fix did not produce expected output.\nExpected:\n{}\n\nGot:\n{}",
                expected_normalized,
                result_normalized
            );
        }
    }
}

/// Apply fixes to content and return the result
///
/// Supports both range-based and line-based fixes.
/// Range-based fixes are applied first, then line-based fixes.
fn apply_fixes(content: &str, fixes: &[&Fix]) -> String {
    // Separate range-based and line-based fixes
    let (range_fixes, line_fixes): (Vec<&&Fix>, Vec<&&Fix>) =
        fixes.iter().partition(|f| f.start_offset.is_some() && f.end_offset.is_some());

    let mut result = content.to_string();

    // Apply range-based fixes first (sort by start_offset descending)
    if !range_fixes.is_empty() {
        let mut sorted_range_fixes = range_fixes;
        sorted_range_fixes.sort_by(|a, b| {
            b.start_offset.unwrap().cmp(&a.start_offset.unwrap())
        });

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

    // Apply line-based fixes
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_sdk::prelude::*;

    #[derive(Default)]
    struct TestPlugin;

    impl Plugin for TestPlugin {
        fn info(&self) -> PluginInfo {
            PluginInfo::new("test-plugin", "test", "Test plugin")
        }

        fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
            let mut errors = Vec::new();
            for directive in config.all_directives() {
                if directive.is("test_directive") && directive.first_arg_is("bad") {
                    errors.push(LintError::warning(
                        "test-plugin",
                        "test",
                        "test_directive should not be 'bad'",
                        directive.span.start.line,
                        directive.span.start.column,
                    ));
                }
            }
            errors
        }
    }

    #[test]
    fn test_runner_check_string() {
        let runner = PluginTestRunner::new(TestPlugin);

        // Should detect error
        let errors = runner.check_string("test_directive bad;").unwrap();
        assert_eq!(errors.len(), 1);

        // Should not detect error
        let errors = runner.check_string("test_directive good;").unwrap();
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_runner_assert_methods() {
        let runner = PluginTestRunner::new(TestPlugin);

        runner.assert_errors("test_directive bad;", 1);
        runner.assert_no_errors("test_directive good;");
        runner.assert_has_errors("test_directive bad;");
        runner.assert_error_on_line("test_directive bad;", 1);
        runner.assert_error_message_contains("test_directive bad;", "should not be");
    }

    #[test]
    fn test_case_builder() {
        let plugin = TestPlugin;

        TestCase::new("test_directive bad;")
            .expect_error_count(1)
            .expect_error_on_line(1)
            .expect_message_contains("should not be")
            .run(&plugin);

        TestCase::new("test_directive good;")
            .expect_no_errors()
            .run(&plugin);
    }

    // Test plugin with fix support
    #[derive(Default)]
    struct TestPluginWithFix;

    impl Plugin for TestPluginWithFix {
        fn info(&self) -> PluginInfo {
            PluginInfo::new("test-plugin-fix", "test", "Test plugin with fix")
        }

        fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
            let mut errors = Vec::new();
            for directive in config.all_directives() {
                if directive.is("test_directive") && directive.first_arg_is("bad") {
                    errors.push(
                        LintError::warning(
                            "test-plugin-fix",
                            "test",
                            "test_directive should not be 'bad'",
                            directive.span.start.line,
                            directive.span.start.column,
                        )
                        .with_fix(Fix::replace(
                            directive.span.start.line,
                            "test_directive bad",
                            "test_directive good",
                        )),
                    );
                }
            }
            errors
        }
    }

    #[test]
    fn test_runner_assert_has_fix() {
        let runner = PluginTestRunner::new(TestPluginWithFix);
        runner.assert_has_fix("test_directive bad;");
    }

    #[test]
    fn test_runner_assert_fix_produces() {
        let runner = PluginTestRunner::new(TestPluginWithFix);
        runner.assert_fix_produces("test_directive bad;", "test_directive good;");
    }

    #[test]
    fn test_case_with_fix() {
        let plugin = TestPluginWithFix;

        TestCase::new("test_directive bad;")
            .expect_error_count(1)
            .expect_has_fix()
            .expect_fix_on_line(1)
            .expect_fix_produces("test_directive good;")
            .run(&plugin);
    }

    #[test]
    fn test_apply_fixes_replace() {
        let content = "line1\ntest bad\nline3";
        let fix = Fix::replace(2, "bad", "good");
        let result = apply_fixes(content, &[&fix]);
        assert_eq!(result, "line1\ntest good\nline3");
    }

    #[test]
    fn test_apply_fixes_replace_line() {
        let content = "line1\nold line\nline3";
        let fix = Fix::replace_line(2, "new line");
        let result = apply_fixes(content, &[&fix]);
        assert_eq!(result, "line1\nnew line\nline3");
    }

    #[test]
    fn test_apply_fixes_delete() {
        let content = "line1\nto delete\nline3";
        let fix = Fix::delete(2);
        let result = apply_fixes(content, &[&fix]);
        assert_eq!(result, "line1\nline3");
    }

    #[test]
    fn test_apply_fixes_insert_after() {
        let content = "line1\nline2";
        let fix = Fix::insert_after(1, "inserted");
        let result = apply_fixes(content, &[&fix]);
        assert_eq!(result, "line1\ninserted\nline2");
    }

    #[test]
    fn test_apply_fixes_multiple() {
        let content = "bad1\nbad2\nbad3";
        let fix1 = Fix::replace(1, "bad1", "good1");
        let fix3 = Fix::replace(3, "bad3", "good3");
        let result = apply_fixes(content, &[&fix1, &fix3]);
        assert_eq!(result, "good1\nbad2\ngood3");
    }
}
