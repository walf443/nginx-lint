//! server-tokens-enabled plugin
//!
//! This plugin detects when server_tokens is enabled, which exposes nginx version
//! information in response headers and error pages.
//!
//! server_tokens defaults to 'on', so this plugin also warns when no server_tokens
//! directive is found in the http context.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;

/// Check if server_tokens is enabled
#[derive(Default)]
pub struct ServerTokensEnabledPlugin;

impl Plugin for ServerTokensEnabledPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "server-tokens-enabled",
            "security",
            "Detects when server_tokens is enabled (exposes nginx version)",
        )
        .with_severity("warning")
        .with_why(
            "When server_tokens is 'on' (the default), nginx includes its version number in \
             the Server response header and on default error pages. This information can help \
             attackers identify specific vulnerabilities associated with your nginx version.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_core_module.html#server_tokens".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let mut has_server_tokens_off = false;
        let mut has_server_tokens_on = false;
        let mut http_block_line: Option<usize> = None;

        // Check if this config is included from within http context
        let in_http_include_context = config.include_context.iter().any(|c| c == "http");

        for ctx in config.all_directives_with_context() {
            // Track if we have an http block and remember its line
            if ctx.directive.is("http") {
                http_block_line = Some(ctx.directive.span.start.line);
            }

            // Only check server_tokens in http context (http, server, location)
            let in_http_context = ctx.is_inside("http") || in_http_include_context;
            if !in_http_context {
                continue;
            }

            if ctx.directive.is("server_tokens") {
                // 'off' or 'build' both hide the version number
                if ctx.directive.first_arg_is("off") || ctx.directive.first_arg_is("build") {
                    has_server_tokens_off = true;
                } else if ctx.directive.first_arg_is("on") {
                    has_server_tokens_on = true;
                    // Explicit 'on' - warn with fix
                    let directive = ctx.directive;
                    let start = directive.span.start.offset - directive.leading_whitespace.len();
                    let end = directive.span.end.offset;
                    let fixed = format!("{}server_tokens off;", directive.leading_whitespace);

                    let error = LintError::warning(
                        "server-tokens-enabled",
                        "security",
                        "server_tokens should be 'off' to hide nginx version",
                        directive.span.start.line,
                        directive.span.start.column,
                    )
                    .with_fix(Fix::replace_range(start, end, &fixed));
                    errors.push(error);
                }
            }
        }

        // If we have http context but no server_tokens directive at all, warn about the default
        // Don't warn if we already warned about explicit 'on' - that's redundant
        // Also don't warn if we're inside server/location context (via --context http,server)
        // because server_tokens is typically set at the http level in the parent config
        let has_http_context = http_block_line.is_some();
        let is_nested_include = config.include_context.iter().any(|c| c == "server" || c == "location");
        if (has_http_context || in_http_include_context) && !has_server_tokens_off && !has_server_tokens_on && !is_nested_include {
            // Use http block line if available, otherwise line 1 for included files
            let line = http_block_line.unwrap_or(1);
            errors.push(LintError::warning(
                "server-tokens-enabled",
                "security",
                "server_tokens defaults to 'on', consider adding 'server_tokens off;' in http context",
                line,
                1,
            ));
        }

        errors
    }
}

// Export the plugin
nginx_lint::export_plugin!(ServerTokensEnabledPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::{PluginTestRunner, TestCase};

    #[test]
    fn test_detects_server_tokens_on() {
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

        runner.assert_has_errors(
            r#"
http {
    server_tokens on;
}
"#,
        );
    }

    #[test]
    fn test_no_error_when_off() {
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

        runner.assert_no_errors(
            r#"
http {
    server_tokens off;
}
"#,
        );
    }

    #[test]
    fn test_warns_when_not_specified_defaults_to_on() {
        // server_tokens defaults to 'on', so we should warn when not specified
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

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
    fn test_error_location() {
        TestCase::new(
            r#"
http {
    server_tokens on;
}
"#,
        )
        .expect_error_count(1)
        .expect_error_on_line(3)
        .expect_message_contains("server_tokens")
        .expect_has_fix()
        .run(&ServerTokensEnabledPlugin);
    }

    #[test]
    fn test_multiple_occurrences() {
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

        // 2 explicit 'on' = 2 errors
        runner.assert_errors(
            r#"
http {
    server_tokens on;
    server {
        server_tokens on;
    }
}
"#,
            2,
        );
    }

    #[test]
    fn test_server_tokens_off_in_server_block() {
        // server_tokens off in a nested block should be sufficient
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        server_tokens off;
    }
}
"#,
        );
    }

    #[test]
    fn test_ignores_stream_context() {
        // server_tokens is not valid in stream context, so we should ignore it
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

        // No http context means no warning about server_tokens
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
    fn test_ignores_server_tokens_in_stream_context() {
        // Even if someone mistakenly puts server_tokens in stream,
        // we don't warn about it (nginx will reject it anyway)
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

        runner.assert_no_errors(
            r#"
stream {
    server_tokens on;
}
"#,
        );
    }

    #[test]
    fn test_no_context_no_http() {
        // Config without http block should not warn
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

        runner.assert_no_errors(
            r#"
events {
    worker_connections 1024;
}
"#,
        );
    }

    #[test]
    fn test_examples_with_fix() {
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);
        runner.test_examples_with_fix(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_server_tokens_build_is_acceptable() {
        // 'build' hides version number, showing only "nginx" or build name
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

        runner.assert_no_errors(
            r#"
http {
    server_tokens build;
}
"#,
        );
    }

    #[test]
    fn test_http_and_stream_mixed() {
        // Only http context should be checked, stream should be ignored
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

        runner.assert_no_errors(
            r#"
http {
    server_tokens off;
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
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

        // 1 error: no server_tokens off in http context
        runner.assert_errors(
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
            1,
        );
    }

    #[test]
    fn test_include_context_from_http() {
        // File included from http context should be checked
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

        let plugin = ServerTokensEnabledPlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should warn because no server_tokens off in http context
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("defaults to 'on'"));
    }

    #[test]
    fn test_include_context_from_http_with_server_tokens_off() {
        // File included from http context with server_tokens off should be OK
        use nginx_lint::parse_string;

        let mut config = parse_string(
            r#"
server {
    server_tokens off;
    listen 80;
}
"#,
        )
        .unwrap();

        // Simulate being included from http context
        config.include_context = vec!["http".to_string()];

        let plugin = ServerTokensEnabledPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_include_context_from_server() {
        // File included from server context (within http) should NOT warn about default
        // because server_tokens is typically set at the http level in parent config
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

        let plugin = ServerTokensEnabledPlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should NOT warn - server_tokens is expected to be set at http level in parent
        assert!(errors.is_empty(), "Expected no errors for nested include context, got: {:?}", errors);
    }

    #[test]
    fn test_include_context_from_server_with_explicit_on() {
        // Explicit 'on' should still warn even in nested context
        use nginx_lint::parse_string;

        let mut config = parse_string(
            r#"
server_tokens on;
"#,
        )
        .unwrap();

        // Simulate being included from http > server context
        config.include_context = vec!["http".to_string(), "server".to_string()];

        let plugin = ServerTokensEnabledPlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should warn because explicit 'on' is always wrong
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("should be 'off'"));
    }

    #[test]
    fn test_include_context_not_from_http() {
        // File included from non-http context should not be checked
        use nginx_lint::parse_string;

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

        let plugin = ServerTokensEnabledPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert!(errors.is_empty(), "Expected no errors for stream context, got: {:?}", errors);
    }
}
