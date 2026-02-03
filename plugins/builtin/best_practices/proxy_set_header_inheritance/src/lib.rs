//! proxy-set-header-inheritance plugin
//!
//! This plugin warns when proxy_set_header is used in a child block without
//! explicitly including headers that were set in the parent block.
//!
//! In nginx, proxy_set_header directives in a child block completely override
//! those in the parent block - they are NOT inherited. This is a common source
//! of bugs where headers set at the server level are lost in location blocks.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;
use std::collections::HashSet;

/// Check if proxy_set_header in child blocks includes all parent headers
#[derive(Default)]
pub struct ProxySetHeaderInheritancePlugin;

impl ProxySetHeaderInheritancePlugin {
    /// Collect proxy_set_header headers from a block's direct children (not nested)
    fn collect_headers_from_block(block: &Block) -> HashSet<String> {
        let mut headers = HashSet::new();
        for item in &block.items {
            if let ConfigItem::Directive(directive) = item {
                if directive.name == "proxy_set_header" {
                    if let Some(header_name) = directive.first_arg() {
                        headers.insert(header_name.to_lowercase());
                    }
                }
            }
        }
        headers
    }

    /// Check a block for proxy_set_header inheritance issues
    fn check_block(
        &self,
        items: &[ConfigItem],
        parent_headers: &HashSet<String>,
        errors: &mut Vec<LintError>,
    ) {
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                // Check if this is a block that can contain proxy_set_header
                if let Some(block) = &directive.block {
                    // Only check server, location, if, limit_except blocks
                    let is_proxy_context = matches!(
                        directive.name.as_str(),
                        "server" | "location" | "if" | "limit_except"
                    );

                    if is_proxy_context {
                        // Collect headers defined in this block
                        let current_headers = Self::collect_headers_from_block(block);

                        // If this block has any proxy_set_header, check for missing parent headers
                        if !current_headers.is_empty() && !parent_headers.is_empty() {
                            let missing: Vec<_> = parent_headers
                                .iter()
                                .filter(|h| !current_headers.contains(*h))
                                .cloned()
                                .collect();

                            if !missing.is_empty() {
                                // Sort for consistent output
                                let mut missing_sorted: Vec<_> = missing.iter().collect();
                                missing_sorted.sort();

                                // Find the first proxy_set_header in this block for error location
                                let first_header_line = block
                                    .items
                                    .iter()
                                    .filter_map(|item| {
                                        if let ConfigItem::Directive(d) = item {
                                            if d.name == "proxy_set_header" {
                                                return Some((d.span.start.line, d.span.start.column));
                                            }
                                        }
                                        None
                                    })
                                    .next()
                                    .unwrap_or((directive.span.start.line, directive.span.start.column));

                                errors.push(LintError::warning(
                                    "proxy-set-header-inheritance",
                                    "best-practices",
                                    &format!(
                                        "proxy_set_header in this block does not include headers from parent block: {}. \
                                         In nginx, proxy_set_header directives are not inherited - \
                                         all headers must be explicitly repeated in child blocks",
                                        missing_sorted
                                            .iter()
                                            .map(|s| format!("'{}'", s))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    ),
                                    first_header_line.0,
                                    first_header_line.1,
                                ));
                            }
                        }

                        // Merge parent and current headers for nested blocks
                        let merged_headers: HashSet<String> = parent_headers
                            .union(&current_headers)
                            .cloned()
                            .collect();

                        // Recursively check nested blocks
                        self.check_block(&block.items, &merged_headers, errors);
                    } else if directive.name == "http" {
                        // For http block, collect headers and pass to children
                        let current_headers = Self::collect_headers_from_block(block);
                        self.check_block(&block.items, &current_headers, errors);
                    } else {
                        // For other blocks (upstream, etc.), continue with same parent headers
                        self.check_block(&block.items, parent_headers, errors);
                    }
                }
            }
        }
    }
}

impl Plugin for ProxySetHeaderInheritancePlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "proxy-set-header-inheritance",
            "best-practices",
            "Warns when proxy_set_header in child blocks doesn't include parent headers",
        )
        .with_severity("warning")
        .with_why(
            "In nginx, proxy_set_header directives in a child block (like location) completely \
             override those in the parent block (like server) - they are NOT inherited. \
             This is a common source of bugs where important headers like Host, X-Real-IP, \
             or X-Forwarded-For are unintentionally lost.\n\n\
             When using proxy_set_header in a child block, you must explicitly repeat all \
             headers that were set in the parent block.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_set_header".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        // Start with empty parent headers at root level
        self.check_block(&config.items, &HashSet::new(), &mut errors);

        errors
    }
}

// Export the plugin
nginx_lint::export_plugin!(ProxySetHeaderInheritancePlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::PluginTestRunner;
    use nginx_lint::parse_string;

    #[test]
    fn test_missing_parent_headers() {
        let config = parse_string(
            r#"
http {
    server {
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;

        location / {
            proxy_set_header X-Custom "value";
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxySetHeaderInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("host"));
        assert!(errors[0].message.contains("x-real-ip"));
    }

    #[test]
    fn test_all_headers_included() {
        let runner = PluginTestRunner::new(ProxySetHeaderInheritancePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;

        location / {
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Custom "value";
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_parent_headers() {
        let runner = PluginTestRunner::new(ProxySetHeaderInheritancePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            proxy_set_header X-Custom "value";
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_child_headers() {
        let runner = PluginTestRunner::new(ProxySetHeaderInheritancePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;

        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_http_level_headers() {
        let config = parse_string(
            r#"
http {
    proxy_set_header Host $host;

    server {
        location / {
            proxy_set_header X-Custom "value";
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxySetHeaderInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("host"));
    }

    #[test]
    fn test_nested_location() {
        let config = parse_string(
            r#"
http {
    server {
        proxy_set_header Host $host;

        location /api {
            proxy_set_header X-API "true";

            location /api/v2 {
                proxy_set_header X-V2 "true";
                proxy_pass http://backend;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxySetHeaderInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should warn for both /api (missing host) and /api/v2 (missing host, x-api)
        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
    }

    #[test]
    fn test_case_insensitive() {
        let runner = PluginTestRunner::new(ProxySetHeaderInheritancePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        proxy_set_header Host $host;

        location / {
            proxy_set_header HOST $host;
            proxy_set_header X-Custom "value";
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_if_block() {
        let config = parse_string(
            r#"
http {
    server {
        proxy_set_header Host $host;

        location / {
            if ($request_method = POST) {
                proxy_set_header X-Method "POST";
            }
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxySetHeaderInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        // The if block has proxy_set_header but missing Host from server
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("host"));
    }

    #[test]
    fn test_multiple_servers() {
        let config = parse_string(
            r#"
http {
    server {
        proxy_set_header Host $host;

        location / {
            proxy_set_header X-Custom "value";
        }
    }

    server {
        proxy_set_header X-Real-IP $remote_addr;

        location / {
            proxy_set_header X-Other "value";
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = ProxySetHeaderInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        // Both servers have location blocks missing parent headers
        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(ProxySetHeaderInheritancePlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }
}
