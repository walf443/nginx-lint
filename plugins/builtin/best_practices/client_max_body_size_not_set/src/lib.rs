//! client-max-body-size-not-set plugin
//!
//! This plugin warns when client_max_body_size is not explicitly set in the http context.
//! The default value is 1m, which may not be appropriate for all applications.
//! Explicitly setting this value ensures intentional control over request body size limits.
//!
//! client_max_body_size is valid in http, server, and location contexts.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check if client_max_body_size is explicitly set
#[derive(Default)]
pub struct ClientMaxBodySizeNotSetPlugin;

impl Plugin for ClientMaxBodySizeNotSetPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "client-max-body-size-not-set",
            "best-practices",
            "Warns when client_max_body_size is not explicitly set",
        )
        .with_severity("warning")
        .with_why(
            "The default client_max_body_size is 1m, which may cause unexpected 413 \
             (Request Entity Too Large) errors for file uploads or large POST requests. \
             Explicitly setting this value ensures intentional control over request body \
             size limits and helps prevent security issues from unrestricted upload sizes.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_core_module.html#client_max_body_size"
                .to_string(),
            "https://github.com/walf443/nginx-lint/blob/main/plugins/builtin/best_practices/client_max_body_size_not_set/tests/container_test.rs".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut has_client_max_body_size = false;
        let mut http_directive: Option<&Directive> = None;

        for ctx in config.all_directives_with_context() {
            // Track if we have an http block in THIS file
            if ctx.directive.is("http") && http_directive.is_none() {
                http_directive = Some(ctx.directive);
            }

            // Only check client_max_body_size in http context (http, server, location)
            // Note: ctx.is_inside() already includes include_context from Config
            if !ctx.is_inside("http") {
                continue;
            }

            if ctx.directive.is("client_max_body_size") {
                has_client_max_body_size = true;
                break;
            }
        }

        // Only warn if THIS file has an http block but no client_max_body_size
        // Don't warn for included files - client_max_body_size should be set in the main config
        if let Some(http_dir) = http_directive
            && !has_client_max_body_size
        {
            let err = self.spec().error_builder();
            return vec![err.warning_at(
                    "Consider explicitly setting client_max_body_size to control request body size limits",
                    http_dir,
                )];
        }

        vec![]
    }
}

// Export the plugin
nginx_lint_plugin::export_plugin!(ClientMaxBodySizeNotSetPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::PluginTestRunner;

    #[test]
    fn test_no_client_max_body_size_directive() {
        let runner = PluginTestRunner::new(ClientMaxBodySizeNotSetPlugin);

        runner.assert_has_errors(
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
    fn test_client_max_body_size_in_http() {
        let runner = PluginTestRunner::new(ClientMaxBodySizeNotSetPlugin);

        runner.assert_no_errors(
            r#"
http {
    client_max_body_size 10m;
    server {
        listen 80;
    }
}
"#,
        );
    }

    #[test]
    fn test_client_max_body_size_in_server() {
        let runner = PluginTestRunner::new(ClientMaxBodySizeNotSetPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        listen 80;
        client_max_body_size 5m;
    }
}
"#,
        );
    }

    #[test]
    fn test_client_max_body_size_in_location() {
        let runner = PluginTestRunner::new(ClientMaxBodySizeNotSetPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        listen 80;
        location /upload {
            client_max_body_size 50m;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_client_max_body_size_zero() {
        // client_max_body_size 0 disables the limit - still counts as explicitly set
        let runner = PluginTestRunner::new(ClientMaxBodySizeNotSetPlugin);

        runner.assert_no_errors(
            r#"
http {
    client_max_body_size 0;
    server {
        listen 80;
    }
}
"#,
        );
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(ClientMaxBodySizeNotSetPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_no_http_context_no_warning() {
        // Config without http block should not warn
        let runner = PluginTestRunner::new(ClientMaxBodySizeNotSetPlugin);

        runner.assert_no_errors(
            r#"
events {
    worker_connections 1024;
}
"#,
        );
    }

    #[test]
    fn test_stream_context_no_warning() {
        // stream context doesn't support client_max_body_size, so no warning
        let runner = PluginTestRunner::new(ClientMaxBodySizeNotSetPlugin);

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
    fn test_http_and_stream_mixed() {
        // Only http context should be checked
        let runner = PluginTestRunner::new(ClientMaxBodySizeNotSetPlugin);

        runner.assert_no_errors(
            r#"
http {
    client_max_body_size 10m;
    server {
        listen 80;
    }
}
stream {
    server {
        listen 12345;
    }
}
"#,
        );
    }

    #[test]
    fn test_http_and_stream_mixed_warns_for_http() {
        // Should warn only about http context, not stream
        let runner = PluginTestRunner::new(ClientMaxBodySizeNotSetPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        listen 80;
    }
}
stream {
    server {
        listen 12345;
    }
}
"#,
        );
    }

    #[test]
    fn test_include_context_from_http() {
        // File included from http context should NOT warn
        // because client_max_body_size should be set in the parent config's http block
        use nginx_lint_plugin::parse_string;

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

        let plugin = ClientMaxBodySizeNotSetPlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should NOT warn - parent config should set client_max_body_size
        assert!(
            errors.is_empty(),
            "Expected no errors for included file, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_include_context_from_http_with_directive() {
        // File included from http context with client_max_body_size should be OK
        use nginx_lint_plugin::parse_string;

        let mut config = parse_string(
            r#"
client_max_body_size 10m;
server {
    listen 80;
}
"#,
        )
        .unwrap();

        // Simulate being included from http context
        config.include_context = vec!["http".to_string()];

        let plugin = ClientMaxBodySizeNotSetPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_include_context_not_from_http() {
        // File included from non-http context should not be checked
        use nginx_lint_plugin::parse_string;

        let mut config = parse_string(
            r#"
server {
    listen 12345;
}
"#,
        )
        .unwrap();

        // Simulate being included from stream context
        config.include_context = vec!["stream".to_string()];

        let plugin = ClientMaxBodySizeNotSetPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert!(
            errors.is_empty(),
            "Expected no errors for stream context, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(ClientMaxBodySizeNotSetPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
