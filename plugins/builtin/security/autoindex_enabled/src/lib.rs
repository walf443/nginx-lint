//! autoindex-enabled plugin
//!
//! This plugin detects when autoindex is enabled, which can expose
//! directory contents and lead to information disclosure.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;

/// Check if autoindex is enabled
#[derive(Default)]
pub struct AutoindexEnabledPlugin;

impl Plugin for AutoindexEnabledPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "autoindex-enabled",
            "security",
            "Detects when autoindex is enabled (can expose directory contents)",
        )
        .with_severity("warning")
        .with_why(
            "When autoindex is enabled, nginx will generate a directory listing when a request \
             is made to a directory without an index file. This can expose sensitive files, \
             backup files, or other content that should not be publicly accessible.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_autoindex_module.html".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("autoindex") && directive.first_arg_is("on") {
                // Calculate byte offsets for range-based fix
                let start = directive.span.start.offset - directive.leading_whitespace.len();
                let end = directive.span.end.offset;
                let fixed = format!("{}autoindex off;", directive.leading_whitespace);

                let error = LintError::warning(
                    "autoindex-enabled",
                    "security",
                    "autoindex is enabled, which can expose directory contents",
                    directive.span.start.line,
                    directive.span.start.column,
                )
                .with_fix(Fix::replace_range(start, end, &fixed));
                errors.push(error);
            }
        }

        errors
    }
}

// Export the plugin
nginx_lint::export_plugin!(AutoindexEnabledPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::{PluginTestRunner, TestCase};

    #[test]
    fn test_detects_autoindex_on() {
        let runner = PluginTestRunner::new(AutoindexEnabledPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location / {
            autoindex on;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_error_when_off() {
        let runner = PluginTestRunner::new(AutoindexEnabledPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            autoindex off;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_error_when_not_specified() {
        let runner = PluginTestRunner::new(AutoindexEnabledPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        listen 80;
    }
}
"#,
        );
    }

    #[test]
    fn test_error_location() {
        TestCase::new(
            r#"
http {
    server {
        location / {
            autoindex on;
        }
    }
}
"#,
        )
        .expect_error_count(1)
        .expect_error_on_line(5)
        .expect_message_contains("autoindex")
        .expect_has_fix()
        .run(&AutoindexEnabledPlugin);
    }

    #[test]
    fn test_fix_produces_correct_output() {
        TestCase::new("autoindex on;")
            .expect_error_count(1)
            .expect_fix_on_line(1)
            .expect_fix_produces("autoindex off;")
            .run(&AutoindexEnabledPlugin);
    }

    #[test]
    fn test_multiple_locations() {
        let runner = PluginTestRunner::new(AutoindexEnabledPlugin);

        runner.assert_errors(
            r#"
http {
    server {
        location /files {
            autoindex on;
        }
        location /docs {
            autoindex on;
        }
    }
}
"#,
            2,
        );
    }

    #[test]
    fn test_examples_with_fix() {
        let runner = PluginTestRunner::new(AutoindexEnabledPlugin);
        runner.test_examples_with_fix(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }
}
