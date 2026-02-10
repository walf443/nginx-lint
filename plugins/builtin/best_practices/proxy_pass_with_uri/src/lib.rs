//! proxy-pass-with-uri plugin
//!
//! This plugin warns when `proxy_pass` directive has a URI path component.
//!
//! When proxy_pass has a URI (including just `/`), nginx replaces the matched
//! location part with the proxy_pass URI. This can lead to unexpected behavior
//! if not intentional.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check for proxy_pass with URI path
#[derive(Default)]
pub struct ProxyPassWithUriPlugin;

impl ProxyPassWithUriPlugin {
    /// Check if a string contains a variable reference like $1, $uri, etc.
    fn contains_variable(s: &str) -> bool {
        if let Some(dollar_pos) = s.find('$') {
            // Check if there's at least one alphanumeric or underscore after $
            let after_dollar = &s[dollar_pos + 1..];
            after_dollar
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        } else {
            false
        }
    }

    /// Check if a proxy_pass URL has a URI/path component
    /// Returns Some(path) if it has a static path (no variables), None otherwise
    fn extract_uri_path(url: &str) -> Option<&str> {
        // Handle variable-only URLs (e.g., $upstream)
        if url.starts_with('$') {
            return None;
        }

        // Find the scheme (http:// or https://)
        let after_scheme = if let Some(pos) = url.find("://") {
            &url[pos + 3..]
        } else {
            // No scheme, might be a variable or unix socket
            return None;
        };

        // Find the first / after the host:port
        // This marks the start of the URI path
        if let Some(slash_pos) = after_scheme.find('/') {
            let path = &after_scheme[slash_pos..];
            // Return the path if it's not empty and doesn't contain variables
            if !path.is_empty() && !Self::contains_variable(path) {
                return Some(path);
            }
        }

        None
    }

    /// Check if any argument is a variable
    fn has_variable_arg(directive: &Directive) -> bool {
        directive.args.iter().any(|arg| arg.is_variable())
    }

    /// Recursively check for proxy_pass with URI
    fn check_items(&self, items: &[ConfigItem], errors: &mut Vec<LintError>) {
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                if directive.name == "proxy_pass" {
                    // Skip if any argument is a variable (e.g., http://backend/$1)
                    // Variables indicate intentional URI manipulation
                    if Self::has_variable_arg(directive) {
                        if let Some(block) = &directive.block {
                            self.check_items(&block.items, errors);
                        }
                        continue;
                    }

                    if let Some(url) = directive.first_arg()
                        && let Some(path) = Self::extract_uri_path(url)
                    {
                        let message = if path == "/" {
                            format!(
                                "proxy_pass '{}' has trailing slash which causes URI rewriting; \
                                     use '# nginx-lint:ignore' if this is intentional",
                                url
                            )
                        } else {
                            format!(
                                "proxy_pass '{}' has URI path '{}' which causes URI rewriting; \
                                     use '# nginx-lint:ignore' if this is intentional",
                                url, path
                            )
                        };

                        let err = PluginSpec::new("proxy-pass-with-uri", "best-practices", "")
                            .error_builder();

                        errors.push(err.warning_at(&message, directive));
                    }
                }

                // Recurse into blocks
                if let Some(block) = &directive.block {
                    self.check_items(&block.items, errors);
                }
            }
        }
    }
}

impl Plugin for ProxyPassWithUriPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "proxy-pass-with-uri",
            "best-practices",
            "Warns when proxy_pass has a URI path that causes URI rewriting",
        )
        .with_severity("warning")
        .with_why(
            "When `proxy_pass` has a URI component (including just `/`), nginx performs URI \
             rewriting - it replaces the matched location prefix with the proxy_pass URI. \
             This behavior can be confusing and lead to unexpected results.\n\n\
             For example:\n\
             - `location /api/ { proxy_pass http://backend/; }` rewrites `/api/foo` to `/foo`\n\
             - `location /api/ { proxy_pass http://backend; }` keeps `/api/foo` as-is\n\n\
             If URI rewriting is intentional, use `# nginx-lint:ignore proxy-pass-with-uri -- reason` \
             to suppress this warning.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_pass".to_string(),
            "https://github.com/walf443/nginx-lint/blob/main/plugins/builtin/best_practices/proxy_pass_with_uri/tests/container_test.rs".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        self.check_items(&config.items, &mut errors);
        errors
    }
}

// Export the plugin
nginx_lint_plugin::export_plugin!(ProxyPassWithUriPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::parse_string;
    use nginx_lint_plugin::testing::PluginTestRunner;

    #[test]
    fn test_proxy_pass_with_trailing_slash_warns() {
        let config = parse_string(
            r#"
http {
    server {
        location /api/ {
            proxy_pass http://backend/;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyPassWithUriPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("trailing slash"));
    }

    #[test]
    fn test_proxy_pass_with_path_warns() {
        let config = parse_string(
            r#"
http {
    server {
        location /api/ {
            proxy_pass http://backend/v1/;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyPassWithUriPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("/v1/"));
    }

    #[test]
    fn test_proxy_pass_without_uri_ok() {
        let runner = PluginTestRunner::new(ProxyPassWithUriPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location /api/ {
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_proxy_pass_with_port_no_uri_ok() {
        let runner = PluginTestRunner::new(ProxyPassWithUriPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location /api/ {
            proxy_pass http://backend:8080;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_proxy_pass_with_variable_ok() {
        let runner = PluginTestRunner::new(ProxyPassWithUriPlugin);

        runner.assert_no_errors(
            r#"
http {
    upstream backend {
        server 127.0.0.1:8080;
    }

    server {
        location /api/ {
            proxy_pass $scheme://$host$request_uri;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_proxy_pass_https_with_slash_warns() {
        let config = parse_string(
            r#"
http {
    server {
        location /api/ {
            proxy_pass https://backend/;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyPassWithUriPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_multiple_proxy_pass_warns() {
        let config = parse_string(
            r#"
http {
    server {
        location /api/ {
            proxy_pass http://backend/;
        }

        location /web/ {
            proxy_pass http://frontend/app/;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxyPassWithUriPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
    }

    #[test]
    fn test_proxy_pass_with_capture_group_ok() {
        let runner = PluginTestRunner::new(ProxyPassWithUriPlugin);

        // Using $1 capture group is intentional
        runner.assert_no_errors(
            r#"
http {
    server {
        location ~ ^/api/(.*)$ {
            proxy_pass http://backend/$1;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_proxy_pass_with_request_uri_ok() {
        let runner = PluginTestRunner::new(ProxyPassWithUriPlugin);

        // Using $request_uri is intentional
        runner.assert_no_errors(
            r#"
http {
    server {
        location /api/ {
            proxy_pass http://backend$request_uri;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(ProxyPassWithUriPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(ProxyPassWithUriPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
