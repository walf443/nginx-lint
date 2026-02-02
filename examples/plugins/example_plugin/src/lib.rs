//! Example nginx-lint plugin
//!
//! This plugin checks for the use of `debug_connection` directive,
//! which should not be used in production.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```
//!
//! The output will be at:
//! `target/wasm32-unknown-unknown/release/example_plugin.wasm`

use nginx_lint::plugin_sdk::prelude::*;

/// Example plugin that warns about debug_connection directive
#[derive(Default)]
pub struct DebugConnectionRule;

impl Plugin for DebugConnectionRule {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "debug-connection-in-production".to_string(),
            category: "best_practices".to_string(),
            description: "Warns about debug_connection directive which should not be used in production".to_string(),
        }
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("debug_connection") {
                errors.push(LintError::warning(
                    "debug-connection-in-production",
                    "best_practices",
                    "debug_connection should not be used in production; it can expose sensitive information",
                    directive.span.start.line,
                    directive.span.start.column,
                ));
            }
        }

        errors
    }
}

// Export the plugin
nginx_lint::export_plugin!(DebugConnectionRule);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::{PluginTestRunner, TestCase};

    #[test]
    fn test_detects_debug_connection() {
        let runner = PluginTestRunner::new(DebugConnectionRule);

        runner.assert_has_errors(
            r#"
events {
    debug_connection 192.168.1.1;
}
"#,
        );
    }

    #[test]
    fn test_no_error_without_debug_connection() {
        let runner = PluginTestRunner::new(DebugConnectionRule);

        runner.assert_no_errors(
            r#"
events {
    worker_connections 1024;
}
"#,
        );
    }

    #[test]
    fn test_error_location() {
        TestCase::new(
            r#"
events {
    debug_connection 192.168.1.1;
}
"#,
        )
        .expect_error_count(1)
        .expect_error_on_line(3)
        .expect_message_contains("production")
        .run(&DebugConnectionRule);
    }

    #[test]
    fn test_multiple_debug_connections() {
        let runner = PluginTestRunner::new(DebugConnectionRule);

        runner.assert_errors(
            r#"
events {
    debug_connection 192.168.1.1;
    debug_connection 10.0.0.1;
}
"#,
            2,
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(DebugConnectionRule);
        let fixtures_dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/debug_connection"
        );
        runner.test_fixtures(fixtures_dir);
    }
}
