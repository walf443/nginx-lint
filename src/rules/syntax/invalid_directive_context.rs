use crate::docs::RuleDoc;
use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::{Config, ConfigItem};
use std::collections::HashMap;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "invalid-directive-context",
    category: "syntax",
    description: "Detects directives placed in invalid parent contexts",
    severity: "error",
    why: r#"nginx directives must be placed in the correct context (parent block).
For example, 'server' can only be inside 'http', 'stream', or 'mail' blocks,
and 'location' can only be inside 'server' or another 'location' block.

When directives are placed in the wrong context, nginx will fail to start
with a configuration error.

If you are linting a file that is included from another config (e.g. sites-available),
add a context comment at the top of the file to tell nginx-lint the parent context:

  # nginx-lint:context http,server

Or use the --context CLI flag:

  nginx-lint --context http,server sites-available/example.conf"#,
    bad_example: include_str!("invalid_directive_context/bad.conf"),
    good_example: include_str!("invalid_directive_context/good.conf"),
    references: &["https://nginx.org/en/docs/beginners_guide.html"],
};

/// Check for directives placed in invalid contexts
pub struct InvalidDirectiveContext {
    /// Additional valid parent contexts for directives (from config)
    additional_contexts: HashMap<String, Vec<String>>,
}

impl InvalidDirectiveContext {
    /// Create a new rule with default settings
    pub fn new() -> Self {
        Self {
            additional_contexts: HashMap::new(),
        }
    }

    /// Create a new rule with additional contexts
    pub fn with_additional_contexts(additional_contexts: HashMap<String, Vec<String>>) -> Self {
        Self {
            additional_contexts,
        }
    }

    /// Define valid parent contexts for each block directive
    /// Returns None if the directive has no restrictions (can be anywhere)
    ///
    /// Note: This only applies to block directives (directives with `{}`).
    /// For example, `server` inside `upstream` is a simple directive (server address),
    /// not a block directive, so it's not checked here.
    fn valid_contexts(&self, directive_name: &str, has_block: bool) -> Option<Vec<&str>> {
        // Only check block directives
        if !has_block {
            return None;
        }

        let base_contexts: Option<&[&str]> = match directive_name {
            // Root-only directives (empty slice = root only)
            "http" | "events" | "stream" | "mail" => Some(&[]),
            // server can be in http, stream, or mail
            "server" => Some(&["http", "stream", "mail"]),
            // location can be in server or nested in another location
            "location" => Some(&["server", "location"]),
            // upstream can be in http or stream
            "upstream" => Some(&["http", "stream"]),
            // if can be in server or location
            "if" => Some(&["server", "location"]),
            // limit_except can only be in location
            "limit_except" => Some(&["location"]),
            // types can be in http, server, or location
            "types" => Some(&["http", "server", "location"]),
            // map and geo can be in http or stream
            "map" | "geo" => Some(&["http", "stream"]),
            // No restrictions for other directives
            _ => None,
        };

        // Merge base contexts with additional contexts from config
        match base_contexts {
            Some(base) => {
                let mut contexts: Vec<&str> = base.to_vec();
                if let Some(additional) = self.additional_contexts.get(directive_name) {
                    for ctx in additional {
                        if !contexts.contains(&ctx.as_str()) {
                            contexts.push(ctx.as_str());
                        }
                    }
                }
                Some(contexts)
            }
            None => {
                // Check if there are additional contexts for unknown directives
                self.additional_contexts
                    .get(directive_name)
                    .map(|additional| additional.iter().map(|s| s.as_str()).collect())
            }
        }
    }

    fn check_context(
        &self,
        items: &[ConfigItem],
        parent_stack: &[&str],
        errors: &mut Vec<LintError>,
    ) {
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                let has_block = directive.block.is_some();

                // Check if this directive has context restrictions
                if let Some(valid_parents) = self.valid_contexts(&directive.name, has_block) {
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

                        errors.push(
                            LintError::new(self.name(), self.category(), &message, Severity::Error)
                                .with_location(
                                    directive.span.start.line,
                                    directive.span.start.column,
                                ),
                        );
                    }
                }

                // Recurse into block
                if let Some(block) = &directive.block {
                    let mut new_stack = parent_stack.to_vec();
                    new_stack.push(&directive.name);
                    self.check_context(&block.items, &new_stack, errors);
                }
            }
        }
    }
}

impl Default for InvalidDirectiveContext {
    fn default() -> Self {
        Self::new()
    }
}

impl LintRule for InvalidDirectiveContext {
    fn name(&self) -> &'static str {
        "invalid-directive-context"
    }

    fn category(&self) -> &'static str {
        "syntax"
    }

    fn description(&self) -> &'static str {
        "Detects directives placed in invalid parent contexts"
    }

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        // Build initial parent stack from include_context
        let parent_stack: Vec<&str> = config.include_context.iter().map(|s| s.as_str()).collect();

        self.check_context(&config.items, &parent_stack, &mut errors);

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_string;
    use std::path::PathBuf;

    fn check_config(content: &str) -> Vec<LintError> {
        let config = parse_string(content).expect("Failed to parse config");
        let rule = InvalidDirectiveContext::new();
        rule.check(&config, &PathBuf::from("test.conf"))
    }

    fn check_config_with_context(content: &str, context: Vec<String>) -> Vec<LintError> {
        let mut config = parse_string(content).expect("Failed to parse config");
        config.include_context = context;
        let rule = InvalidDirectiveContext::new();
        rule.check(&config, &PathBuf::from("test.conf"))
    }

    fn check_config_with_additional_contexts(
        content: &str,
        additional_contexts: HashMap<String, Vec<String>>,
    ) -> Vec<LintError> {
        let config = parse_string(content).expect("Failed to parse config");
        let rule = InvalidDirectiveContext::with_additional_contexts(additional_contexts);
        rule.check(&config, &PathBuf::from("test.conf"))
    }

    #[test]
    fn test_valid_root_level_directives() {
        let content = r#"
events {
    worker_connections 1024;
}

http {
    server {
        listen 80;
    }
}
"#;
        let errors = check_config(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_server_in_http() {
        let content = r#"
http {
    server {
        listen 80;
    }
}
"#;
        let errors = check_config(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_server_in_server_error() {
        let content = r#"
http {
    server {
        server {
            listen 80;
        }
    }
}
"#;
        let errors = check_config(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0]
                .message
                .contains("'server' directive cannot be inside 'server'")
        );
    }

    #[test]
    fn test_location_in_http_error() {
        let content = r#"
http {
    location / {
        root /var/www;
    }
}
"#;
        let errors = check_config(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0]
                .message
                .contains("'location' directive cannot be inside 'http'")
        );
    }

    #[test]
    fn test_location_in_server() {
        let content = r#"
http {
    server {
        location / {
            root /var/www;
        }
    }
}
"#;
        let errors = check_config(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_nested_location() {
        let content = r#"
http {
    server {
        location /api {
            location /api/v1 {
                root /var/www;
            }
        }
    }
}
"#;
        let errors = check_config(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_http_not_at_root_error() {
        let content = r#"
http {
    server {
        http {
            server {
                listen 80;
            }
        }
    }
}
"#;
        let errors = check_config(content);
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("'http' directive must be in main context")),
            "Expected http context error, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_events_not_at_root_error() {
        let content = r#"
http {
    events {
        worker_connections 1024;
    }
}
"#;
        let errors = check_config(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0]
                .message
                .contains("'events' directive must be in main context")
        );
    }

    #[test]
    fn test_upstream_in_http() {
        let content = r#"
http {
    upstream backend {
        server 127.0.0.1:8080;
    }
}
"#;
        let errors = check_config(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_upstream_in_location_error() {
        let content = r#"
http {
    server {
        location / {
            upstream backend {
                server 127.0.0.1:8080;
            }
        }
    }
}
"#;
        let errors = check_config(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0]
                .message
                .contains("'upstream' directive cannot be inside 'location'")
        );
    }

    #[test]
    fn test_if_in_location() {
        let content = r#"
http {
    server {
        location / {
            if ($request_method = POST) {
                return 405;
            }
        }
    }
}
"#;
        let errors = check_config(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_if_in_http_error() {
        let content = r#"
http {
    if ($host = "example.com") {
        return 301 https://www.example.com$request_uri;
    }
}
"#;
        let errors = check_config(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0]
                .message
                .contains("'if' directive cannot be inside 'http'")
        );
    }

    #[test]
    fn test_limit_except_in_location() {
        let content = r#"
http {
    server {
        location / {
            limit_except GET POST {
                deny all;
            }
        }
    }
}
"#;
        let errors = check_config(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_limit_except_in_server_error() {
        let content = r#"
http {
    server {
        limit_except GET {
            deny all;
        }
    }
}
"#;
        let errors = check_config(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0]
                .message
                .contains("'limit_except' directive cannot be inside 'server'")
        );
    }

    #[test]
    fn test_map_in_http() {
        let content = r#"
http {
    map $uri $new_uri {
        default 0;
    }
}
"#;
        let errors = check_config(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_geo_in_http() {
        let content = r#"
http {
    geo $country {
        default unknown;
    }
}
"#;
        let errors = check_config(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_server_at_root_error() {
        let content = r#"
server {
    listen 80;
}
"#;
        let errors = check_config(content);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0]
                .message
                .contains("'server' directive must be inside one of:")
        );
        assert!(errors[0].message.contains("not in main context"));
    }

    #[test]
    fn test_stream_at_root() {
        let content = r#"
stream {
    server {
        listen 12345;
    }
}
"#;
        let errors = check_config(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_mail_at_root() {
        let content = r#"
mail {
    server {
        listen 25;
    }
}
"#;
        let errors = check_config(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    // Tests for include context

    #[test]
    fn test_include_context_location_in_server() {
        // File included from server context, location is valid
        let content = r#"
location / {
    root /var/www;
}
"#;
        let context = vec!["http".to_string(), "server".to_string()];
        let errors = check_config_with_context(content, context);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_include_context_server_in_server_error() {
        // File included from server context, another server is invalid
        let content = r#"
server {
    listen 8080;
}
"#;
        let context = vec!["http".to_string(), "server".to_string()];
        let errors = check_config_with_context(content, context);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0]
                .message
                .contains("'server' directive cannot be inside 'server'")
        );
    }

    #[test]
    fn test_include_context_location_in_http_error() {
        // File included from http context, location without server is invalid
        let content = r#"
location / {
    root /var/www;
}
"#;
        let context = vec!["http".to_string()];
        let errors = check_config_with_context(content, context);
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(
            errors[0]
                .message
                .contains("'location' directive cannot be inside 'http'")
        );
    }

    #[test]
    fn test_include_context_server_in_http() {
        // File included from http context, server is valid
        let content = r#"
server {
    listen 80;
    location / {
        root /var/www;
    }
}
"#;
        let context = vec!["http".to_string()];
        let errors = check_config_with_context(content, context);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_include_context_empty_root() {
        // File included at root context, http/events are valid
        let content = r#"
http {
    server {
        listen 80;
    }
}
"#;
        let context = vec![];
        let errors = check_config_with_context(content, context);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    // Tests for additional_contexts (extension modules like nginx-rtmp-module)

    #[test]
    fn test_additional_contexts_rtmp_server() {
        // Without additional_contexts, server inside rtmp should error
        let content = r#"
rtmp {
    server {
        listen 1935;
    }
}
"#;
        let errors = check_config(content);
        assert_eq!(
            errors.len(),
            1,
            "Expected 1 error without additional_contexts, got: {:?}",
            errors
        );
        assert!(
            errors[0]
                .message
                .contains("'server' directive cannot be inside 'rtmp'")
        );

        // With additional_contexts, server inside rtmp should be valid
        let mut additional = HashMap::new();
        additional.insert("server".to_string(), vec!["rtmp".to_string()]);
        let errors = check_config_with_additional_contexts(content, additional);
        assert!(
            errors.is_empty(),
            "Expected no errors with additional_contexts, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_additional_contexts_multiple_directives() {
        // Test multiple directives with additional contexts
        let content = r#"
rtmp {
    server {
        listen 1935;
        application live {
            live on;
        }
    }
}
"#;
        // Add server and application as valid inside rtmp
        let mut additional = HashMap::new();
        additional.insert("server".to_string(), vec!["rtmp".to_string()]);
        // application is not a built-in directive, so we define its valid contexts
        additional.insert("application".to_string(), vec!["server".to_string()]);
        let errors = check_config_with_additional_contexts(content, additional);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }
}
