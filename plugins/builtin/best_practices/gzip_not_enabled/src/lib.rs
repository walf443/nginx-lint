//! gzip-not-enabled plugin
//!
//! This plugin suggests enabling gzip compression for better performance.
//! Gzip compression significantly reduces response sizes and improves
//! page load times.
//!
//! gzip is only valid in http, server, and location contexts.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check if gzip compression is enabled
#[derive(Default)]
pub struct GzipNotEnabledPlugin;

impl Plugin for GzipNotEnabledPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "gzip-not-enabled",
            "best-practices",
            "Suggests enabling gzip compression for better performance",
        )
        .with_severity("info")
        .with_why(
            "Gzip compression can significantly reduce the size of HTTP responses, often by \
             60-80% for text-based content like HTML, CSS, and JavaScript. This improves page \
             load times and reduces bandwidth usage.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_gzip_module.html".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut gzip_on = false;
        let mut has_http_block = false;

        for ctx in config.all_directives_with_context() {
            // Track if we have an http block in THIS file
            if ctx.directive.is("http") {
                has_http_block = true;
            }

            // Only check gzip in http context (http, server, location)
            // Note: ctx.is_inside() already includes include_context from Config
            if !ctx.is_inside("http") {
                continue;
            }

            if ctx.directive.is("gzip") && ctx.directive.first_arg_is("on") {
                gzip_on = true;
                break;
            }
        }

        // Only warn if THIS file has an http block but no gzip on
        // Don't warn for included files - gzip should be set in the main config
        if has_http_block && !gzip_on {
            let err = self.info().error_builder();
            vec![err.info("Consider enabling gzip compression for better performance", 0, 0)]
        } else {
            vec![]
        }
    }
}

// Export the plugin
nginx_lint_plugin::export_plugin!(GzipNotEnabledPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::PluginTestRunner;

    #[test]
    fn test_no_gzip_directive() {
        let runner = PluginTestRunner::new(GzipNotEnabledPlugin);

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
    fn test_gzip_on() {
        let runner = PluginTestRunner::new(GzipNotEnabledPlugin);

        runner.assert_no_errors(
            r#"
http {
    gzip on;
    server {
        listen 80;
    }
}
"#,
        );
    }

    #[test]
    fn test_gzip_off() {
        let runner = PluginTestRunner::new(GzipNotEnabledPlugin);

        runner.assert_has_errors(
            r#"
http {
    gzip off;
    server {
        listen 80;
    }
}
"#,
        );
    }

    #[test]
    fn test_gzip_in_server_block() {
        let runner = PluginTestRunner::new(GzipNotEnabledPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        listen 80;
        gzip on;
    }
}
"#,
        );
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(GzipNotEnabledPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_no_http_context_no_warning() {
        // Config without http block should not warn about gzip
        let runner = PluginTestRunner::new(GzipNotEnabledPlugin);

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
        // stream context doesn't support gzip, so no warning
        let runner = PluginTestRunner::new(GzipNotEnabledPlugin);

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
        let runner = PluginTestRunner::new(GzipNotEnabledPlugin);

        runner.assert_no_errors(
            r#"
http {
    gzip on;
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
        let runner = PluginTestRunner::new(GzipNotEnabledPlugin);

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
        // because gzip should be set in the parent config's http block
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

        let plugin = GzipNotEnabledPlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should NOT warn - parent config should set gzip
        assert!(errors.is_empty(), "Expected no errors for included file, got: {:?}", errors);
    }

    #[test]
    fn test_include_context_from_http_with_gzip() {
        // File included from http context with gzip on should be OK
        use nginx_lint_plugin::parse_string;

        let mut config = parse_string(
            r#"
gzip on;
server {
    listen 80;
}
"#,
        )
        .unwrap();

        // Simulate being included from http context
        config.include_context = vec!["http".to_string()];

        let plugin = GzipNotEnabledPlugin;
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

        let plugin = GzipNotEnabledPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert!(errors.is_empty(), "Expected no errors for stream context, got: {:?}", errors);
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(GzipNotEnabledPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}