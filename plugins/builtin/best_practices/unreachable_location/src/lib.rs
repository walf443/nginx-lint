//! unreachable-location plugin
//!
//! This plugin detects location blocks that will never be evaluated due to
//! nginx's location matching rules.
//!
//! nginx location matching priority:
//! 1. Exact match (`=`) - highest priority, stops search
//! 2. Prefix match with `^~` - stops regex search if longest match
//! 3. Regex matches (`~`, `~*`) - first match in config order wins
//! 4. Regular prefix matches - longest match wins
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;
use std::collections::HashMap;

/// Check for unreachable location blocks
#[derive(Default)]
pub struct UnreachableLocationPlugin;

/// Represents a parsed location directive
#[derive(Debug, Clone)]
struct LocationInfo {
    /// The modifier (=, ~, ~*, ^~, or empty)
    modifier: String,
    /// The path or pattern
    pattern: String,
    /// Line number for error reporting
    line: usize,
    /// Column number for error reporting
    column: usize,
    /// Original full location string for display
    display: String,
}

impl LocationInfo {
    fn from_directive(directive: &Directive) -> Option<Self> {
        if directive.name != "location" {
            return None;
        }

        let args: Vec<String> = directive.args.iter().map(|a| a.as_str().to_string()).collect();
        if args.is_empty() {
            return None;
        }

        let (modifier, pattern): (String, String) = if args.len() >= 2 {
            match args[0].as_str() {
                "=" | "~" | "~*" | "^~" => (args[0].clone(), args[1].clone()),
                _ => (String::new(), args[0].clone()),
            }
        } else {
            (String::new(), args[0].clone())
        };

        let display: String = if modifier.is_empty() {
            pattern.clone()
        } else {
            format!("{} {}", modifier, pattern)
        };

        Some(LocationInfo {
            modifier,
            pattern,
            line: directive.span.start.line,
            column: directive.span.start.column,
            display,
        })
    }

    fn is_regex(&self) -> bool {
        self.modifier == "~" || self.modifier == "~*"
    }

    fn is_prefix_no_regex(&self) -> bool {
        self.modifier == "^~"
    }
}

impl UnreachableLocationPlugin {
    /// Check locations within a server block
    fn check_server_locations(&self, items: &[ConfigItem], errors: &mut Vec<LintError>) {
        let mut locations: Vec<LocationInfo> = Vec::new();

        // Collect all location directives
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                if let Some(loc_info) = LocationInfo::from_directive(directive) {
                    locations.push(loc_info);
                }
            }
        }

        // Check for unreachable locations
        self.check_duplicate_locations(&locations, errors);
        self.check_regex_order(&locations, errors);
        self.check_prefix_no_regex_shadowing(&locations, errors);
    }

    /// Check for duplicate location paths (same modifier and pattern)
    fn check_duplicate_locations(&self, locations: &[LocationInfo], errors: &mut Vec<LintError>) {
        let mut seen: HashMap<String, &LocationInfo> = HashMap::new();

        for loc in locations {
            let key = format!("{}:{}", loc.modifier, loc.pattern);
            if let Some(first) = seen.get(&key) {
                errors.push(LintError::warning(
                    "unreachable-location",
                    "best-practices",
                    &format!(
                        "Duplicate location '{}' (first defined on line {})",
                        loc.display, first.line
                    ),
                    loc.line,
                    loc.column,
                ));
            } else {
                seen.insert(key, loc);
            }
        }
    }

    /// Check for regex locations that will never match due to order
    fn check_regex_order(&self, locations: &[LocationInfo], errors: &mut Vec<LintError>) {
        let regex_locations: Vec<&LocationInfo> = locations.iter().filter(|l| l.is_regex()).collect();

        for (i, loc) in regex_locations.iter().enumerate() {
            for earlier in &regex_locations[..i] {
                // Check if earlier regex would always match what this one matches
                if self.regex_shadows(earlier, loc) {
                    errors.push(LintError::warning(
                        "unreachable-location",
                        "best-practices",
                        &format!(
                            "Location '{}' may never match because '{}' (line {}) matches first",
                            loc.display, earlier.display, earlier.line
                        ),
                        loc.line,
                        loc.column,
                    ));
                }
            }
        }
    }

    /// Check if earlier regex shadows later regex
    /// This is a heuristic check for common patterns
    fn regex_shadows(&self, earlier: &LocationInfo, later: &LocationInfo) -> bool {
        // If earlier pattern is .* or similar catch-all
        if earlier.pattern == ".*" || earlier.pattern == "^.*" || earlier.pattern == "." {
            return true;
        }

        // If later pattern is more specific version of earlier
        // e.g., earlier: /api/.* later: /api/v1/.*
        if later.pattern.starts_with(&earlier.pattern.trim_end_matches(".*"))
            && earlier.pattern.ends_with(".*")
            && later.pattern.len() > earlier.pattern.len() {
            return true;
        }

        // Check for prefix patterns like /foo vs /foo/bar
        // Earlier: ~ /api  Later: ~ /api/v1
        let earlier_base = earlier.pattern.trim_start_matches('^').trim_end_matches('$');
        let later_base = later.pattern.trim_start_matches('^').trim_end_matches('$');

        if !earlier_base.contains('*') && !earlier_base.contains('+') && !earlier_base.contains('?') {
            // Earlier is a literal pattern
            if later_base.starts_with(earlier_base) && later_base.len() > earlier_base.len() {
                return true;
            }
        }

        false
    }

    /// Check if ^~ prefix locations shadow regex locations
    fn check_prefix_no_regex_shadowing(&self, locations: &[LocationInfo], errors: &mut Vec<LintError>) {
        let prefix_no_regex: Vec<&LocationInfo> = locations.iter()
            .filter(|l| l.is_prefix_no_regex())
            .collect();

        let regex_locations: Vec<&LocationInfo> = locations.iter()
            .filter(|l| l.is_regex())
            .collect();

        for regex_loc in &regex_locations {
            for prefix_loc in &prefix_no_regex {
                if self.prefix_might_shadow_regex(prefix_loc, regex_loc) {
                    errors.push(LintError::info(
                        "unreachable-location",
                        "best-practices",
                        &format!(
                            "Location '{}' may not match paths under '{}' due to ^~ modifier (line {})",
                            regex_loc.display, prefix_loc.pattern, prefix_loc.line
                        ),
                        regex_loc.line,
                        regex_loc.column,
                    ));
                }
            }
        }
    }

    /// Check if a ^~ prefix might shadow a regex location
    fn prefix_might_shadow_regex(&self, prefix: &LocationInfo, regex: &LocationInfo) -> bool {
        // Extract literal prefix from regex if possible
        let regex_pattern = &regex.pattern;

        // Common file extension patterns
        if regex_pattern.contains(r"\.") {
            // e.g., ~* \.(gif|jpg|png)$ might be shadowed by ^~ /images/
            // This is a heuristic - we can't fully analyze regex
            let prefix_path = &prefix.pattern;

            // If the ^~ prefix is something like /static/ or /images/
            // and the regex matches file extensions, there might be shadowing
            if prefix_path.ends_with('/') {
                return true;
            }
        }

        // If regex starts with the prefix path
        let regex_literal = regex_pattern
            .trim_start_matches('^')
            .split(|c: char| !c.is_alphanumeric() && c != '/' && c != '_' && c != '-')
            .next()
            .unwrap_or("");

        if !regex_literal.is_empty() && regex_literal.starts_with(&prefix.pattern) {
            return true;
        }

        false
    }

    /// Recursively check all server blocks
    fn check_items(&self, items: &[ConfigItem], errors: &mut Vec<LintError>) {
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                if directive.name == "server" {
                    if let Some(block) = &directive.block {
                        self.check_server_locations(&block.items, errors);
                    }
                }

                // Recurse into http block
                if let Some(block) = &directive.block {
                    self.check_items(&block.items, errors);
                }
            }
        }
    }
}

impl Plugin for UnreachableLocationPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "unreachable-location",
            "best-practices",
            "Detects location blocks that may never be evaluated",
        )
        .with_severity("warning")
        .with_why(
            "nginx's location matching follows specific rules:\n\
             1. Exact match (`=`) has highest priority\n\
             2. `^~` prefix match stops regex search\n\
             3. Regex matches (`~`, `~*`) are checked in config order\n\
             4. Regular prefix matches use longest match\n\n\
             Due to these rules, some location blocks may never be reached:\n\
             - Duplicate locations with the same path\n\
             - Regex locations shadowed by earlier, broader regex\n\
             - Regex locations that can't match due to `^~` prefix",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_core_module.html#location".to_string(),
            "https://www.nginx.com/resources/wiki/start/topics/tutorials/config_pitfalls/#taxing-rewrites".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        self.check_items(&config.items, &mut errors);
        errors
    }
}

// Export the plugin
nginx_lint::export_plugin!(UnreachableLocationPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::PluginTestRunner;

    #[test]
    fn test_duplicate_exact_location() {
        let runner = PluginTestRunner::new(UnreachableLocationPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location = /favicon.ico {
            return 204;
        }
        location = /favicon.ico {
            return 404;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_duplicate_prefix_location() {
        let runner = PluginTestRunner::new(UnreachableLocationPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location /api {
            proxy_pass http://backend;
        }
        location /api {
            proxy_pass http://other;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_regex_order_broad_first() {
        let runner = PluginTestRunner::new(UnreachableLocationPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location ~ /api {
            proxy_pass http://backend;
        }
        location ~ /api/v1 {
            proxy_pass http://v1;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_regex_order_specific_first_ok() {
        let runner = PluginTestRunner::new(UnreachableLocationPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location ~ /api/v1 {
            proxy_pass http://v1;
        }
        location ~ /api {
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_different_locations_ok() {
        let runner = PluginTestRunner::new(UnreachableLocationPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            root /var/www;
        }
        location /api {
            proxy_pass http://backend;
        }
        location /static {
            alias /var/static;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_same_path_different_modifier_ok() {
        let runner = PluginTestRunner::new(UnreachableLocationPlugin);

        // Different modifiers are different locations
        runner.assert_no_errors(
            r#"
http {
    server {
        location = /api {
            return 200;
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
    fn test_examples() {
        let runner = PluginTestRunner::new(UnreachableLocationPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }
}
