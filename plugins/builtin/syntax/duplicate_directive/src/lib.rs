//! duplicate-directive plugin
//!
//! This plugin detects duplicate directives that should only appear once
//! in a given context.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;

/// Check for duplicate directives
#[derive(Default)]
pub struct DuplicateDirectivePlugin;

/// Directives that should only appear once in main context
const UNIQUE_DIRECTIVES: &[&str] = &[
    "worker_processes",
    "pid",
    "error_log",
    "user",
    "daemon",
    "master_process",
    "timer_resolution",
    "lock_file",
    "pcre_jit",
    "thread_pool",
];

impl Plugin for DuplicateDirectivePlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "duplicate-directive",
            "syntax",
            "Detects duplicate directives in the same context",
        )
        .with_severity("warning")
        .with_why(
            "Some directives cannot be specified multiple times in the same context. \
             When duplicated, nginx may use only the last value or throw an error. \
             Duplicate directives often indicate unintentional configuration mistakes \
             and should be reviewed.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        // Count occurrences of each unique directive
        for unique_name in UNIQUE_DIRECTIVES {
            let mut count = 0usize;
            for item in &config.items {
                if let ConfigItem::Directive(directive) = item {
                    if directive.name.as_str() == *unique_name {
                        count += 1;
                        if count > 1 {
                            // Create error for the duplicate
                            let error = LintError::warning(
                                "duplicate-directive",
                                "syntax",
                                "Duplicate directive in main context",
                                directive.span.start.line,
                                directive.span.start.column,
                            )
                            .with_fix(Fix::delete(directive.span.start.line));
                            errors.push(error);
                        }
                    }
                }
            }
        }

        errors
    }
}

// Export the plugin
nginx_lint::export_plugin!(DuplicateDirectivePlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::{PluginTestRunner, TestCase};

    #[test]
    fn test_detects_duplicate_worker_processes() {
        let runner = PluginTestRunner::new(DuplicateDirectivePlugin);

        runner.assert_has_errors(
            r#"
worker_processes 4;
worker_processes 8;
"#,
        );
    }

    #[test]
    fn test_no_error_single_directive() {
        let runner = PluginTestRunner::new(DuplicateDirectivePlugin);

        runner.assert_no_errors(
            r#"
worker_processes auto;
"#,
        );
    }

    #[test]
    fn test_detects_duplicate_pid() {
        let runner = PluginTestRunner::new(DuplicateDirectivePlugin);

        runner.assert_has_errors(
            r#"
pid /run/nginx.pid;
pid /var/run/nginx.pid;
"#,
        );
    }

    #[test]
    fn test_error_location() {
        TestCase::new(
            r#"
worker_processes 4;
worker_processes 8;
"#,
        )
        .expect_error_count(1)
        .expect_error_on_line(3)
        .expect_message_contains("Duplicate directive")
        .expect_has_fix()
        .run(&DuplicateDirectivePlugin);
    }

    #[test]
    fn test_fix_deletes_duplicate() {
        TestCase::new(
            r#"worker_processes 4;
worker_processes 8;
"#,
        )
        .expect_error_count(1)
        .expect_fix_on_line(2)
        .run(&DuplicateDirectivePlugin);
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(DuplicateDirectivePlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }
}
