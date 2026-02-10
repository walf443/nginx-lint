//! try-files-with-proxy plugin
//!
//! This plugin warns when both try_files and proxy_pass are used in the same
//! location block. When combined, proxy_pass becomes the content handler and
//! try_files only rewrites the URI - static files are never served from disk.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check if try_files and proxy_pass are used together incorrectly
#[derive(Default)]
pub struct TryFilesWithProxyPlugin;

impl TryFilesWithProxyPlugin {
    /// Check a block for try_files + proxy_pass combination
    fn check_block(&self, items: &[ConfigItem], errors: &mut Vec<LintError>) {
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                // Only check location blocks
                if directive.is("location") {
                    if let Some(block) = &directive.block {
                        self.check_location_items(&block.items, errors);
                    }
                }

                // Recursively check nested blocks (server, http, etc.)
                if let Some(block) = &directive.block {
                    self.check_block(&block.items, errors);
                }
            }
        }
    }

    /// Check items inside a location for try_files + proxy_pass
    fn check_location_items(&self, items: &[ConfigItem], errors: &mut Vec<LintError>) {
        let mut try_files_directive: Option<&Directive> = None;
        let mut proxy_pass_directive: Option<&Directive> = None;

        for item in items {
            if let ConfigItem::Directive(directive) = item {
                if directive.is("try_files") {
                    try_files_directive = Some(directive);
                } else if directive.is("proxy_pass") {
                    proxy_pass_directive = Some(directive);
                }

                // Also check nested blocks like if
                if let Some(nested_block) = &directive.block {
                    // Check for proxy_pass inside if blocks within this location
                    for nested_item in &nested_block.items {
                        if let ConfigItem::Directive(nested_directive) = nested_item {
                            if nested_directive.is("proxy_pass") && try_files_directive.is_some() {
                                // proxy_pass inside if, but try_files outside - still a problem
                                // unless the if is specifically handling the fallback
                            }
                        }
                    }
                }
            }
        }

        // If both are present in the same location, warn
        if let (Some(try_files), Some(proxy_pass)) = (try_files_directive, proxy_pass_directive) {
            // Check if try_files has a named location fallback (like @backend)
            // In that case, it might be intentional
            let has_named_location_fallback = try_files
                .args
                .iter()
                .any(|arg| arg.as_str().starts_with('@'));

            if has_named_location_fallback {
                // This is likely intentional - try_files with named location fallback
                // and proxy_pass is probably in a different location
                return;
            }

            let err = PluginSpec::new("try-files-with-proxy", "best-practices", "").error_builder();

            errors.push(err.warning_at(
                "try_files and proxy_pass in the same location: proxy_pass becomes the content handler \
                 and try_files only rewrites the URI. Static files will never be served from disk. \
                 Use a named location (@fallback) for proxy_pass",
                proxy_pass,
            ));
        }
    }
}

impl Plugin for TryFilesWithProxyPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "try-files-with-proxy",
            "best-practices",
            "Warns when try_files and proxy_pass are used in the same location block",
        )
        .with_severity("warning")
        .with_why(
            "When both try_files and proxy_pass are in the same location block, \
             proxy_pass becomes the content handler and try_files only performs URI \
             rewriting. This means:\n\
             1. Static files are never served directly from disk, even if they exist\n\
             2. All requests are proxied to the upstream server\n\
             3. try_files only rewrites the URI (e.g., to a fallback like /index.html) \
             before proxy_pass forwards it\n\n\
             To serve static files locally and proxy only when no file is found, \
             use a named location:\n\
             try_files $uri $uri/ @backend;\n\
             And define @backend with proxy_pass separately.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_core_module.html#try_files".to_string(),
            "https://www.nginx.com/resources/wiki/start/topics/tutorials/config_pitfalls/#proxy-pass-and-try-files".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        // If included from a location context, check top-level items directly
        if config.is_included_from_http_location() {
            self.check_location_items(&config.items, &mut errors);
        }

        self.check_block(&config.items, &mut errors);
        errors
    }
}

// Export the plugin
nginx_lint_plugin::export_plugin!(TryFilesWithProxyPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::parse_string;
    use nginx_lint_plugin::testing::PluginTestRunner;

    #[test]
    fn test_try_files_with_proxy_pass() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            try_files $uri $uri/ /index.html;
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = TryFilesWithProxyPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("try_files"));
        assert!(errors[0].message.contains("proxy_pass"));
    }

    #[test]
    fn test_try_files_with_named_location_fallback() {
        let runner = PluginTestRunner::new(TryFilesWithProxyPlugin);

        // Named location fallback is OK - proxy_pass is in @backend
        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            try_files $uri $uri/ @backend;
        }

        location @backend {
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_try_files_with_error_code() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            try_files $uri $uri/ =404;
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = TryFilesWithProxyPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0]
                .message
                .contains("proxy_pass becomes the content handler")
        );
    }

    #[test]
    fn test_only_proxy_pass() {
        let runner = PluginTestRunner::new(TryFilesWithProxyPlugin);

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
    fn test_only_try_files() {
        let runner = PluginTestRunner::new(TryFilesWithProxyPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            try_files $uri $uri/ /index.html;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_separate_locations() {
        let runner = PluginTestRunner::new(TryFilesWithProxyPlugin);

        // Different locations are fine
        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            try_files $uri $uri/ /index.html;
        }

        location /api {
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_nested_location() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            location /nested {
                try_files $uri $uri/ /index.html;
                proxy_pass http://backend;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = TryFilesWithProxyPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error for nested location");
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(TryFilesWithProxyPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    // =========================================================================
    // Include context tests
    // =========================================================================

    #[test]
    fn test_include_context_from_location() {
        // Test that try_files + proxy_pass is detected when file is included from a location block
        let mut config = parse_string(
            r#"
try_files $uri $uri/ /index.html;
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

        let plugin = TryFilesWithProxyPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(
            errors.len(),
            1,
            "Expected 1 error for try_files+proxy_pass in included file from location, got: {:?}",
            errors
        );
        assert!(errors[0].message.contains("try_files"));
        assert!(errors[0].message.contains("proxy_pass"));
    }

    #[test]
    fn test_include_context_from_server_no_error() {
        // Test that try_files + proxy_pass at server level (not location) doesn't trigger
        // (though this is still not a valid nginx config, the rule specifically checks location blocks)
        let mut config = parse_string(
            r#"
try_files $uri $uri/ /index.html;
proxy_pass http://backend;
"#,
        )
        .unwrap();

        // Simulate being included from http > server context (not location)
        config.include_context = vec!["http".to_string(), "server".to_string()];

        let plugin = TryFilesWithProxyPlugin;
        let errors = plugin.check(&config, "test.conf");

        // This should NOT trigger because we're not in a location context
        assert!(
            errors.is_empty(),
            "Expected no errors for try_files+proxy_pass in server context, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_include_context_with_named_location_fallback() {
        // Test that named location fallback is still OK when included from location
        let mut config = parse_string(
            r#"
try_files $uri $uri/ @backend;
"#,
        )
        .unwrap();

        // Simulate being included from http > server > location context
        config.include_context = vec![
            "http".to_string(),
            "server".to_string(),
            "location".to_string(),
        ];

        let plugin = TryFilesWithProxyPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert!(
            errors.is_empty(),
            "Expected no errors for try_files with named location, got: {:?}",
            errors
        );
    }
}
