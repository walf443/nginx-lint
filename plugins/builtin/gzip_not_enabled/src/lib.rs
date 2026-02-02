//! gzip-not-enabled plugin
//!
//! This plugin suggests enabling gzip compression for better performance.
//! Gzip compression significantly reduces response sizes and improves
//! page load times.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;

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
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut gzip_on = false;

        for directive in config.all_directives() {
            if directive.is("gzip") && directive.first_arg_is("on") {
                gzip_on = true;
                break;
            }
        }

        if !gzip_on {
            vec![LintError::info(
                "gzip-not-enabled",
                "best-practices",
                "Consider enabling gzip compression for better performance",
                0,
                0,
            )]
        } else {
            vec![]
        }
    }
}

// Export the plugin
nginx_lint::export_plugin!(GzipNotEnabledPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::PluginTestRunner;

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
}
