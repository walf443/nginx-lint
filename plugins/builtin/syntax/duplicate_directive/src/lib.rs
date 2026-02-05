//! duplicate-directive plugin
//!
//! This plugin detects duplicate directives that should only appear once
//! in a given context. It now uses context awareness to detect duplicates
//! within each block, not just at the main level.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;
use std::collections::HashMap;

/// Check for duplicate directives
#[derive(Default)]
pub struct DuplicateDirectivePlugin;

/// Directives that should only appear once in main context
const MAIN_UNIQUE_DIRECTIVES: &[&str] = &[
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

/// Directives that should only appear once per http context
const HTTP_UNIQUE_DIRECTIVES: &[&str] = &[
    "default_type",
    "sendfile",
    "tcp_nopush",
    "tcp_nodelay",
    "keepalive_timeout",
    "types_hash_max_size",
    "server_names_hash_bucket_size",
    "server_names_hash_max_size",
    "variables_hash_max_size",
    "variables_hash_bucket_size",
];

/// Directives that should only appear once per server context
const SERVER_UNIQUE_DIRECTIVES: &[&str] = &[
    "root",
    "index",
    "server_tokens",
    "client_max_body_size",
    "access_log",
    "error_log",
];

/// Directives that should only appear once per location context
const LOCATION_UNIQUE_DIRECTIVES: &[&str] = &[
    "root",
    "alias",
    "index",
    "try_files",
    "internal",
    "autoindex",
    "client_max_body_size",
];

/// Directives that should only appear once per upstream context
const UPSTREAM_UNIQUE_DIRECTIVES: &[&str] = &[
    "hash",
    "ip_hash",
    "least_conn",
    "random",
    "keepalive",
    "keepalive_requests",
    "keepalive_timeout",
    "zone",
];

fn get_unique_directives_for_context(context: Option<&str>) -> &'static [&'static str] {
    match context {
        None => MAIN_UNIQUE_DIRECTIVES,
        Some("http") => HTTP_UNIQUE_DIRECTIVES,
        Some("server") => SERVER_UNIQUE_DIRECTIVES,
        Some("location") => LOCATION_UNIQUE_DIRECTIVES,
        Some("upstream") => UPSTREAM_UNIQUE_DIRECTIVES,
        _ => &[],
    }
}

impl DuplicateDirectivePlugin {
    /// Check for duplicates within a single block's direct children
    fn check_block(
        &self,
        items: &[ConfigItem],
        parent_context: Option<&str>,
        err: &ErrorBuilder,
        errors: &mut Vec<LintError>,
    ) {
        let unique_directives = get_unique_directives_for_context(parent_context);
        let mut seen: HashMap<&str, usize> = HashMap::new();

        for item in items {
            if let ConfigItem::Directive(directive) = item {
                // Check if this directive should be unique in its context
                if unique_directives.contains(&directive.name.as_str()) {
                    if let Some(&first_line) = seen.get(directive.name.as_str()) {
                        // This is a duplicate
                        let context_name = parent_context.unwrap_or("main");
                        let message = format!(
                            "Duplicate '{}' directive in {} context (first defined on line {})",
                            directive.name, context_name, first_line
                        );

                        let error = err
                            .warning_at(&message, directive)
                            .with_fix(directive.delete_line());
                        errors.push(error);
                    } else {
                        seen.insert(&directive.name, directive.span.start.line);
                    }
                }

                // Recursively check nested blocks
                if let Some(block) = &directive.block {
                    self.check_block(&block.items, Some(&directive.name), err, errors);
                }
            }
        }
    }
}

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
        let err = self.info().error_builder();

        // Determine the parent context from include_context
        let parent_context = config.immediate_parent_context();

        self.check_block(&config.items, parent_context, &err, &mut errors);

        errors
    }
}

// Export the plugin
nginx_lint_plugin::export_plugin!(DuplicateDirectivePlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::{PluginTestRunner, TestCase};

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
        .expect_message_contains("Duplicate")
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

    // New context-aware tests

    #[test]
    fn test_duplicate_root_in_location() {
        let runner = PluginTestRunner::new(DuplicateDirectivePlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location / {
            root /var/www;
            root /var/www/html;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_root_in_different_locations_is_ok() {
        // root in different location blocks is fine
        let runner = PluginTestRunner::new(DuplicateDirectivePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            root /var/www;
        }
        location /static {
            root /var/static;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_duplicate_in_http_context() {
        let runner = PluginTestRunner::new(DuplicateDirectivePlugin);

        runner.assert_has_errors(
            r#"
http {
    sendfile on;
    sendfile off;
}
"#,
        );
    }

    #[test]
    fn test_duplicate_in_server_context() {
        let runner = PluginTestRunner::new(DuplicateDirectivePlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        server_tokens off;
        server_tokens on;
    }
}
"#,
        );
    }

    #[test]
    fn test_same_directive_different_servers_is_ok() {
        // Same directive in different server blocks is fine
        let runner = PluginTestRunner::new(DuplicateDirectivePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        server_tokens off;
    }
    server {
        server_tokens on;
    }
}
"#,
        );
    }

    #[test]
    fn test_duplicate_in_upstream() {
        let runner = PluginTestRunner::new(DuplicateDirectivePlugin);

        runner.assert_has_errors(
            r#"
http {
    upstream backend {
        ip_hash;
        ip_hash;
    }
}
"#,
        );
    }

    #[test]
    fn test_include_context_main() {
        // Test with include context from main
        use nginx_lint_plugin::parse_string;

        let config = parse_string(
            r#"
worker_processes 4;
worker_processes 8;
"#,
        )
        .unwrap();

        let plugin = DuplicateDirectivePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Duplicate 'worker_processes'"));
    }

    #[test]
    fn test_include_context_from_server() {
        // Test with include context from server
        use nginx_lint_plugin::parse_string;

        let mut config = parse_string(
            r#"
root /var/www;
root /var/www/html;
"#,
        )
        .unwrap();

        // Simulate being included from http > server context
        config.include_context = vec!["http".to_string(), "server".to_string()];

        let plugin = DuplicateDirectivePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Duplicate 'root'"));
        assert!(errors[0].message.contains("server context"));
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(DuplicateDirectivePlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}