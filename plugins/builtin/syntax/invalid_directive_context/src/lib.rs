//! invalid-directive-context plugin
//!
//! This plugin detects directives placed in invalid parent contexts.
//! For example, 'server' can only be inside 'http', 'stream', or 'mail' blocks,
//! and 'location' can only be inside 'server' or another 'location' block.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;

/// Check for directives placed in invalid contexts
#[derive(Default)]
pub struct InvalidDirectiveContextPlugin;

impl Plugin for InvalidDirectiveContextPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "invalid-directive-context",
            "syntax",
            "Detects directives placed in invalid parent contexts",
        )
        .with_severity("error")
        .with_why(
            "nginx directives must be placed in the correct context (parent block). \
             For example, 'server' can only be inside 'http', 'stream', or 'mail' blocks, \
             and 'location' can only be inside 'server' or another 'location' block. \
             When directives are placed in the wrong context, nginx will fail to start \
             with a configuration error.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/beginners_guide.html".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        // Build initial parent stack from include_context
        let parent_stack: Vec<&str> = config.include_context.iter().map(|s| s.as_str()).collect();

        check_context(&config.items, &parent_stack, &mut errors);

        errors
    }
}

/// Define valid parent contexts for each block directive
/// Returns None if the directive has no restrictions (can be anywhere)
fn valid_contexts(directive_name: &str, has_block: bool) -> Option<Vec<&'static str>> {
    // Only check block directives
    if !has_block {
        return None;
    }

    match directive_name {
        // Root-only directives (empty slice = root only)
        "http" | "events" | "stream" | "mail" => Some(vec![]),
        // server can be in http, stream, or mail
        "server" => Some(vec!["http", "stream", "mail"]),
        // location can be in server or nested in another location
        "location" => Some(vec!["server", "location"]),
        // upstream can be in http or stream
        "upstream" => Some(vec!["http", "stream"]),
        // if can be in server or location
        "if" => Some(vec!["server", "location"]),
        // limit_except can only be in location
        "limit_except" => Some(vec!["location"]),
        // types can be in http, server, or location
        "types" => Some(vec!["http", "server", "location"]),
        // map and geo can be in http or stream
        "map" | "geo" => Some(vec!["http", "stream"]),
        // No restrictions for other directives
        _ => None,
    }
}

fn check_context(items: &[ConfigItem], parent_stack: &[&str], errors: &mut Vec<LintError>) {
    for item in items {
        if let ConfigItem::Directive(directive) = item {
            let has_block = directive.block.is_some();

            // Check if this directive has context restrictions
            if let Some(valid_parents) = valid_contexts(&directive.name, has_block) {
                let current_parent = parent_stack.last().copied();
                let is_valid = if valid_parents.is_empty() {
                    // Must be at root (no parent)
                    current_parent.is_none()
                } else {
                    current_parent.is_some_and(|p| valid_parents.contains(&p))
                };

                if !is_valid {
                    let message = if valid_parents.is_empty() {
                        format!(
                            "'{}' directive must be in main context, not inside '{}'",
                            directive.name,
                            current_parent.unwrap_or("unknown")
                        )
                    } else if current_parent.is_none() {
                        format!(
                            "'{}' directive must be inside one of: {}, not in main context",
                            directive.name,
                            valid_parents.join(", ")
                        )
                    } else {
                        format!(
                            "'{}' directive cannot be inside '{}', valid contexts: {}",
                            directive.name,
                            current_parent.unwrap_or("unknown"),
                            valid_parents.join(", ")
                        )
                    };

                    errors.push(LintError::error(
                        "invalid-directive-context",
                        "syntax",
                        &message,
                        directive.span.start.line,
                        directive.span.start.column,
                    ));
                }
            }

            // Recurse into block
            if let Some(block) = &directive.block {
                let mut new_stack: Vec<&str> = parent_stack.to_vec();
                new_stack.push(&directive.name);
                check_context(&block.items, &new_stack, errors);
            }
        }
    }
}

// Export the plugin
nginx_lint::export_plugin!(InvalidDirectiveContextPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::PluginTestRunner;

    #[test]
    fn test_valid_root_level_directives() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_no_errors(
            r#"
events {
    worker_connections 1024;
}

http {
    server {
        listen 80;
    }
}
"#,
        );
    }

    #[test]
    fn test_server_in_http() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_no_errors(
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
    fn test_server_in_server_error() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        server {
            listen 80;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_location_in_http_error() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_has_errors(
            r#"
http {
    location / {
        root /var/www;
    }
}
"#,
        );
    }

    #[test]
    fn test_location_in_server() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            root /var/www;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_nested_location() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location /api {
            location /api/v1 {
                root /var/www;
            }
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_http_not_at_root_error() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        http {
            server {
                listen 80;
            }
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_events_not_at_root_error() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_has_errors(
            r#"
http {
    events {
        worker_connections 1024;
    }
}
"#,
        );
    }

    #[test]
    fn test_upstream_in_http() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_no_errors(
            r#"
http {
    upstream backend {
        server 127.0.0.1:8080;
    }
}
"#,
        );
    }

    #[test]
    fn test_upstream_in_location_error() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location / {
            upstream backend {
                server 127.0.0.1:8080;
            }
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_if_in_location() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            if ($request_method = POST) {
                return 405;
            }
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_if_in_http_error() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_has_errors(
            r#"
http {
    if ($host = "example.com") {
        return 301 https://www.example.com$request_uri;
    }
}
"#,
        );
    }

    #[test]
    fn test_limit_except_in_location() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            limit_except GET POST {
                deny all;
            }
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_limit_except_in_server_error() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        limit_except GET {
            deny all;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_map_in_http() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_no_errors(
            r#"
http {
    map $uri $new_uri {
        default 0;
    }
}
"#,
        );
    }

    #[test]
    fn test_geo_in_http() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_no_errors(
            r#"
http {
    geo $country {
        default unknown;
    }
}
"#,
        );
    }

    #[test]
    fn test_server_at_root_error() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_has_errors(
            r#"
server {
    listen 80;
}
"#,
        );
    }

    #[test]
    fn test_stream_at_root() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_no_errors(
            r#"
stream {
    server {
        listen 12345;
    }
}
"#,
        );
    }

    #[test]
    fn test_mail_at_root() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);

        runner.assert_no_errors(
            r#"
mail {
    server {
        listen 25;
    }
}
"#,
        );
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(InvalidDirectiveContextPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    // Tests for include context

    #[test]
    fn test_include_context_location_in_server() {
        // File included from server context, location is valid
        use nginx_lint::parse_string;

        let mut config = parse_string(
            r#"
location / {
    root /var/www;
}
"#,
        )
        .unwrap();

        config.include_context = vec!["http".to_string(), "server".to_string()];

        let plugin = InvalidDirectiveContextPlugin;
        let errors = plugin.check(&config, "test.conf");
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_include_context_server_in_server_error() {
        // File included from server context, another server is invalid
        use nginx_lint::parse_string;

        let mut config = parse_string(
            r#"
server {
    listen 8080;
}
"#,
        )
        .unwrap();

        config.include_context = vec!["http".to_string(), "server".to_string()];

        let plugin = InvalidDirectiveContextPlugin;
        let errors = plugin.check(&config, "test.conf");
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("'server' directive cannot be inside 'server'"));
    }

    #[test]
    fn test_include_context_server_in_http() {
        // File included from http context, server is valid
        use nginx_lint::parse_string;

        let mut config = parse_string(
            r#"
server {
    listen 80;
    location / {
        root /var/www;
    }
}
"#,
        )
        .unwrap();

        config.include_context = vec!["http".to_string()];

        let plugin = InvalidDirectiveContextPlugin;
        let errors = plugin.check(&config, "test.conf");
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }
}
