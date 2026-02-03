//! space-before-semicolon plugin
//!
//! This plugin detects spaces or tabs before semicolons in directives.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;

/// Check for spaces before semicolons
#[derive(Default)]
pub struct SpaceBeforeSemicolonPlugin;

impl Plugin for SpaceBeforeSemicolonPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "space-before-semicolon",
            "style",
            "Detects spaces or tabs before semicolons",
        )
        .with_severity("warning")
        .with_why(
            "Spaces before semicolons violate common coding style conventions \
             and reduce readability. Semicolons should be placed immediately \
             after directive values.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        check_items(&config.items, &mut errors);
        errors
    }
}

fn check_items(items: &[ConfigItem], errors: &mut Vec<LintError>) {
    for item in items {
        if let ConfigItem::Directive(directive) = item {
            // Check if directive has space before terminator (semicolon or brace)
            // Only check for non-block directives (those ending with semicolon)
            if directive.block.is_none() && !directive.space_before_terminator.is_empty() {
                let error = LintError::warning(
                    "space-before-semicolon",
                    "style",
                    "space before semicolon",
                    directive.span.start.line,
                    directive.span.start.column,
                )
                .with_fix(create_fix(directive));
                errors.push(error);
            }

            // Recursively check nested blocks
            if let Some(block) = &directive.block {
                check_items(&block.items, errors);
            }
        }
    }
}

fn create_fix(directive: &Directive) -> Fix {
    // Reconstruct the directive line without the space before semicolon
    let mut line = String::new();
    line.push_str(&directive.leading_whitespace);
    line.push_str(&directive.name);

    for arg in &directive.args {
        line.push(' ');
        line.push_str(&arg.raw);
    }

    line.push(';');

    Fix::replace_line(directive.span.start.line, &line)
}

// Export the plugin
nginx_lint::export_plugin!(SpaceBeforeSemicolonPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::{PluginTestRunner, TestCase};

    #[test]
    fn test_no_space_before_semicolon() {
        let runner = PluginTestRunner::new(SpaceBeforeSemicolonPlugin);

        runner.assert_no_errors(
            r#"
server {
    listen 80;
}
"#,
        );
    }

    #[test]
    fn test_detects_space_before_semicolon() {
        let runner = PluginTestRunner::new(SpaceBeforeSemicolonPlugin);

        runner.assert_has_errors(
            r#"
server {
    listen 80 ;
}
"#,
        );
    }

    #[test]
    fn test_detects_tab_before_semicolon() {
        let runner = PluginTestRunner::new(SpaceBeforeSemicolonPlugin);

        runner.assert_has_errors("listen 80\t;");
    }

    #[test]
    fn test_multiple_spaces() {
        let runner = PluginTestRunner::new(SpaceBeforeSemicolonPlugin);

        runner.assert_has_errors("listen 80   ;");
    }

    #[test]
    fn test_error_location() {
        TestCase::new(
            r#"
server {
    listen 80 ;
}
"#,
        )
        .expect_error_count(1)
        .expect_error_on_line(3)
        .expect_has_fix()
        .run(&SpaceBeforeSemicolonPlugin);
    }

    #[test]
    fn test_fix_removes_space() {
        TestCase::new("    listen 80 ;")
            .expect_error_count(1)
            .expect_fix_produces("    listen 80;")
            .run(&SpaceBeforeSemicolonPlugin);
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(SpaceBeforeSemicolonPlugin);
        runner.test_examples_with_fix(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }
}
