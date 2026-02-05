//! proxy-keepalive plugin
//!
//! This plugin warns when proxy_http_version is set to 1.1 or higher
//! but proxy_set_header Connection is not set in the same block.
//!
//! When using HTTP/1.1 with upstream servers for keepalive connections,
//! the Connection header should be cleared to prevent the default "close"
//! value from being passed to the upstream.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;

/// Check if proxy_http_version 1.1+ has corresponding Connection header setting
#[derive(Default)]
pub struct ProxyKeepalivePlugin;

impl ProxyKeepalivePlugin {
    /// Check if a version string is not 1.0 (i.e., requires Connection header for keepalive)
    fn is_http_11_or_higher(version: &str) -> bool {
        version != "1.0"
    }

    /// Check if a block has proxy_set_header Connection
    fn has_connection_header(block: &Block) -> bool {
        for item in &block.items {
            if let ConfigItem::Directive(directive) = item {
                if directive.name == "proxy_set_header" {
                    if let Some(header_name) = directive.first_arg() {
                        if header_name.eq_ignore_ascii_case("connection") {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Check a block for proxy_http_version without Connection header
    fn check_block(&self, items: &[ConfigItem], errors: &mut Vec<LintError>) {
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                // Check if this directive has a block
                if let Some(block) = &directive.block {
                    // Find proxy_http_version 1.1+ in this block
                    let mut http_version_directive: Option<&Directive> = None;

                    for block_item in &block.items {
                        if let ConfigItem::Directive(d) = block_item {
                            if d.name == "proxy_http_version" {
                                if let Some(version) = d.first_arg() {
                                    if Self::is_http_11_or_higher(version) {
                                        http_version_directive = Some(d);
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // If we found proxy_http_version 1.1+, check for Connection header
                    if let Some(version_directive) = http_version_directive {
                        if !Self::has_connection_header(block) {
                            let version = version_directive.first_arg().unwrap_or("1.1");

                            // Calculate fix: add proxy_set_header Connection ""; after proxy_http_version
                            let indent = " ".repeat(version_directive.span.start.column.saturating_sub(1));
                            let fix_text = format!("\n{}proxy_set_header Connection \"\";", indent);

                            // Insert after the proxy_http_version line (at end of directive)
                            let insert_offset = version_directive.span.end.offset;

                            let err = PluginInfo::new(
                                "proxy-keepalive",
                                "best-practices",
                                "",
                            ).error_builder();

                            let mut error = err.warning_at(
                                &format!(
                                    "proxy_http_version {} is set but proxy_set_header Connection is not configured. \
                                     For keepalive connections with upstream, add 'proxy_set_header Connection \"\";'",
                                    version
                                ),
                                version_directive,
                            );

                            error = error.with_fix(Fix::replace_range(
                                insert_offset,
                                insert_offset,
                                &fix_text,
                            ));

                            errors.push(error);
                        }
                    }

                    // Recursively check nested blocks
                    self.check_block(&block.items, errors);
                }
            }
        }
    }
}

impl Plugin for ProxyKeepalivePlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "proxy-keepalive",
            "best-practices",
            "Warns when proxy_http_version 1.1+ is set without proxy_set_header Connection",
        )
        .with_severity("warning")
        .with_why(
            "When using HTTP/1.1 or higher with upstream servers, the Connection header \
             should be cleared to enable keepalive connections. Without this, the default \
             'close' value may be passed to the upstream, preventing connection reuse.\n\n\
             This is especially important when using the 'keepalive' directive in upstream blocks.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_http_version".to_string(),
            "https://nginx.org/en/docs/http/ngx_http_upstream_module.html#keepalive".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        self.check_block(&config.items, &mut errors);
        errors
    }
}

// Export the plugin
nginx_lint::export_plugin!(ProxyKeepalivePlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::PluginTestRunner;
    use nginx_lint::parse_string;

    #[test]
    fn test_missing_connection_header() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            proxy_http_version 1.1;
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyKeepalivePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("proxy_http_version 1.1"));
        assert!(errors[0].message.contains("Connection"));
    }

    #[test]
    fn test_missing_connection_header_with_fix() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            proxy_http_version 1.1;
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyKeepalivePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].fix.is_some());

        let fix = errors[0].fix.as_ref().unwrap();
        assert!(
            fix.new_text.contains("proxy_set_header Connection"),
            "Fix should contain Connection header: {}",
            fix.new_text
        );
    }

    #[test]
    fn test_with_connection_header() {
        let runner = PluginTestRunner::new(ProxyKeepalivePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            proxy_http_version 1.1;
            proxy_set_header Connection "";
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_with_connection_header_upgrade() {
        let runner = PluginTestRunner::new(ProxyKeepalivePlugin);

        // Connection "upgrade" is also valid (for WebSocket)
        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            proxy_http_version 1.1;
            proxy_set_header Upgrade $http_upgrade;
            proxy_set_header Connection "upgrade";
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_http_10_no_warning() {
        let runner = PluginTestRunner::new(ProxyKeepalivePlugin);

        // HTTP/1.0 doesn't need Connection header for keepalive
        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            proxy_http_version 1.0;
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_proxy_http_version() {
        let runner = PluginTestRunner::new(ProxyKeepalivePlugin);

        // No proxy_http_version means default 1.0, no warning needed
        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_server_level() {
        let config = parse_string(
            r#"
http {
    server {
        proxy_http_version 1.1;

        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyKeepalivePlugin;
        let errors = plugin.check(&config, "test.conf");

        // Warning at server level
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_server_level_with_connection() {
        let runner = PluginTestRunner::new(ProxyKeepalivePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        proxy_http_version 1.1;
        proxy_set_header Connection "";

        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_http_level() {
        let config = parse_string(
            r#"
http {
    proxy_http_version 1.1;

    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyKeepalivePlugin;
        let errors = plugin.check(&config, "test.conf");

        // Warning at http level
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_multiple_locations() {
        let config = parse_string(
            r#"
http {
    server {
        location /api {
            proxy_http_version 1.1;
            proxy_pass http://api-backend;
        }

        location /web {
            proxy_http_version 1.1;
            proxy_set_header Connection "";
            proxy_pass http://web-backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyKeepalivePlugin;
        let errors = plugin.check(&config, "test.conf");

        // Only /api should have warning
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_case_insensitive_header() {
        let runner = PluginTestRunner::new(ProxyKeepalivePlugin);

        // Header name should be case-insensitive
        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            proxy_http_version 1.1;
            proxy_set_header connection "";
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_http_2() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            proxy_http_version 2.0;
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyKeepalivePlugin;
        let errors = plugin.check(&config, "test.conf");

        // HTTP/2 also needs Connection header cleared
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(ProxyKeepalivePlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }
}
