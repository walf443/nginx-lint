//! listen-http2-deprecated plugin
//!
//! This plugin detects the use of the deprecated `http2` parameter in `listen` directives.
//! Since nginx 1.25.1, `listen 443 ssl http2;` is deprecated in favor of the standalone
//! `http2 on;` directive.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check for deprecated `http2` parameter in `listen` directives
#[derive(Default)]
pub struct ListenHttp2DeprecatedPlugin;

impl Plugin for ListenHttp2DeprecatedPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "listen-http2-deprecated",
            "deprecation",
            "Detects the deprecated 'http2' parameter in 'listen' directive (use 'http2 on;' instead)",
        )
        .with_severity("warning")
        .with_why(
            "The 'http2' parameter on the 'listen' directive was deprecated in nginx 1.25.1. \
             Use the standalone 'http2 on;' directive instead. \
             The deprecated syntax may be removed in a future nginx version.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_v2_module.html".to_string(),
            "https://github.com/walf443/nginx-lint/blob/main/plugins/builtin/deprecation/listen_http2_deprecated/tests/container_test.rs".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();

        check_items(&config.items, &err, &mut errors);

        errors
    }
}

/// Check a list of config items for `listen ... http2;` and generate fixes
fn check_items(items: &[ConfigItem], err: &ErrorBuilder, errors: &mut Vec<LintError>) {
    let mut listen_with_http2: Vec<&Directive> = Vec::new();
    let mut has_http2_directive = false;

    for item in items {
        if let ConfigItem::Directive(directive) = item {
            if directive.is("listen") && directive.has_arg("http2") {
                listen_with_http2.push(directive);
            }
            if directive.is("http2") && directive.first_arg_is("on") {
                has_http2_directive = true;
            }
            // Recurse into child blocks
            if let Some(block) = &directive.block {
                check_items(&block.items, err, errors);
            }
        }
    }

    if listen_with_http2.is_empty() {
        return;
    }

    // Report a single error for the first listen directive with http2
    let first = listen_with_http2[0];
    let mut error = err.warning_at(
        "'http2' parameter in 'listen' is deprecated, use 'http2 on;' instead",
        first,
    );

    // For each listen directive with http2, remove the ` http2` argument
    for listen_dir in &listen_with_http2 {
        if let Some(http2_arg) = listen_dir.args.iter().find(|a| a.as_str() == "http2") {
            // Remove the space before `http2` and the `http2` itself
            let start = http2_arg.span.start.offset - 1; // include preceding space
            let end = http2_arg.span.end.offset;
            error = error.with_fix(Fix::replace_range(start, end, ""));
        }
    }

    // Add `http2 on;` after the last listen directive (only if not already present)
    if !has_http2_directive {
        let last_listen = listen_with_http2.last().unwrap();
        error = error.with_fix(last_listen.insert_after("http2 on;"));
    }

    errors.push(error);
}

// Export the plugin
nginx_lint_plugin::export_plugin!(ListenHttp2DeprecatedPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::{PluginTestRunner, TestCase};

    #[test]
    fn test_detects_listen_http2() {
        let runner = PluginTestRunner::new(ListenHttp2DeprecatedPlugin);

        runner.assert_has_errors(
            r#"
server {
    listen 443 ssl http2;
}
"#,
        );
    }

    #[test]
    fn test_no_error_without_http2() {
        let runner = PluginTestRunner::new(ListenHttp2DeprecatedPlugin);

        runner.assert_no_errors(
            r#"
server {
    listen 443 ssl;
}
"#,
        );
    }

    #[test]
    fn test_multiple_listen_directives() {
        // Multiple listen directives with http2 should produce only one error
        TestCase::new(
            r#"
server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name example.com;
}
"#,
        )
        .expect_error_count(1)
        .expect_error_on_line(3)
        .expect_message_contains("http2")
        .expect_has_fix()
        .expect_fix_produces(
            r#"
server {
    listen 443 ssl;
    listen [::]:443 ssl;
    http2 on;
    server_name example.com;
}
"#,
        )
        .run(&ListenHttp2DeprecatedPlugin);
    }

    #[test]
    fn test_http2_directive_already_exists() {
        // Should still warn about deprecated listen syntax but not add duplicate http2 on;
        TestCase::new(
            r#"
server {
    listen 443 ssl http2;
    http2 on;
    server_name example.com;
}
"#,
        )
        .expect_error_count(1)
        .expect_has_fix()
        .expect_fix_produces(
            r#"
server {
    listen 443 ssl;
    http2 on;
    server_name example.com;
}
"#,
        )
        .run(&ListenHttp2DeprecatedPlugin);
    }

    #[test]
    fn test_examples_with_fix() {
        let runner = PluginTestRunner::new(ListenHttp2DeprecatedPlugin);
        runner.test_examples_with_fix(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(ListenHttp2DeprecatedPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
