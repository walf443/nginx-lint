//! alias-location-slash-mismatch plugin
//!
//! This plugin warns when the `alias` directive path doesn't end with a trailing slash,
//! but only when the parent `location` directive ends with a trailing slash.
//!
//! Without a trailing slash, nginx may not correctly append the URI to the alias path,
//! leading to unexpected behavior or 404 errors.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check for alias directive without trailing slash
#[derive(Default)]
pub struct AliasLocationSlashMismatchPlugin;

impl AliasLocationSlashMismatchPlugin {
    /// Check if a string ends with a variable reference like $1, $2, $uri, etc.
    fn ends_with_variable(s: &str) -> bool {
        // Find the last $ in the string
        if let Some(dollar_pos) = s.rfind('$') {
            // Check if everything after $ is alphanumeric or underscore
            let after_dollar = &s[dollar_pos + 1..];
            !after_dollar.is_empty()
                && after_dollar
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_')
        } else {
            false
        }
    }

    /// Get the location path from a location directive
    fn get_location_path(directive: &Directive) -> Option<String> {
        if directive.name != "location" {
            return None;
        }

        // location can have modifiers: location [=|~|~*|^~] uri { ... }
        // We need to get the actual URI path
        for arg in &directive.args {
            let value = match &arg.value {
                ArgumentValue::Literal(s) => s.clone(),
                ArgumentValue::QuotedString(s) => s.clone(),
                ArgumentValue::SingleQuotedString(s) => s.clone(),
                ArgumentValue::Variable(s) => format!("${}", s),
            };

            // Skip modifiers
            if value == "=" || value == "~" || value == "~*" || value == "^~" {
                continue;
            }

            return Some(value);
        }

        None
    }

    /// Check if location is a regex location
    fn is_regex_location(directive: &Directive) -> bool {
        if directive.name != "location" {
            return false;
        }

        for arg in &directive.args {
            if let ArgumentValue::Literal(s) = &arg.value {
                if s == "~" || s == "~*" {
                    return true;
                }
            }
        }

        false
    }

    /// Recursively check for alias directives without trailing slash
    fn check_items(
        &self,
        items: &[ConfigItem],
        location_ends_with_slash: bool,
        is_regex_location: bool,
        errors: &mut Vec<LintError>,
    ) {
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                if directive.name == "alias" && location_ends_with_slash {
                    if let Some(path) = directive.first_arg() {
                        // Don't warn if path ends with slash
                        if path.ends_with('/') {
                            continue;
                        }

                        // Don't warn if path ends with a variable (like $1 in regex locations)
                        if Self::ends_with_variable(path) {
                            continue;
                        }

                        let err = PluginInfo::new(
                            "alias-location-slash-mismatch",
                            "best-practices",
                            "",
                        ).error_builder();

                        let mut error = err.warning_at(
                            &format!(
                                "alias path '{}' should end with a trailing slash when location ends with '/'",
                                path
                            ),
                            directive,
                        );

                        // Add autofix: append trailing slash (only for non-regex locations)
                        if !is_regex_location {
                            if let Some(arg) = directive.args.first() {
                                match &arg.value {
                                    ArgumentValue::QuotedString(_) | ArgumentValue::SingleQuotedString(_) => {
                                        // Insert before closing quote
                                        let fix_start = arg.span.end.offset - 1;
                                        let fix_end = arg.span.end.offset - 1;
                                        error = error.with_fix(Fix::replace_range(fix_start, fix_end, "/"));
                                    }
                                    ArgumentValue::Literal(_) => {
                                        // Append at end of literal
                                        let end = arg.span.end.offset;
                                        error = error.with_fix(Fix::replace_range(end, end, "/"));
                                    }
                                    ArgumentValue::Variable(_) => {
                                        // Don't autofix variables
                                    }
                                }
                            }
                        }

                        errors.push(error);
                    }
                }

                // Recurse into blocks
                if let Some(block) = &directive.block {
                    if directive.name == "location" {
                        let loc_path = Self::get_location_path(directive);
                        let ends_with_slash = loc_path.as_ref().map_or(false, |p| p.ends_with('/'));
                        let is_regex = Self::is_regex_location(directive);
                        self.check_items(&block.items, ends_with_slash, is_regex, errors);
                    } else {
                        self.check_items(&block.items, location_ends_with_slash, is_regex_location, errors);
                    }
                }
            }
        }
    }
}

impl Plugin for AliasLocationSlashMismatchPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "alias-location-slash-mismatch",
            "best-practices",
            "Warns when alias path doesn't end with '/' when location ends with '/'",
        )
        .with_severity("warning")
        .with_why(
            "The `alias` directive replaces the matched location prefix with the specified path. \
             When the location ends with a trailing slash, the alias should also end with a \
             trailing slash. Otherwise, the URI portion after the location match may not be \
             correctly appended to the alias path, potentially causing 404 errors or serving \
             wrong files.\n\n\
             For example, with `location /images/ { alias /data/images; }`, a request to \
             `/images/photo.jpg` would try to access `/data/imagesphoto.jpg` instead of \
             `/data/images/photo.jpg`.\n\n\
             For regex locations using capture groups (e.g., `alias /data/$1`), this rule \
             does not warn since the variable handles the path correctly.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_core_module.html#alias".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        // Start with location_ends_with_slash=false and is_regex_location=false
        self.check_items(&config.items, false, false, &mut errors);
        errors
    }
}

// Export the plugin
nginx_lint_plugin::export_plugin!(AliasLocationSlashMismatchPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::PluginTestRunner;
    use nginx_lint_plugin::parse_string;

    #[test]
    fn test_alias_without_trailing_slash_warns() {
        let config = parse_string(
            r#"
http {
    server {
        listen 80;

        location /images/ {
            alias /data/images;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = AliasLocationSlashMismatchPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("trailing slash"));
    }

    #[test]
    fn test_alias_with_trailing_slash_ok() {
        let runner = PluginTestRunner::new(AliasLocationSlashMismatchPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        listen 80;

        location /images/ {
            alias /data/images/;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_alias_quoted_without_trailing_slash_warns() {
        let config = parse_string(
            r#"
http {
    server {
        listen 80;

        location /files/ {
            alias "/var/www/files";
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = AliasLocationSlashMismatchPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_alias_quoted_with_trailing_slash_ok() {
        let runner = PluginTestRunner::new(AliasLocationSlashMismatchPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        listen 80;

        location /files/ {
            alias "/var/www/files/";
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_multiple_aliases_warns() {
        let config = parse_string(
            r#"
http {
    server {
        listen 80;

        location /images/ {
            alias /data/images;
        }

        location /docs/ {
            alias /data/docs;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = AliasLocationSlashMismatchPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
    }

    #[test]
    fn test_alias_with_fix() {
        let config = parse_string(
            r#"
http {
    server {
        location /images/ {
            alias /data/images;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = AliasLocationSlashMismatchPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].fix.is_some(), "Expected fix to be present");
    }

    #[test]
    fn test_location_without_trailing_slash_no_warn() {
        let runner = PluginTestRunner::new(AliasLocationSlashMismatchPlugin);

        // When location doesn't end with /, alias doesn't need to end with /
        runner.assert_no_errors(
            r#"
http {
    server {
        listen 80;

        location /images {
            alias /data/images;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_regex_location_with_capture_group_no_warn() {
        let runner = PluginTestRunner::new(AliasLocationSlashMismatchPlugin);

        // Regex location with $1 should not warn
        runner.assert_no_errors(
            r#"
http {
    server {
        listen 80;

        location ~ ^/images/(.*)$ {
            alias /data/images/$1;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_regex_location_case_insensitive_with_variable_no_warn() {
        let runner = PluginTestRunner::new(AliasLocationSlashMismatchPlugin);

        // ~* is case-insensitive regex
        runner.assert_no_errors(
            r#"
http {
    server {
        listen 80;

        location ~* ^/images/(.*)$ {
            alias /data/images/$1;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(AliasLocationSlashMismatchPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }
}
