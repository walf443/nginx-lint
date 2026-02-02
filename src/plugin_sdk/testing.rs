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

use super::types::{Config, LintError, Plugin, PluginInfo};
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
///     .run(&MyPlugin);
/// ```
pub struct TestCase {
    content: String,
    expected_error_count: Option<usize>,
    expected_lines: Vec<usize>,
    expected_message_contains: Vec<String>,
}

impl TestCase {
    /// Create a new test case with the given config content
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            expected_error_count: None,
            expected_lines: Vec::new(),
            expected_message_contains: Vec::new(),
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
    }
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
}
