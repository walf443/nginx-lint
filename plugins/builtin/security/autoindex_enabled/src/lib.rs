//! autoindex-enabled plugin
//!
//! This plugin detects when autoindex is enabled, which can expose
//! directory contents and lead to information disclosure.
//!
//! autoindex is only valid in http, server, and location contexts.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check if autoindex is enabled
#[derive(Default)]
pub struct AutoindexEnabledPlugin;

impl Plugin for AutoindexEnabledPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
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
            "https://github.com/walf443/nginx-lint/blob/main/plugins/builtin/security/autoindex_enabled/tests/container_test.rs".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();

        for ctx in config.all_directives_with_context() {
            // Only check autoindex in http context (http, server, location)
            // Note: ctx.is_inside() already includes include_context from Config
            if !ctx.is_inside("http") {
                continue;
            }

            if ctx.directive.is("autoindex") && ctx.directive.first_arg_is("on") {
                let directive = ctx.directive;
                let error = err
                    .warning_at(
                        "autoindex is enabled, which can expose directory contents",
                        directive,
                    )
                    .with_fix(directive.replace_with("autoindex off;"));
                errors.push(error);
            }
        }

        errors
    }
}

nginx_lint_plugin::export_component_plugin!(AutoindexEnabledPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::{PluginTestRunner, TestCase};

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

    #[test]
    fn test_ignores_stream_context() {
        // autoindex is not valid in stream context, so we should ignore it
        let runner = PluginTestRunner::new(AutoindexEnabledPlugin);

        runner.assert_no_errors(
            r#"
stream {
    server {
        listen 12345;
    }
}
"#,
        );
    }

    #[test]
    fn test_ignores_autoindex_in_stream_context() {
        // Even if someone mistakenly puts autoindex in stream,
        // we don't warn about it (nginx will reject it anyway)
        let runner = PluginTestRunner::new(AutoindexEnabledPlugin);

        runner.assert_no_errors(
            r#"
stream {
    autoindex on;
}
"#,
        );
    }

    #[test]
    fn test_include_context_from_http() {
        // File included from http context should be checked
        use nginx_lint_plugin::parse_string;

        let mut config = parse_string(
            r#"
autoindex on;
"#,
        )
        .unwrap();

        // Simulate being included from http context
        config.include_context = vec!["http".to_string()];

        let plugin = AutoindexEnabledPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("autoindex"));
    }

    #[test]
    fn test_include_context_from_server() {
        // File included from server context should be checked
        use nginx_lint_plugin::parse_string;

        let mut config = parse_string(
            r#"
location / {
    autoindex on;
}
"#,
        )
        .unwrap();

        // Simulate being included from http > server context
        config.include_context = vec!["http".to_string(), "server".to_string()];

        let plugin = AutoindexEnabledPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn test_include_context_not_from_http() {
        // File included from non-http context should not be checked
        use nginx_lint_plugin::parse_string;

        let mut config = parse_string(
            r#"
autoindex on;
"#,
        )
        .unwrap();

        // Simulate being included from stream context
        config.include_context = vec!["stream".to_string()];

        let plugin = AutoindexEnabledPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert!(
            errors.is_empty(),
            "Expected no errors for stream context, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(AutoindexEnabledPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
