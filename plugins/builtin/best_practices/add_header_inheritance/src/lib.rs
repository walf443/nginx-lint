//! add-header-inheritance plugin
//!
//! This plugin warns when add_header is used in a child block without
//! explicitly including headers that were set in the parent block.
//!
//! In nginx, add_header directives in a child block completely override
//! those in the parent block - they are NOT inherited. This is a common source
//! of bugs where headers set at the server level are lost in location blocks.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;
use std::collections::HashMap;

/// Information about an add_header directive
#[derive(Clone, Debug)]
struct HeaderInfo {
    /// The header name (lowercase for comparison)
    name_lower: String,
    /// The original directive text (e.g., "add_header X-Frame-Options DENY;")
    directive_text: String,
    /// The line number where this header was defined (for preserving order)
    line: usize,
}

/// Check if add_header in child blocks includes all parent headers
#[derive(Default)]
pub struct AddHeaderInheritancePlugin;

impl AddHeaderInheritancePlugin {
    /// Reconstruct the directive text from a Directive
    fn directive_to_text(directive: &Directive) -> String {
        let mut parts = vec![directive.name.clone()];
        for arg in &directive.args {
            let arg_text = match &arg.value {
                ArgumentValue::Literal(s) => s.clone(),
                ArgumentValue::QuotedString(s) => format!("\"{}\"", s),
                ArgumentValue::SingleQuotedString(s) => format!("'{}'", s),
                ArgumentValue::Variable(s) => format!("${}", s),
            };
            parts.push(arg_text);
        }
        format!("{};", parts.join(" "))
    }

    /// Collect add_header headers from a block's direct children (not nested)
    fn collect_headers_from_block(block: &Block) -> HashMap<String, HeaderInfo> {
        let mut headers = HashMap::new();
        for item in &block.items {
            if let ConfigItem::Directive(directive) = item {
                if directive.name == "add_header" {
                    if let Some(header_name) = directive.first_arg() {
                        let info = HeaderInfo {
                            name_lower: header_name.to_lowercase(),
                            directive_text: Self::directive_to_text(directive),
                            line: directive.span.start.line,
                        };
                        headers.insert(header_name.to_lowercase(), info);
                    }
                }
            }
        }
        headers
    }

    /// Get the indentation string for a given column (1-indexed)
    fn get_indent(column: usize) -> String {
        " ".repeat(column.saturating_sub(1))
    }

    /// Check a block for add_header inheritance issues
    fn check_block(
        &self,
        items: &[ConfigItem],
        parent_headers: &HashMap<String, HeaderInfo>,
        errors: &mut Vec<LintError>,
    ) {
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                // Check if this is a block that can contain add_header
                if let Some(block) = &directive.block {
                    // Only check server, location, if, limit_except blocks
                    let is_header_context = matches!(
                        directive.name.as_str(),
                        "server" | "location" | "if" | "limit_except"
                    );

                    if is_header_context {
                        // Collect headers defined in this block
                        let current_headers = Self::collect_headers_from_block(block);

                        // If this block has any add_header, check for missing parent headers
                        if !current_headers.is_empty() && !parent_headers.is_empty() {
                            let missing: Vec<_> = parent_headers
                                .iter()
                                .filter(|(name, _)| !current_headers.contains_key(*name))
                                .map(|(_, info)| info.clone())
                                .collect();

                            if !missing.is_empty() {
                                // Sort by original line number to preserve parent block order
                                let mut missing_sorted = missing.clone();
                                missing_sorted.sort_by_key(|h| h.line);

                                // Find the first add_header in this block for error location and indentation
                                let first_header_info = block
                                    .items
                                    .iter()
                                    .filter_map(|item| {
                                        if let ConfigItem::Directive(d) = item {
                                            if d.name == "add_header" {
                                                return Some((
                                                    d.span.start.line,
                                                    d.span.start.column,
                                                    d.span.start.offset,
                                                ));
                                            }
                                        }
                                        None
                                    })
                                    .next()
                                    .unwrap_or((
                                        directive.span.start.line,
                                        directive.span.start.column,
                                        directive.span.start.offset,
                                    ));

                                let (line, column, offset) = first_header_info;
                                let indent = Self::get_indent(column);

                                // Calculate offset at the beginning of the line (before indentation)
                                let line_start_offset = offset - (column - 1);

                                // Build the fix text: insert missing headers before the first add_header
                                let fix_text: String = missing_sorted
                                    .iter()
                                    .map(|h| format!("{}{}\n", indent, h.directive_text))
                                    .collect();

                                let mut error = LintError::warning(
                                    "add-header-inheritance",
                                    "best-practices",
                                    &format!(
                                        "add_header in this block does not include headers from parent block: {}. \
                                         In nginx, add_header directives are not inherited - \
                                         all headers must be explicitly repeated in child blocks",
                                        missing_sorted
                                            .iter()
                                            .map(|h| format!("'{}'", h.name_lower))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    ),
                                    line,
                                    column,
                                );

                                // Add fix: insert missing headers before the first add_header
                                error = error.with_fix(Fix::replace_range(line_start_offset, line_start_offset, &fix_text));

                                errors.push(error);
                            }
                        }

                        // Merge parent and current headers for nested blocks
                        let mut merged_headers = parent_headers.clone();
                        for (name, info) in current_headers {
                            merged_headers.insert(name, info);
                        }

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

impl Plugin for AddHeaderInheritancePlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "add-header-inheritance",
            "best-practices",
            "Warns when add_header in child blocks doesn't include parent headers",
        )
        .with_severity("warning")
        .with_why(
            "In nginx, add_header directives in a child block (like location) completely \
             override those in the parent block (like server) - they are NOT inherited. \
             This is a common source of bugs where important security headers like \
             X-Frame-Options, X-Content-Type-Options, or Content-Security-Policy are \
             unintentionally lost.\n\n\
             When using add_header in a child block, you must explicitly repeat all \
             headers that were set in the parent block.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_headers_module.html#add_header".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        // Start with empty parent headers at root level
        self.check_block(&config.items, &HashMap::new(), &mut errors);

        errors
    }
}

// Export the plugin
nginx_lint::export_plugin!(AddHeaderInheritancePlugin);

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
        add_header X-Frame-Options DENY;
        add_header X-Content-Type-Options nosniff;

        location / {
            add_header X-Custom "value";
            root /var/www/html;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = AddHeaderInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("x-frame-options"));
        assert!(errors[0].message.contains("x-content-type-options"));
    }

    #[test]
    fn test_missing_parent_headers_with_fix() {
        let config = parse_string(
            r#"
http {
    server {
        add_header X-Frame-Options DENY;
        add_header X-Content-Type-Options nosniff;

        location / {
            add_header X-Custom "value";
            root /var/www/html;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = AddHeaderInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].fix.is_some(), "Expected fix to be present");

        let fix = errors[0].fix.as_ref().unwrap();
        assert!(
            fix.new_text.contains("add_header X-Frame-Options"),
            "Fix should contain X-Frame-Options header: {}",
            fix.new_text
        );
        assert!(
            fix.new_text.contains("add_header X-Content-Type-Options"),
            "Fix should contain X-Content-Type-Options header: {}",
            fix.new_text
        );
    }

    #[test]
    fn test_all_headers_included() {
        let runner = PluginTestRunner::new(AddHeaderInheritancePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        add_header X-Frame-Options DENY;
        add_header X-Content-Type-Options nosniff;

        location / {
            add_header X-Frame-Options DENY;
            add_header X-Content-Type-Options nosniff;
            add_header X-Custom "value";
            root /var/www/html;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_parent_headers() {
        let runner = PluginTestRunner::new(AddHeaderInheritancePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            add_header X-Custom "value";
            root /var/www/html;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_child_headers() {
        let runner = PluginTestRunner::new(AddHeaderInheritancePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        add_header X-Frame-Options DENY;
        add_header X-Content-Type-Options nosniff;

        location / {
            root /var/www/html;
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
    add_header X-Frame-Options DENY;

    server {
        location / {
            add_header X-Custom "value";
            root /var/www/html;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = AddHeaderInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("x-frame-options"));
    }

    #[test]
    fn test_nested_location() {
        let config = parse_string(
            r#"
http {
    server {
        add_header X-Frame-Options DENY;

        location /api {
            add_header X-API "true";

            location /api/v2 {
                add_header X-V2 "true";
                root /var/www/api;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = AddHeaderInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should warn for both /api (missing x-frame-options) and /api/v2 (missing x-frame-options, x-api)
        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
    }

    #[test]
    fn test_case_insensitive() {
        let runner = PluginTestRunner::new(AddHeaderInheritancePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        add_header X-Frame-Options DENY;

        location / {
            add_header x-frame-options DENY;
            add_header X-Custom "value";
            root /var/www/html;
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
        add_header X-Frame-Options DENY;

        location / {
            if ($request_method = POST) {
                add_header X-Method "POST";
            }
            root /var/www/html;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = AddHeaderInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        // The if block has add_header but missing X-Frame-Options from server
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("x-frame-options"));
    }

    #[test]
    fn test_multiple_servers() {
        let config = parse_string(
            r#"
http {
    server {
        add_header X-Frame-Options DENY;

        location / {
            add_header X-Custom "value";
        }
    }

    server {
        add_header X-Content-Type-Options nosniff;

        location / {
            add_header X-Other "value";
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = AddHeaderInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        // Both servers have location blocks missing parent headers
        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
    }

    #[test]
    fn test_quoted_value_in_fix() {
        let config = parse_string(
            r#"
http {
    server {
        add_header Content-Security-Policy "default-src 'self'";

        location / {
            add_header X-Other "other";
            root /var/www/html;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = AddHeaderInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].fix.is_some());

        let fix = errors[0].fix.as_ref().unwrap();
        // Check that quoted value is preserved in fix
        assert!(
            fix.new_text.contains("\"default-src 'self'\""),
            "Fix should preserve quoted value: {}",
            fix.new_text
        );
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(AddHeaderInheritancePlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }
}
