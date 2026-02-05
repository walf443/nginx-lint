//! try-files-with-proxy plugin
//!
//! This plugin warns when both try_files and proxy_pass are used in the same
//! location block. try_files takes precedence and proxy_pass will only be
//! executed if try_files falls through to a named location.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;

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
                        self.check_location_block(block, errors);
                    }
                }

                // Recursively check nested blocks (server, http, etc.)
                if let Some(block) = &directive.block {
                    self.check_block(&block.items, errors);
                }
            }
        }
    }

    /// Check a location block for try_files + proxy_pass
    fn check_location_block(&self, block: &Block, errors: &mut Vec<LintError>) {
        let mut try_files_directive: Option<&Directive> = None;
        let mut proxy_pass_directive: Option<&Directive> = None;

        for item in &block.items {
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

            // Check if try_files ends with =404 or similar error code
            let ends_with_error_code = try_files
                .args
                .last()
                .map(|arg| arg.as_str().starts_with('='))
                .unwrap_or(false);

            let err = PluginInfo::new(
                "try-files-with-proxy",
                "best-practices",
                "",
            ).error_builder();

            errors.push(err.warning_at(
                if ends_with_error_code {
                    "try_files and proxy_pass in the same location: try_files takes precedence, \
                     proxy_pass will never be executed. Use a named location (@fallback) for proxy_pass"
                } else {
                    "try_files and proxy_pass in the same location: try_files takes precedence. \
                     If the last try_files argument is a URI, it will be used instead of proxy_pass"
                },
                proxy_pass,
            ));
        }
    }
}

impl Plugin for TryFilesWithProxyPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "try-files-with-proxy",
            "best-practices",
            "Warns when try_files and proxy_pass are used in the same location block",
        )
        .with_severity("warning")
        .with_why(
            "When both try_files and proxy_pass are in the same location block, \
             try_files takes precedence. The proxy_pass directive will never be executed \
             because try_files will either:\n\
             1. Find a matching file and serve it\n\
             2. Use the last argument as a fallback URI or return an error code\n\n\
             To proxy requests that don't match static files, use a named location:\n\
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
        self.check_block(&config.items, &mut errors);
        errors
    }
}

// Export the plugin
nginx_lint::export_plugin!(TryFilesWithProxyPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::PluginTestRunner;
    use nginx_lint::parse_string;

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
        assert!(errors[0].message.contains("never be executed"));
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
}
