//! proxy-missing-host-header plugin
//!
//! This plugin warns when proxy_pass is used in a block but
//! proxy_set_header Host is not configured in the same block.
//!
//! Without setting the Host header, the backend receives the upstream
//! hostname instead of the original client hostname, which breaks
//! virtual host routing.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check if proxy_pass has corresponding Host header setting
#[derive(Default)]
pub struct ProxyMissingHostHeaderPlugin;

impl ProxyMissingHostHeaderPlugin {
    /// Check if a block has proxy_set_header Host
    fn has_host_header(block: &Block) -> bool {
        for item in &block.items {
            if let ConfigItem::Directive(directive) = item
                && directive.name == "proxy_set_header"
                && let Some(header_name) = directive.first_arg()
                && header_name.eq_ignore_ascii_case("host")
            {
                return true;
            }
        }
        false
    }

    /// Check a block for proxy_pass without Host header
    ///
    /// `parent_has_host` indicates whether any ancestor block already has
    /// `proxy_set_header Host`, so child blocks can inherit it.
    fn check_block(
        &self,
        items: &[ConfigItem],
        parent_has_host: bool,
        errors: &mut Vec<LintError>,
    ) {
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                // Check if this directive has a block
                if let Some(block) = &directive.block {
                    let block_has_host = Self::has_host_header(block);
                    let effective_has_host = parent_has_host || block_has_host;

                    // Find proxy_pass in this block
                    let mut proxy_pass_directive: Option<&Directive> = None;

                    for block_item in &block.items {
                        if let ConfigItem::Directive(d) = block_item
                            && d.name == "proxy_pass"
                        {
                            proxy_pass_directive = Some(d);
                            break;
                        }
                    }

                    // If we found proxy_pass, check for Host header
                    if let Some(pass_directive) = proxy_pass_directive
                        && !effective_has_host
                    {
                        let err =
                            PluginSpec::new("proxy-missing-host-header", "best-practices", "")
                                .error_builder();

                        let error = err
                            .warning_at(
                                "proxy_pass is set but proxy_set_header Host is not configured. \
                                 Without the Host header, the backend may not receive the correct \
                                 hostname. Add 'proxy_set_header Host $host;'",
                                pass_directive,
                            )
                            .with_fix(pass_directive.insert_after("proxy_set_header Host $host;"));

                        errors.push(error);
                    }

                    // Recursively check nested blocks
                    self.check_block(&block.items, effective_has_host, errors);
                }
            }
        }
    }
}

impl ProxyMissingHostHeaderPlugin {
    /// Check top-level items when included from a block context
    fn check_top_level(&self, items: &[ConfigItem], errors: &mut Vec<LintError>) {
        // Find proxy_pass in top-level items
        let mut proxy_pass_directive: Option<&Directive> = None;

        for item in items {
            if let ConfigItem::Directive(d) = item
                && d.name == "proxy_pass"
            {
                proxy_pass_directive = Some(d);
                break;
            }
        }

        // If we found proxy_pass, check for Host header
        if let Some(pass_directive) = proxy_pass_directive {
            let has_host = items.iter().any(|ci| {
                if let ConfigItem::Directive(d) = ci
                    && d.name == "proxy_set_header"
                    && let Some(header_name) = d.first_arg()
                {
                    return header_name.eq_ignore_ascii_case("host");
                }
                false
            });

            if !has_host {
                let err = PluginSpec::new("proxy-missing-host-header", "best-practices", "")
                    .error_builder();

                let error = err
                    .warning_at(
                        "proxy_pass is set but proxy_set_header Host is not configured. \
                     Without the Host header, the backend may not receive the correct \
                     hostname. Add 'proxy_set_header Host $host;'",
                        pass_directive,
                    )
                    .with_fix(pass_directive.insert_after("proxy_set_header Host $host;"));

                errors.push(error);
            }
        }
    }
}

impl Plugin for ProxyMissingHostHeaderPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "proxy-missing-host-header",
            "best-practices",
            "Warns when proxy_pass is used without proxy_set_header Host",
        )
        .with_severity("warning")
        .with_why(
            "When using proxy_pass, the default Host header sent to the backend is \
             $proxy_host (the host and port from the proxy_pass URL). This breaks \
             virtual host routing on the backend. Setting 'proxy_set_header Host $host;' \
             forwards the original client hostname to the backend.\n\n\
             Common values:\n\
             - $host: the hostname from the request line or Host header\n\
             - $http_host: the Host header as sent by the client (includes port)",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_set_header"
                .to_string(),
            "https://github.com/walf443/nginx-lint/blob/main/plugins/builtin/best_practices/proxy_missing_host_header/tests/container_test.rs".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        // If included from a block context (server, location, http), check top-level items
        if config.is_included_from_http() {
            self.check_top_level(&config.items, &mut errors);
        }

        self.check_block(&config.items, false, &mut errors);
        errors
    }
}

nginx_lint_plugin::export_component_plugin!(ProxyMissingHostHeaderPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::parse_string;
    use nginx_lint_plugin::testing::PluginTestRunner;

    #[test]
    fn test_missing_host_header() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyMissingHostHeaderPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("proxy_pass"));
        assert!(errors[0].message.contains("Host"));
    }

    #[test]
    fn test_with_host_header() {
        let runner = PluginTestRunner::new(ProxyMissingHostHeaderPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            proxy_pass http://backend;
            proxy_set_header Host $host;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_with_http_host() {
        let runner = PluginTestRunner::new(ProxyMissingHostHeaderPlugin);

        // $http_host is also a valid value for Host header
        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            proxy_pass http://backend;
            proxy_set_header Host $http_host;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_case_insensitive_header() {
        let runner = PluginTestRunner::new(ProxyMissingHostHeaderPlugin);

        // Header name should be case-insensitive
        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            proxy_pass http://backend;
            proxy_set_header host $host;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_proxy_pass_with_variable() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            set $backend "http://upstream";
            proxy_pass $backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyMissingHostHeaderPlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should still warn even when proxy_pass uses a variable
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_no_proxy_pass() {
        let runner = PluginTestRunner::new(ProxyMissingHostHeaderPlugin);

        // No proxy_pass means no warning
        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            root /var/www/html;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_missing_host_header_with_fix() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyMissingHostHeaderPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(!errors[0].fixes.is_empty());

        let fix = &errors[0].fixes[0];
        assert!(
            fix.new_text.contains("proxy_set_header Host $host"),
            "Fix should contain Host header: {}",
            fix.new_text
        );
    }

    #[test]
    fn test_multiple_locations() {
        let config = parse_string(
            r#"
http {
    server {
        location /api {
            proxy_pass http://api-backend;
        }

        location /web {
            proxy_pass http://web-backend;
            proxy_set_header Host $host;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyMissingHostHeaderPlugin;
        let errors = plugin.check(&config, "test.conf");

        // Only /api should have warning
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_other_proxy_set_header_without_host() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            proxy_pass http://backend;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyMissingHostHeaderPlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should still warn because Host is not set, even though other headers are
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_proxy_pass_at_server_level() {
        let config = parse_string(
            r#"
http {
    server {
        proxy_pass http://backend;
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyMissingHostHeaderPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_host_header_in_server_block() {
        let runner = PluginTestRunner::new(ProxyMissingHostHeaderPlugin);

        // proxy_set_header Host at server level should suppress warnings for child locations
        runner.assert_no_errors(
            r#"
http {
    server {
        proxy_set_header Host $host;

        location /api {
            proxy_pass http://api-backend;
        }

        location /web {
            proxy_pass http://web-backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_host_header_in_http_block() {
        let runner = PluginTestRunner::new(ProxyMissingHostHeaderPlugin);

        // proxy_set_header Host at http level should suppress warnings for all nested blocks
        runner.assert_no_errors(
            r#"
http {
    proxy_set_header Host $host;

    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    // =========================================================================
    // Include context tests
    // =========================================================================

    #[test]
    fn test_include_context_from_location() {
        // Test that proxy_pass without Host header is detected when included from location
        let mut config = parse_string(
            r#"
proxy_pass http://backend;
"#,
        )
        .unwrap();

        // Simulate being included from http > server > location context
        config.include_context = vec![
            "http".to_string(),
            "server".to_string(),
            "location".to_string(),
        ];

        let plugin = ProxyMissingHostHeaderPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(
            errors.len(),
            1,
            "Expected 1 error for proxy_pass without Host, got: {:?}",
            errors
        );
        assert!(errors[0].message.contains("proxy_pass"));
        assert!(errors[0].message.contains("Host"));
    }

    #[test]
    fn test_include_context_with_host_header() {
        // Test that no error when Host header is present in included file
        let mut config = parse_string(
            r#"
proxy_pass http://backend;
proxy_set_header Host $host;
"#,
        )
        .unwrap();

        // Simulate being included from http > server > location context
        config.include_context = vec![
            "http".to_string(),
            "server".to_string(),
            "location".to_string(),
        ];

        let plugin = ProxyMissingHostHeaderPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert!(
            errors.is_empty(),
            "Expected no errors when Host header is set, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_no_include_context_no_error() {
        // Test that top-level directives without include context don't trigger
        let config = parse_string(
            r#"
proxy_pass http://backend;
"#,
        )
        .unwrap();

        let plugin = ProxyMissingHostHeaderPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert!(
            errors.is_empty(),
            "Expected no errors without include context, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(ProxyMissingHostHeaderPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(ProxyMissingHostHeaderPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
