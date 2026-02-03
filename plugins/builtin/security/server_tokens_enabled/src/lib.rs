//! server-tokens-enabled plugin
//!
//! This plugin detects when server_tokens is enabled, which exposes nginx version
//! information in response headers and error pages.
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
            "When server_tokens is 'on', nginx includes its version number in the Server \
             response header and on default error pages. This information can help attackers \
             identify specific vulnerabilities associated with your nginx version.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_core_module.html#server_tokens".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("server_tokens") && directive.first_arg_is("on") {
                // Calculate byte offsets for range-based fix
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
    fn test_no_error_when_not_specified() {
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

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
    fn test_fix_produces_correct_output() {
        TestCase::new("server_tokens on;")
            .expect_error_count(1)
            .expect_fix_on_line(1)
            .expect_fix_produces("server_tokens off;")
            .run(&ServerTokensEnabledPlugin);
    }

    #[test]
    fn test_multiple_occurrences() {
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);

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
    fn test_examples_with_fix() {
        let runner = PluginTestRunner::new(ServerTokensEnabledPlugin);
        runner.test_examples_with_fix(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }
}
