//! missing-error-log plugin
//!
//! This plugin suggests configuring error_log for debugging purposes.
//! Setting an appropriate log level helps capture necessary information
//! while managing disk usage.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;

/// Check if error_log is configured
#[derive(Default)]
pub struct MissingErrorLogPlugin;

impl Plugin for MissingErrorLogPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "missing-error-log",
            "best-practices",
            "Suggests configuring error_log for debugging",
        )
        .with_severity("info")
        .with_why(
            "Configuring error_log allows you to record errors and issues in log files for \
             troubleshooting purposes. Setting an appropriate log level helps capture necessary \
             information while managing disk usage.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/ngx_core_module.html#error_log".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        // Check if error_log directive exists anywhere in the config
        for directive in config.all_directives() {
            if directive.is("error_log") {
                return vec![];
            }
        }

        // No error_log found
        vec![LintError::info(
            "missing-error-log",
            "best-practices",
            "Consider configuring error_log for debugging",
            0,
            0,
        )]
    }
}

// Export the plugin
nginx_lint::export_plugin!(MissingErrorLogPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::PluginTestRunner;

    #[test]
    fn test_no_error_log() {
        let runner = PluginTestRunner::new(MissingErrorLogPlugin);

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
    fn test_with_error_log() {
        let runner = PluginTestRunner::new(MissingErrorLogPlugin);

        runner.assert_no_errors(
            r#"
error_log /var/log/nginx/error.log warn;
http {
    server {
        listen 80;
    }
}
"#,
        );
    }

    #[test]
    fn test_error_log_in_http_block() {
        let runner = PluginTestRunner::new(MissingErrorLogPlugin);

        runner.assert_no_errors(
            r#"
http {
    error_log /var/log/nginx/http.log;
    server {
        listen 80;
    }
}
"#,
        );
    }

    #[test]
    fn test_error_log_in_server_block() {
        let runner = PluginTestRunner::new(MissingErrorLogPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        listen 80;
        error_log /var/log/nginx/server.log;
    }
}
"#,
        );
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(MissingErrorLogPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }
}
