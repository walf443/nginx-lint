//! root-in-location plugin
//!
//! This plugin warns when the `root` directive is used inside a `location` block.
//!
//! The recommended practice is to define `root` at the server level and use
//! `alias` inside location blocks when a different path is needed.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check for root directive inside location blocks
#[derive(Default)]
pub struct RootInLocationPlugin;

impl RootInLocationPlugin {
    /// Recursively check for root directives inside location blocks
    fn check_items(
        &self,
        items: &[ConfigItem],
        in_location: bool,
        err: &ErrorBuilder,
        errors: &mut Vec<LintError>,
    ) {
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                // Check if we're in a location block and found a root directive
                if in_location && directive.name == "root" {
                    errors.push(err.warning_at(
                        "root directive inside location block; consider defining root at server level and using alias in location blocks",
                        directive,
                    ));
                }

                // Recurse into blocks
                if let Some(block) = &directive.block {
                    let is_location = directive.name == "location";
                    // Once we're in a location, stay in_location for nested blocks
                    self.check_items(&block.items, in_location || is_location, err, errors);
                }
            }
        }
    }
}

impl Plugin for RootInLocationPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "root-in-location",
            "best-practices",
            "Warns when root directive is used inside location blocks",
        )
        .with_severity("warning")
        .with_why(
            "Defining `root` inside location blocks can lead to confusion and maintenance issues. \
             The recommended practice is to define `root` at the server level, which applies to \
             all locations by default. When a location needs a different document root, use the \
             `alias` directive instead, which is more explicit about its purpose.\n\n\
             Using `root` at server level also helps avoid the common pitfall of forgetting to \
             define `root` in some location blocks.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_core_module.html#root".to_string(),
            "https://nginx.org/en/docs/http/ngx_http_core_module.html#alias".to_string(),
            "https://github.com/walf443/nginx-lint/blob/main/plugins/builtin/best_practices/root_in_location/tests/container_test.rs".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();
        // Check if this file is included from within a location context
        let in_location = config.is_included_from_http_location();
        self.check_items(&config.items, in_location, &err, &mut errors);
        errors
    }
}

// Export the plugin
nginx_lint_plugin::export_plugin!(RootInLocationPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::parse_string;
    use nginx_lint_plugin::testing::PluginTestRunner;

    #[test]
    fn test_root_in_location_warns() {
        let config = parse_string(
            r#"
http {
    server {
        listen 80;

        location / {
            root /var/www/html;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = RootInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("root directive inside location"));
    }

    #[test]
    fn test_root_at_server_level_ok() {
        let runner = PluginTestRunner::new(RootInLocationPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        listen 80;
        root /var/www/html;

        location / {
            try_files $uri $uri/ =404;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_alias_in_location_ok() {
        let runner = PluginTestRunner::new(RootInLocationPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        listen 80;
        root /var/www/html;

        location /images/ {
            alias /data/images/;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_root_in_nested_location_warns() {
        let config = parse_string(
            r#"
http {
    server {
        listen 80;

        location /api {
            location /api/v1 {
                root /var/www/api;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = RootInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_root_in_if_inside_location_warns() {
        let config = parse_string(
            r#"
http {
    server {
        listen 80;

        location / {
            if ($host = "example.com") {
                root /var/www/example;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = RootInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_multiple_locations_with_root() {
        let config = parse_string(
            r#"
http {
    server {
        listen 80;

        location / {
            root /var/www/main;
        }

        location /api {
            root /var/www/api;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = RootInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(RootInLocationPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_include_context_from_location() {
        // Test that root directive is detected when file is included from a location block
        let mut config = parse_string(
            r#"
root /var/www/html;
"#,
        )
        .unwrap();

        // Simulate being included from http > server > location context
        config.include_context = vec![
            "http".to_string(),
            "server".to_string(),
            "location".to_string(),
        ];

        let plugin = RootInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(
            errors.len(),
            1,
            "Expected 1 error for root in included file from location, got: {:?}",
            errors
        );
        assert!(errors[0].message.contains("root directive inside location"));
    }

    #[test]
    fn test_include_context_from_server_no_error() {
        // Test that root directive is OK when file is included from a server block (not location)
        let mut config = parse_string(
            r#"
root /var/www/html;
"#,
        )
        .unwrap();

        // Simulate being included from http > server context (not location)
        config.include_context = vec!["http".to_string(), "server".to_string()];

        let plugin = RootInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert!(
            errors.is_empty(),
            "Expected no errors for root in server context, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(RootInLocationPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
