//! space-before-semicolon plugin
//!
//! This plugin detects spaces or tabs before semicolons in directives.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check for spaces before semicolons
#[derive(Default)]
pub struct SpaceBeforeSemicolonPlugin;

impl Plugin for SpaceBeforeSemicolonPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
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
        let err = self.spec().error_builder();
        check_items(&config.items, &err, &mut errors);
        errors
    }
}

fn check_items(items: &[ConfigItem], err: &ErrorBuilder, errors: &mut Vec<LintError>) {
    for item in items {
        if let ConfigItem::Directive(directive) = item {
            // Check if directive has space before terminator (semicolon or brace)
            // Only check for non-block directives (those ending with semicolon)
            if directive.block.is_none() && !directive.space_before_terminator.is_empty() {
                let error = err
                    .warning_at("space before semicolon", directive)
                    .with_fix(create_fix(directive));
                errors.push(error);
            }

            // Recursively check nested blocks
            if let Some(block) = &directive.block {
                check_items(&block.items, err, errors);
            }
        }
    }
}

fn create_fix(directive: &Directive) -> Fix {
    // Use range-based fix to remove only the space before semicolon
    // span.end.offset is right after the ';'
    // The space is before the ';', so:
    //   semicolon is at span.end.offset - 1
    //   space starts at span.end.offset - 1 - space_before_terminator.len()
    let semicolon_offset = directive.span.end.offset - 1;
    let start = semicolon_offset - directive.space_before_terminator.len();
    let end = semicolon_offset;
    Fix::replace_range(start, end, "")
}

// Export the plugin
nginx_lint_plugin::export_plugin!(SpaceBeforeSemicolonPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::{PluginTestRunner, TestCase};

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

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(SpaceBeforeSemicolonPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}