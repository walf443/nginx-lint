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

use nginx_lint_plugin::prelude::*;
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

        let args: Vec<String> = directive
            .args
            .iter()
            .map(|a| a.as_str().to_string())
            .collect();
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
            if let ConfigItem::Directive(directive) = item
                && let Some(loc_info) = LocationInfo::from_directive(directive)
            {
                locations.push(loc_info);
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
        let err = PluginSpec::new("unreachable-location", "best-practices", "").error_builder();

        for loc in locations {
            let key = format!("{}:{}", loc.modifier, loc.pattern);
            if let Some(first) = seen.get(&key) {
                errors.push(err.warning(
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
        let regex_locations: Vec<&LocationInfo> =
            locations.iter().filter(|l| l.is_regex()).collect();
        let err = PluginSpec::new("unreachable-location", "best-practices", "").error_builder();

        for (i, loc) in regex_locations.iter().enumerate() {
            for earlier in &regex_locations[..i] {
                // Check if earlier regex would always match what this one matches
                if self.regex_shadows(earlier, loc) {
                    errors.push(err.warning(
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

    /// Check if earlier regex shadows later regex.
    /// This is a heuristic check for common patterns.
    fn regex_shadows(&self, earlier: &LocationInfo, later: &LocationInfo) -> bool {
        if self.is_catchall_regex(&earlier.pattern) {
            return true;
        }

        // If later pattern is more specific version of earlier
        // e.g., earlier: /api/.* later: /api/v1/.*
        if earlier.pattern.ends_with(".*")
            && later
                .pattern
                .starts_with(earlier.pattern.trim_end_matches(".*"))
            && later.pattern.len() > earlier.pattern.len()
        {
            return true;
        }

        // Check for prefix patterns like /foo vs /foo/bar
        // Earlier: ~ /api  Later: ~ /api/v1
        let earlier_base = earlier
            .pattern
            .trim_start_matches('^')
            .trim_end_matches('$');
        let later_base = later.pattern.trim_start_matches('^').trim_end_matches('$');

        if !earlier_base.contains('*')
            && !earlier_base.contains('+')
            && !earlier_base.contains('?')
            && later_base.starts_with(earlier_base)
            && later_base.len() > earlier_base.len()
        {
            return true;
        }

        false
    }

    /// Check if ^~ prefix locations shadow regex locations
    fn check_prefix_no_regex_shadowing(
        &self,
        locations: &[LocationInfo],
        errors: &mut Vec<LintError>,
    ) {
        let prefix_no_regex: Vec<&LocationInfo> = locations
            .iter()
            .filter(|l| l.is_prefix_no_regex())
            .collect();

        let regex_locations: Vec<&LocationInfo> =
            locations.iter().filter(|l| l.is_regex()).collect();

        let err = PluginSpec::new("unreachable-location", "best-practices", "").error_builder();

        for regex_loc in &regex_locations {
            for prefix_loc in &prefix_no_regex {
                if self.prefix_might_shadow_regex(prefix_loc, regex_loc) {
                    errors.push(err.warning(
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

    /// Check if a ^~ prefix might shadow a regex location.
    ///
    /// A `^~` prefix prevents regex evaluation for any URI that matches the prefix.
    /// This checks four scenarios:
    /// 1. `^~ /` matches all URIs
    /// 2. Catch-all regex (e.g., `.*`) is always shadowed
    /// 3. Regex's literal prefix overlaps with `^~` path
    /// 4. Global extension patterns (e.g., `\.(css|js)$`) are shadowed by any `^~`
    fn prefix_might_shadow_regex(&self, prefix: &LocationInfo, regex: &LocationInfo) -> bool {
        let prefix_path = &prefix.pattern;
        let regex_pattern = &regex.pattern;

        prefix_path == "/"
            || self.is_catchall_regex(regex_pattern)
            || self.prefix_and_regex_paths_overlap(prefix_path, regex_pattern)
            || self.is_global_extension_pattern(regex_pattern)
    }

    // =========================================================================
    // Regex pattern analysis utilities
    // =========================================================================

    /// Check if a regex pattern is a catch-all that matches any URI.
    /// e.g., `.*`, `^.*$`, `.`, `.+`
    fn is_catchall_regex(&self, pattern: &str) -> bool {
        let normalized = pattern.trim_start_matches('^').trim_end_matches('$');
        normalized == ".*" || normalized == "." || normalized == ".+"
    }

    /// Check if a `^~` prefix path and a regex pattern have overlapping paths.
    ///
    /// Extracts the literal prefix from the regex and checks bidirectional overlap:
    /// - Regex literal starts with prefix → regex is fully under `^~` scope
    ///   (e.g., `^~ /static/` shadows `~ ^/static/.*\.css$`)
    /// - Prefix starts with regex literal → partial overlap
    ///   (e.g., `^~ /images/photos/` shadows `~ /images/`)
    fn prefix_and_regex_paths_overlap(&self, prefix_path: &str, regex_pattern: &str) -> bool {
        let regex_literal = self.extract_regex_literal_prefix(regex_pattern);
        if regex_literal.is_empty() {
            return false;
        }
        regex_literal.starts_with(prefix_path) || prefix_path.starts_with(&regex_literal)
    }

    /// Extract the literal prefix from a regex pattern.
    ///
    /// Scans from the start collecting path-safe literal characters, stopping at
    /// the first regex metacharacter. Escaped path characters (e.g., `\.`, `\/`)
    /// are treated as literals; unescaped `.` is treated as a wildcard.
    ///
    /// Examples:
    /// - `^/static/.*\.css$` → `/static/`
    /// - `/images/` → `/images/`
    /// - `\.(css|js)$` → `.`
    fn extract_regex_literal_prefix(&self, pattern: &str) -> String {
        let s = pattern.trim_start_matches('^');
        let mut result = String::new();
        let mut chars = s.chars().peekable();

        while let Some(&c) = chars.peek() {
            if c == '\\' {
                chars.next();
                if let Some(&next) = chars.peek() {
                    if is_escaped_path_literal(next) {
                        result.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
            } else if is_plain_path_char(c) {
                result.push(c);
                chars.next();
            } else {
                break;
            }
        }

        result
    }

    /// Check if a regex pattern is a global file extension pattern (no path prefix).
    /// e.g., `\.(jpg|png|gif)$` or `.*\.(css|js)$`
    fn is_global_extension_pattern(&self, regex_pattern: &str) -> bool {
        let s = regex_pattern.trim_start_matches('^');

        // Must not start with a path prefix
        if s.starts_with('/') {
            return false;
        }

        // Must start like a pure extension pattern: `\.` or `.*\.`
        if !(s.starts_with(r"\.") || s.starts_with(r".*\.")) {
            return false;
        }

        // Ensure no `/` after `\.` to avoid matching path patterns like `\.well-known/`
        if let Some(dot_idx) = s.find(r"\.") {
            let after_dot = &s[dot_idx + 2..];
            if after_dot.contains('/') {
                return false;
            }
        }

        // Typical extension patterns end with `$` or include a group/class
        // e.g., `\.(jpg|png)$`, `\.[a-z]+$`
        s.ends_with('$') || s.contains('(') || s.contains('[')
    }

    /// Recursively check all server blocks
    fn check_items(&self, items: &[ConfigItem], errors: &mut Vec<LintError>) {
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                if directive.name == "server"
                    && let Some(block) = &directive.block
                {
                    self.check_server_locations(&block.items, errors);
                }

                // Recurse into http block
                if let Some(block) = &directive.block {
                    self.check_items(&block.items, errors);
                }
            }
        }
    }
}

// =========================================================================
// Character classification helpers for regex literal prefix extraction
// =========================================================================

/// Check if a character is a plain path-safe literal (not a regex metacharacter).
fn is_plain_path_char(c: char) -> bool {
    c.is_alphanumeric() || c == '/' || c == '_' || c == '-'
}

/// Check if an escaped character represents a literal in path context.
/// e.g., `\.` `\/` `\-` `\_` and escaped alphanumerics are literals,
/// while `\d` `\w` `\s` etc. are regex character classes (but alphanumeric,
/// so they pass through — acceptable for our heuristic since paths rarely
/// contain `\d` style sequences).
fn is_escaped_path_literal(c: char) -> bool {
    c == '/' || c == '.' || c == '_' || c == '-' || c.is_alphanumeric()
}

impl Plugin for UnreachableLocationPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
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
            "https://github.com/walf443/nginx-lint/blob/main/plugins/builtin/best_practices/unreachable_location/tests/container_test.rs".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        // If included from a server context, check top-level locations directly
        if config.is_included_from_http_server() {
            self.check_server_locations(&config.items, &mut errors);
        }

        self.check_items(&config.items, &mut errors);
        errors
    }
}

// Export the plugin
nginx_lint_plugin::export_plugin!(UnreachableLocationPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::PluginTestRunner;

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

    // =========================================================================
    // Include context tests
    // =========================================================================

    #[test]
    fn test_include_context_from_server_duplicate_location() {
        // Test that duplicate locations are detected when file is included from server
        use nginx_lint_plugin::parse_string;

        let mut config = parse_string(
            r#"
location /api {
    proxy_pass http://backend1;
}
location /api {
    proxy_pass http://backend2;
}
"#,
        )
        .unwrap();

        // Simulate being included from http > server context
        config.include_context = vec!["http".to_string(), "server".to_string()];

        let plugin = UnreachableLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(
            errors.len(),
            1,
            "Expected 1 error for duplicate location, got: {:?}",
            errors
        );
        assert!(errors[0].message.contains("Duplicate location"));
    }

    #[test]
    fn test_include_context_from_server_regex_order() {
        // Test that regex order issues are detected when file is included from server
        use nginx_lint_plugin::parse_string;

        let mut config = parse_string(
            r#"
location ~ /api {
    proxy_pass http://backend;
}
location ~ /api/v1 {
    proxy_pass http://v1;
}
"#,
        )
        .unwrap();

        // Simulate being included from http > server context
        config.include_context = vec!["http".to_string(), "server".to_string()];

        let plugin = UnreachableLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(
            errors.len(),
            1,
            "Expected 1 error for shadowed regex location, got: {:?}",
            errors
        );
        assert!(errors[0].message.contains("may never match"));
    }

    #[test]
    fn test_include_context_from_http_no_error() {
        // Test that locations at http level (not server) don't trigger
        use nginx_lint_plugin::parse_string;

        let mut config = parse_string(
            r#"
location /api {
    proxy_pass http://backend1;
}
location /api {
    proxy_pass http://backend2;
}
"#,
        )
        .unwrap();

        // Simulate being included from http context only (not server)
        config.include_context = vec!["http".to_string()];

        let plugin = UnreachableLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should NOT trigger because we're not in a server context
        assert!(
            errors.is_empty(),
            "Expected no errors for locations in http context, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_include_context_different_locations_ok() {
        // Test that different locations don't trigger when included from server
        use nginx_lint_plugin::parse_string;

        let mut config = parse_string(
            r#"
location / {
    root /var/www;
}
location /api {
    proxy_pass http://backend;
}
"#,
        )
        .unwrap();

        // Simulate being included from http > server context
        config.include_context = vec!["http".to_string(), "server".to_string()];

        let plugin = UnreachableLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert!(
            errors.is_empty(),
            "Expected no errors for different locations, got: {:?}",
            errors
        );
    }

    // =========================================================================
    // ^~ prefix shadowing regex - improved detection tests
    // =========================================================================

    #[test]
    fn test_prefix_no_regex_without_trailing_slash() {
        // ^~ /static (no trailing slash) should shadow ~* \.(css|js)$
        let runner = PluginTestRunner::new(UnreachableLocationPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location ^~ /static {
            alias /var/static;
        }
        location ~* \.(css|js)$ {
            expires 30d;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_prefix_no_regex_root_shadows_all() {
        // ^~ / should shadow any regex
        let runner = PluginTestRunner::new(UnreachableLocationPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location ^~ / {
            root /var/www;
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
    fn test_prefix_no_regex_catchall_regex() {
        // ^~ /images/ should shadow ~ .*
        let runner = PluginTestRunner::new(UnreachableLocationPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location ^~ /images/ {
            alias /var/images;
        }
        location ~ .* {
            return 404;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_prefix_no_regex_longer_prefix_shadows_shorter_regex() {
        // ^~ /images/photos should shadow ~ /images/
        let runner = PluginTestRunner::new(UnreachableLocationPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location ^~ /images/photos {
            alias /var/photos;
        }
        location ~ /images/ {
            return 200;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_prefix_no_regex_regex_under_prefix_path() {
        // ^~ /static/ should shadow ~ ^/static/.*\.css$
        let runner = PluginTestRunner::new(UnreachableLocationPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location ^~ /static/ {
            alias /var/static;
        }
        location ~ ^/static/.*\.css$ {
            expires 30d;
        }
    }
}
"#,
        );
    }
}
