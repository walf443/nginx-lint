//! block-lines plugin
//!
//! This plugin warns when a block directive (server, location, http, etc.)
//! contains too many lines, suggesting it should be split for readability.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Default maximum number of lines allowed in a block
pub const DEFAULT_MAX_LINES: usize = 100;

/// Check for overly long block directives
pub struct BlockLinesPlugin {
    max_lines: usize,
}

impl Default for BlockLinesPlugin {
    fn default() -> Self {
        Self {
            max_lines: DEFAULT_MAX_LINES,
        }
    }
}

impl BlockLinesPlugin {
    /// Create a new plugin with a custom maximum line count
    pub fn with_max_lines(max_lines: usize) -> Self {
        Self { max_lines }
    }
}

impl Plugin for BlockLinesPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "block-lines",
            "style",
            "Warns when a block directive exceeds the maximum number of lines",
        )
        .with_severity("warning")
        .with_why(
            "Blocks with too many lines are difficult to read and maintain. \
             Consider splitting large blocks into smaller files using the include directive.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();
        check_items(&config.items, self.max_lines, &err, &mut errors);
        errors
    }
}

fn check_items(
    items: &[ConfigItem],
    max_lines: usize,
    err: &ErrorBuilder,
    errors: &mut Vec<LintError>,
) {
    for item in items {
        if let ConfigItem::Directive(directive) = item
            && let Some(block) = &directive.block
        {
            // Recursively check nested blocks first
            let errors_before = errors.len();
            check_items(&block.items, max_lines, err, errors);
            let child_exceeded = errors.len() > errors_before;

            // Only report this block if no nested child block already exceeded the threshold
            let line_count = block.span.end.line - directive.span.start.line + 1;
            if line_count > max_lines && !child_exceeded {
                errors.push(err.warning_at(
                    &format!(
                        "'{}' block is {} lines long (max {}), consider splitting with include",
                        directive.name, line_count, max_lines
                    ),
                    directive,
                ));
            }
        }
    }
}

// Export the plugin
nginx_lint_plugin::export_plugin!(BlockLinesPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::PluginTestRunner;

    #[test]
    fn test_short_block_no_error() {
        let runner = PluginTestRunner::new(BlockLinesPlugin::default());
        runner.assert_no_errors(
            r#"
server {
    listen 80;
    server_name example.com;
}
"#,
        );
    }

    #[test]
    fn test_block_at_threshold_no_error() {
        let runner = PluginTestRunner::new(BlockLinesPlugin::default());
        // Generate a block with exactly 100 lines (threshold)
        let mut config = String::from("server {\n");
        for i in 1..=98 {
            config.push_str(&format!("    listen {};\n", i));
        }
        config.push_str("}\n");
        // This block is exactly 100 lines (line 1 "server {" to line 100 "}")
        runner.assert_no_errors(&config);
    }

    #[test]
    fn test_block_exceeds_threshold() {
        let runner = PluginTestRunner::new(BlockLinesPlugin::default());
        // Generate a block with 102 lines (exceeds 100)
        let mut config = String::from("server {\n");
        for i in 1..=100 {
            config.push_str(&format!("    listen {};\n", i));
        }
        config.push_str("}\n");
        // This block is 102 lines
        let errors = runner.check_string(&config).expect("parse failed");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("block is 102 lines long"));
    }

    #[test]
    fn test_nested_blocks() {
        let runner = PluginTestRunner::new(BlockLinesPlugin::default());
        // Outer block is long, inner block is short
        let mut config = String::from("http {\n");
        config.push_str("    server {\n");
        config.push_str("        listen 80;\n");
        config.push_str("    }\n");
        for i in 1..=98 {
            config.push_str(&format!("    server_name example{}.com;\n", i));
        }
        config.push_str("}\n");
        // http block is 102 lines, server block is only 3 lines
        let errors = runner.check_string(&config).expect("parse failed");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("'http' block"));
    }

    #[test]
    fn test_nested_child_exceeds_only_child_reported() {
        let runner = PluginTestRunner::new(BlockLinesPlugin::with_max_lines(10));
        // http block contains a server block that exceeds the threshold; only server should be reported
        let mut config = String::from("http {\n");
        config.push_str("    server {\n");
        for i in 1..=10 {
            config.push_str(&format!("        listen {};\n", i));
        }
        config.push_str("    }\n");
        config.push_str("}\n");
        let errors = runner.check_string(&config).expect("parse failed");
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].message.contains("'server' block"),
            "expected server block error, got: {}",
            errors[0].message
        );
    }

    #[test]
    fn test_nested_no_child_exceeds_parent_reported() {
        let runner = PluginTestRunner::new(BlockLinesPlugin::with_max_lines(10));
        // An http block with two short server blocks (3 lines each)
        // Only http should be reported since no child exceeds the threshold
        let mut config = String::from("http {\n");
        config.push_str("    server {\n");
        config.push_str("        listen 80;\n");
        config.push_str("    }\n");
        for i in 1..=8 {
            config.push_str(&format!("    access_log /var/log/nginx/{}.log;\n", i));
        }
        config.push_str("    server {\n");
        config.push_str("        listen 81;\n");
        config.push_str("    }\n");
        config.push_str("}\n");
        let errors = runner.check_string(&config).expect("parse failed");
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].message.contains("'http' block"),
            "expected http block error, got: {}",
            errors[0].message
        );
    }

    #[test]
    fn test_custom_max_lines() {
        let runner = PluginTestRunner::new(BlockLinesPlugin::with_max_lines(10));
        // 12-line block exceeds max_lines=10
        let config = "server {\n    listen 80;\n    listen 81;\n    listen 82;\n    listen 83;\n    listen 84;\n    listen 85;\n    listen 86;\n    listen 87;\n    listen 88;\n    listen 89;\n}\n";
        let errors = runner.check_string(config).expect("parse failed");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("max 10"));
    }

    #[test]
    fn test_custom_max_lines_no_error() {
        let runner = PluginTestRunner::new(BlockLinesPlugin::with_max_lines(200));
        // Default bad.conf (102 lines) should not trigger with max_lines=200
        runner.assert_no_errors(include_str!("../examples/bad.conf"));
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(BlockLinesPlugin::default());

        // bad.conf should produce errors
        let bad_errors = runner
            .check_string(include_str!("../examples/bad.conf"))
            .expect("parse failed");
        assert!(!bad_errors.is_empty(), "bad.conf should produce errors");

        // good.conf should produce no errors
        runner.assert_no_errors(include_str!("../examples/good.conf"));
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(BlockLinesPlugin::default());
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
