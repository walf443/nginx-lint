//! ssl-on-deprecated plugin
//!
//! This plugin detects the use of the deprecated `ssl on;` directive.
//! Since nginx 1.15.0, `ssl on;` is deprecated in favor of the `ssl` parameter
//! on the `listen` directive (e.g., `listen 443 ssl;`).
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check for deprecated `ssl on;` directive
#[derive(Default)]
pub struct SslOnDeprecatedPlugin;

impl Plugin for SslOnDeprecatedPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "ssl-on-deprecated",
            "deprecation",
            "Detects the deprecated 'ssl on;' directive (use 'listen ... ssl;' instead)",
        )
        .with_severity("warning")
        .with_why(
            "The 'ssl on;' directive was deprecated in nginx 1.15.0 and \
             removed in nginx 1.25.1. Use the 'ssl' parameter on the \
             'listen' directive instead (e.g., 'listen 443 ssl;'). \
             Using 'ssl on;' on nginx 1.25.1+ will cause a configuration error.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_ssl_module.html#ssl".to_string(),
            "https://github.com/walf443/nginx-lint/blob/main/plugins/builtin/deprecation/ssl_on_deprecated/tests/container_test.rs".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();

        check_items(&config.items, &err, &mut errors);

        errors
    }
}

/// Check a list of config items for `ssl on;` and generate fixes
fn check_items(items: &[ConfigItem], err: &ErrorBuilder, errors: &mut Vec<LintError>) {
    let mut listen_without_ssl: Vec<&Directive> = Vec::new();
    let mut ssl_on_directives: Vec<&Directive> = Vec::new();

    for item in items {
        if let ConfigItem::Directive(directive) = item {
            if directive.is("listen") && !directive.has_arg("ssl") {
                listen_without_ssl.push(directive);
            }
            if directive.is("ssl") && directive.first_arg_is("on") {
                ssl_on_directives.push(directive);
            }
            // Recurse into child blocks
            if let Some(block) = &directive.block {
                check_items(&block.items, err, errors);
            }
        }
    }

    for ssl_dir in &ssl_on_directives {
        let mut error = err.warning_at(
            "'ssl on;' is deprecated, use 'listen ... ssl;' instead",
            *ssl_dir,
        );

        // Delete `ssl on;`
        error = error.with_fix(ssl_dir.delete_line());

        // Add `ssl` to each listen that lacks it.
        // If there are no listen directives (e.g., SSL config in an included file),
        // we only delete `ssl on;` — the listen directive lives elsewhere.
        // Using a narrow range fix (insert at last arg end) avoids conflicts with
        // other rules' fixes (e.g., indent) that may also target the listen line.
        for listen_dir in &listen_without_ssl {
            if let Some(last_arg) = listen_dir.args.last() {
                let insert_offset = last_arg.span.end.offset;
                error = error.with_fix(Fix::replace_range(insert_offset, insert_offset, " ssl"));
            }
        }

        errors.push(error);
    }
}

// Export the plugin
nginx_lint_plugin::export_plugin!(SslOnDeprecatedPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::{PluginTestRunner, TestCase};

    #[test]
    fn test_detects_ssl_on() {
        let runner = PluginTestRunner::new(SslOnDeprecatedPlugin);

        runner.assert_has_errors(
            r#"
server {
    listen 443;
    ssl on;
}
"#,
        );
    }

    #[test]
    fn test_no_error_without_ssl_directive() {
        let runner = PluginTestRunner::new(SslOnDeprecatedPlugin);

        runner.assert_no_errors(
            r#"
server {
    listen 443 ssl;
}
"#,
        );
    }

    #[test]
    fn test_no_error_for_ssl_off() {
        let runner = PluginTestRunner::new(SslOnDeprecatedPlugin);

        runner.assert_no_errors(
            r#"
server {
    listen 443;
    ssl off;
}
"#,
        );
    }

    #[test]
    fn test_error_location() {
        TestCase::new(
            r#"
server {
    listen 443;
    ssl on;
}
"#,
        )
        .expect_error_count(1)
        .expect_error_on_line(4)
        .expect_message_contains("ssl on")
        .expect_has_fix()
        .run(&SslOnDeprecatedPlugin);
    }

    #[test]
    fn test_examples_with_fix() {
        let runner = PluginTestRunner::new(SslOnDeprecatedPlugin);
        runner.test_examples_with_fix(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_listen_already_has_ssl_no_extra_fix() {
        // listen already has ssl, so fix should only delete `ssl on;` without modifying listen
        TestCase::new(
            r#"
server {
    listen 443 ssl;
    ssl on;
}
"#,
        )
        .expect_error_count(1)
        .expect_has_fix()
        .expect_fix_produces(
            r#"
server {
    listen 443 ssl;
}
"#,
        )
        .run(&SslOnDeprecatedPlugin);
    }

    #[test]
    fn test_multiple_listen_directives() {
        // Multiple listen directives should all get ssl added
        TestCase::new(
            r#"
server {
    listen 443;
    listen [::]:443;
    ssl on;
}
"#,
        )
        .expect_error_count(1)
        .expect_has_fix()
        .expect_fix_produces(
            r#"
server {
    listen 443 ssl;
    listen [::]:443 ssl;
}
"#,
        )
        .run(&SslOnDeprecatedPlugin);
    }

    #[test]
    fn test_multiple_listen_some_with_ssl() {
        // Only listen directives without ssl should get ssl added
        TestCase::new(
            r#"
server {
    listen 443 ssl;
    listen [::]:443;
    ssl on;
}
"#,
        )
        .expect_error_count(1)
        .expect_has_fix()
        .expect_fix_produces(
            r#"
server {
    listen 443 ssl;
    listen [::]:443 ssl;
}
"#,
        )
        .run(&SslOnDeprecatedPlugin);
    }

    #[test]
    fn test_no_listen_directive() {
        // ssl on without any listen directive (e.g., SSL config in an included file)
        // just delete ssl on; — the listen directive lives in another file
        TestCase::new(
            r#"
server {
    ssl on;
    ssl_certificate /etc/ssl/certs/server.crt;
}
"#,
        )
        .expect_error_count(1)
        .expect_has_fix()
        .expect_fix_produces(
            r#"
server {
    ssl_certificate /etc/ssl/certs/server.crt;
}
"#,
        )
        .run(&SslOnDeprecatedPlugin);
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(SslOnDeprecatedPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
