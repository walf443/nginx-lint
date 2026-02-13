//! directive-inheritance plugin
//!
//! This plugin warns when certain directives are used in a child block without
//! explicitly including values that were set in the parent block.
//!
//! In nginx, many directives in a child block completely override those in the
//! parent block - they are NOT inherited. This is a common source of bugs.
//!
//! Checked directives:
//! - proxy_set_header, add_header, proxy_hide_header, grpc_set_header (case-insensitive keys)
//! - fastcgi_param, uwsgi_param, scgi_param (case-sensitive keys)

use nginx_lint_plugin::prelude::*;
use std::collections::HashMap;

/// Specification for a directive to check for inheritance issues
struct DirectiveSpec {
    /// The directive name (e.g., "proxy_set_header")
    name: &'static str,
    /// Whether the first argument key comparison is case-insensitive
    case_insensitive: bool,
    /// If true, all numeric arguments are separate keys (for error_page)
    multi_key: bool,
}

/// Directives that have the "override, not inherit" behavior in nginx
const CHECKED_DIRECTIVES: &[DirectiveSpec] = &[
    DirectiveSpec {
        name: "proxy_set_header",
        case_insensitive: true,
        multi_key: false,
    },
    DirectiveSpec {
        name: "add_header",
        case_insensitive: true,
        multi_key: false,
    },
    DirectiveSpec {
        name: "fastcgi_param",
        case_insensitive: false,
        multi_key: false,
    },
    DirectiveSpec {
        name: "proxy_hide_header",
        case_insensitive: true,
        multi_key: false,
    },
    DirectiveSpec {
        name: "grpc_set_header",
        case_insensitive: true,
        multi_key: false,
    },
    DirectiveSpec {
        name: "uwsgi_param",
        case_insensitive: false,
        multi_key: false,
    },
    DirectiveSpec {
        name: "scgi_param",
        case_insensitive: false,
        multi_key: false,
    },
    DirectiveSpec {
        name: "error_page",
        case_insensitive: false,
        multi_key: true,
    },
];

/// Information about a directive instance
#[derive(Clone, Debug)]
struct DirectiveInfo {
    /// The key (first argument), normalized for comparison
    key_normalized: String,
    /// The original directive text for fix generation
    directive_text: String,
    /// Line number for preserving order
    line: usize,
}

/// Per-directive-name tracking of parent directives
type ParentDirectives = HashMap<&'static str, HashMap<String, DirectiveInfo>>;

#[derive(Default)]
pub struct DirectiveInheritancePlugin;

impl DirectiveInheritancePlugin {
    /// Reconstruct directive text from a Directive AST node
    fn directive_to_text(directive: &Directive) -> String {
        let mut parts = vec![directive.name.clone()];
        for arg in &directive.args {
            parts.push(arg.to_source());
        }
        format!("{};", parts.join(" "))
    }

    /// Normalize a key based on the directive spec
    fn normalize_key(key: &str, spec: &DirectiveSpec) -> String {
        if spec.case_insensitive {
            key.to_lowercase()
        } else {
            key.to_string()
        }
    }

    /// Find the DirectiveSpec for a given directive name
    fn find_spec(name: &str) -> Option<&'static DirectiveSpec> {
        CHECKED_DIRECTIVES.iter().find(|s| s.name == name)
    }

    /// Collect checked directives from a block's direct children (not nested)
    fn collect_directives_from_block(
        block: &Block,
    ) -> HashMap<&'static str, HashMap<String, DirectiveInfo>> {
        let mut result: HashMap<&'static str, HashMap<String, DirectiveInfo>> = HashMap::new();

        for item in &block.items {
            if let ConfigItem::Directive(directive) = item
                && let Some(spec) = Self::find_spec(&directive.name)
            {
                let directive_text = Self::directive_to_text(directive);
                let line = directive.span.start.line;

                if spec.multi_key {
                    // Extract all numeric arguments as separate keys (for error_page)
                    for arg in &directive.args {
                        if arg.as_str().parse::<u16>().is_ok() {
                            let key = arg.as_str().to_string();
                            let info = DirectiveInfo {
                                key_normalized: key.clone(),
                                directive_text: directive_text.clone(),
                                line,
                            };
                            result.entry(spec.name).or_default().insert(key, info);
                        }
                    }
                } else if let Some(first_arg) = directive.first_arg() {
                    let key = Self::normalize_key(first_arg, spec);
                    let info = DirectiveInfo {
                        key_normalized: key.clone(),
                        directive_text,
                        line,
                    };
                    result.entry(spec.name).or_default().insert(key, info);
                }
            }
        }

        result
    }

    /// Check a block for directive inheritance issues
    fn check_block(
        &self,
        items: &[ConfigItem],
        parent_directives: &ParentDirectives,
        errors: &mut Vec<LintError>,
    ) {
        for item in items {
            if let ConfigItem::Directive(directive) = item
                && let Some(block) = &directive.block
            {
                let is_inheritable_context = matches!(
                    directive.name.as_str(),
                    "server" | "location" | "if" | "limit_except"
                );

                if is_inheritable_context {
                    let current = Self::collect_directives_from_block(block);

                    // Check each directive type for missing parent entries
                    for spec in CHECKED_DIRECTIVES {
                        let parent_entries = parent_directives.get(spec.name);
                        let current_entries = current.get(spec.name);

                        if let (Some(parent), Some(current_map)) = (parent_entries, current_entries)
                            && !parent.is_empty()
                            && !current_map.is_empty()
                        {
                            let missing: Vec<_> = parent
                                .iter()
                                .filter(|(key, _)| !current_map.contains_key(*key))
                                .map(|(_, info)| info.clone())
                                .collect();

                            if !missing.is_empty() {
                                self.report_missing(block, spec, &missing, errors);
                            }
                        }
                    }

                    // Merge parent and current for recursion
                    let mut merged = parent_directives.clone();
                    for (directive_name, entries) in &current {
                        let merged_entries = merged.entry(directive_name).or_default();
                        for (key, info) in entries {
                            merged_entries.insert(key.clone(), info.clone());
                        }
                    }

                    self.check_block(&block.items, &merged, errors);
                } else if directive.name == "http" {
                    // http block: start fresh collection
                    let current = Self::collect_directives_from_block(block);
                    let mut fresh: ParentDirectives = HashMap::new();
                    for (name, entries) in current {
                        fresh.insert(name, entries);
                    }
                    self.check_block(&block.items, &fresh, errors);
                } else {
                    // Other blocks (upstream, etc.): pass through
                    self.check_block(&block.items, parent_directives, errors);
                }
            }
        }
    }

    /// Report missing directives as a lint error with autofix
    fn report_missing(
        &self,
        block: &Block,
        spec: &DirectiveSpec,
        missing: &[DirectiveInfo],
        errors: &mut Vec<LintError>,
    ) {
        let mut missing_sorted = missing.to_vec();
        missing_sorted.sort_by_key(|d| d.line);

        // Find the first directive of this type in the block
        let first_directive = block
            .items
            .iter()
            .filter_map(|item| {
                if let ConfigItem::Directive(d) = item
                    && d.name == spec.name
                {
                    Some(d.as_ref())
                } else {
                    None
                }
            })
            .next();

        if let Some(first) = first_directive {
            let err_builder =
                PluginSpec::new("directive-inheritance", "best-practices", "").error_builder();

            let missing_keys: Vec<String> = missing_sorted
                .iter()
                .map(|d| format!("'{}'", d.key_normalized))
                .collect();

            let mut missing_texts: Vec<&str> = missing_sorted
                .iter()
                .map(|d| d.directive_text.as_str())
                .collect();
            // Deduplicate: multi-key directives (e.g., error_page 500 502 503 504 /50x.html)
            // may register multiple keys pointing to the same directive text.
            missing_texts.dedup();

            let error = err_builder
                .warning_at(
                    &format!(
                        "{} in this block does not include directives from parent block: {}. \
                         In nginx, {} directives are not inherited - \
                         all directives must be explicitly repeated in child blocks",
                        spec.name,
                        missing_keys.join(", "),
                        spec.name,
                    ),
                    first,
                )
                .with_fix(first.insert_before_many(&missing_texts));

            errors.push(error);
        }
    }
}

impl Plugin for DirectiveInheritancePlugin {
    fn spec(&self) -> PluginSpec {
        let directive_names: Vec<&str> = CHECKED_DIRECTIVES.iter().map(|s| s.name).collect();

        PluginSpec::new(
            "directive-inheritance",
            "best-practices",
            "Warns when directives in child blocks don't include parent block values",
        )
        .with_severity("warning")
        .with_why(
            format!(
                "In nginx, certain directives in a child block (like location) completely \
                 override those in the parent block (like server) - they are NOT inherited. \
                 This is a common source of bugs where important settings are unintentionally lost.\n\n\
                 Checked directives: {}\n\n\
                 When using these directives in a child block, you must explicitly repeat all \
                 values that were set in the parent block.",
                directive_names.join(", "),
            ),
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_set_header".to_string(),
            "https://nginx.org/en/docs/http/ngx_http_headers_module.html#add_header".to_string(),
            "https://nginx.org/en/docs/http/ngx_http_fastcgi_module.html#fastcgi_param".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        self.check_block(&config.items, &HashMap::new(), &mut errors);
        errors
    }
}

nginx_lint_plugin::export_plugin!(DirectiveInheritancePlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::parse_string;
    use nginx_lint_plugin::testing::PluginTestRunner;

    // ========================================================================
    // proxy_set_header tests (migrated from proxy-set-header-inheritance)
    // ========================================================================

    #[test]
    fn test_proxy_set_header_missing_parent() {
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

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("proxy_set_header"));
        assert!(errors[0].message.contains("host"));
        assert!(errors[0].message.contains("x-real-ip"));
    }

    #[test]
    fn test_proxy_set_header_with_fix() {
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

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(!errors[0].fixes.is_empty(), "Expected fix to be present");

        let fix = &errors[0].fixes[0];
        assert!(
            fix.new_text.contains("proxy_set_header Host"),
            "Fix should contain Host header: {}",
            fix.new_text
        );
        assert!(
            fix.new_text.contains("proxy_set_header X-Real-IP"),
            "Fix should contain X-Real-IP header: {}",
            fix.new_text
        );
    }

    #[test]
    fn test_proxy_set_header_all_included() {
        let runner = PluginTestRunner::new(DirectiveInheritancePlugin);

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
    fn test_no_parent_directives() {
        let runner = PluginTestRunner::new(DirectiveInheritancePlugin);

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
    fn test_no_child_directives() {
        let runner = PluginTestRunner::new(DirectiveInheritancePlugin);

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

        let plugin = DirectiveInheritancePlugin;
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

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
    }

    #[test]
    fn test_case_insensitive_headers() {
        let runner = PluginTestRunner::new(DirectiveInheritancePlugin);

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

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

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

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
    }

    #[test]
    fn test_quoted_value_in_fix() {
        let config = parse_string(
            r#"
http {
    server {
        proxy_set_header X-Custom-Header "custom value";

        location / {
            proxy_set_header X-Other "other";
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(!errors[0].fixes.is_empty());

        let fix = &errors[0].fixes[0];
        assert!(
            fix.new_text.contains("\"custom value\""),
            "Fix should preserve quoted value: {}",
            fix.new_text
        );
    }

    // ========================================================================
    // add_header tests (migrated from add-header-inheritance)
    // ========================================================================

    #[test]
    fn test_add_header_missing_parent() {
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

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("add_header"));
        assert!(errors[0].message.contains("x-frame-options"));
        assert!(errors[0].message.contains("x-content-type-options"));
    }

    #[test]
    fn test_add_header_all_included() {
        let runner = PluginTestRunner::new(DirectiveInheritancePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        add_header X-Frame-Options DENY;

        location / {
            add_header X-Frame-Options DENY;
            add_header X-Custom "value";
            root /var/www/html;
        }
    }
}
"#,
        );
    }

    // ========================================================================
    // fastcgi_param tests (new)
    // ========================================================================

    #[test]
    fn test_fastcgi_param_missing_parent() {
        let config = parse_string(
            r#"
http {
    server {
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
        fastcgi_param QUERY_STRING $query_string;

        location ~ \.php$ {
            fastcgi_param REQUEST_METHOD $request_method;
            fastcgi_pass unix:/run/php/php-fpm.sock;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("fastcgi_param"));
        assert!(errors[0].message.contains("SCRIPT_FILENAME"));
        assert!(errors[0].message.contains("QUERY_STRING"));
    }

    #[test]
    fn test_fastcgi_param_case_sensitive() {
        let config = parse_string(
            r#"
http {
    server {
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;

        location ~ \.php$ {
            fastcgi_param script_filename $document_root$fastcgi_script_name;
            fastcgi_pass unix:/run/php/php-fpm.sock;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        // fastcgi_param keys are case-sensitive, so SCRIPT_FILENAME != script_filename
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("SCRIPT_FILENAME"));
    }

    #[test]
    fn test_fastcgi_param_all_included() {
        let runner = PluginTestRunner::new(DirectiveInheritancePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;

        location ~ \.php$ {
            fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
            fastcgi_param REQUEST_METHOD $request_method;
            fastcgi_pass unix:/run/php/php-fpm.sock;
        }
    }
}
"#,
        );
    }

    // ========================================================================
    // proxy_hide_header tests (new)
    // ========================================================================

    #[test]
    fn test_proxy_hide_header_missing_parent() {
        let config = parse_string(
            r#"
http {
    server {
        proxy_hide_header X-Powered-By;
        proxy_hide_header Server;

        location / {
            proxy_hide_header X-Custom;
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("proxy_hide_header"));
        assert!(errors[0].message.contains("x-powered-by"));
        assert!(errors[0].message.contains("server"));
    }

    // ========================================================================
    // grpc_set_header tests (new)
    // ========================================================================

    #[test]
    fn test_grpc_set_header_missing_parent() {
        let config = parse_string(
            r#"
http {
    server {
        grpc_set_header Host $host;

        location / {
            grpc_set_header X-Custom "value";
            grpc_pass grpc://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("grpc_set_header"));
        assert!(errors[0].message.contains("host"));
    }

    // ========================================================================
    // error_page tests (new)
    // ========================================================================

    #[test]
    fn test_error_page_missing_parent() {
        let config = parse_string(
            r#"
http {
    server {
        error_page 404 /404.html;

        location / {
            error_page 403 /403.html;
            root /var/www/html;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("error_page"));
        assert!(errors[0].message.contains("404"));
    }

    #[test]
    fn test_error_page_multi_code() {
        let config = parse_string(
            r#"
http {
    server {
        error_page 500 502 503 504 /50x.html;

        location / {
            error_page 403 /403.html;
            root /var/www/html;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("'500'"));
        assert!(errors[0].message.contains("'502'"));
        assert!(errors[0].message.contains("'503'"));
        assert!(errors[0].message.contains("'504'"));

        // Fix should insert the original directive (only once, not 4 times)
        let fix = &errors[0].fixes[0];
        let count = fix
            .new_text
            .matches("error_page 500 502 503 504 /50x.html;")
            .count();
        assert_eq!(
            count, 1,
            "Fix should contain the directive exactly once: {}",
            fix.new_text
        );
    }

    #[test]
    fn test_error_page_all_included() {
        let runner = PluginTestRunner::new(DirectiveInheritancePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        error_page 404 /404.html;

        location / {
            error_page 404 /404.html;
            error_page 403 /403.html;
            root /var/www/html;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_error_page_no_child() {
        let runner = PluginTestRunner::new(DirectiveInheritancePlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        error_page 404 /404.html;
        error_page 500 502 503 504 /50x.html;

        location / {
            root /var/www/html;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_error_page_with_response_code() {
        let config = parse_string(
            r#"
http {
    server {
        error_page 404 =200 /empty.gif;

        location / {
            error_page 403 /403.html;
            root /var/www/html;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("'404'"));
    }

    // ========================================================================
    // Mixed directive tests
    // ========================================================================

    #[test]
    fn test_multiple_directive_types_in_same_block() {
        let config = parse_string(
            r#"
http {
    server {
        proxy_set_header Host $host;
        add_header X-Frame-Options DENY;

        location / {
            proxy_set_header X-Custom "value";
            add_header X-Custom "value";
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = DirectiveInheritancePlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should report 2 errors: one for proxy_set_header, one for add_header
        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);

        let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
        assert!(
            messages.iter().any(|m| m.contains("proxy_set_header")),
            "Should have proxy_set_header error"
        );
        assert!(
            messages.iter().any(|m| m.contains("add_header")),
            "Should have add_header error"
        );
    }

    #[test]
    fn test_independent_directive_types() {
        let runner = PluginTestRunner::new(DirectiveInheritancePlugin);

        // proxy_set_header in child should not affect add_header checking
        runner.assert_no_errors(
            r#"
http {
    server {
        add_header X-Frame-Options DENY;

        location / {
            proxy_set_header Host $host;
            root /var/www/html;
        }
    }
}
"#,
        );
    }

    // ========================================================================
    // Example tests
    // ========================================================================

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(DirectiveInheritancePlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(DirectiveInheritancePlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
