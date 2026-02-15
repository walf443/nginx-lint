//! trailing-whitespace plugin
//!
//! This plugin detects trailing whitespace at the end of lines.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check for trailing whitespace
#[derive(Default)]
pub struct TrailingWhitespacePlugin;

impl Plugin for TrailingWhitespacePlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "trailing-whitespace",
            "style",
            "Detects trailing whitespace at the end of lines",
        )
        .with_severity("warning")
        .with_why(
            "Trailing whitespace is invisible and can cause unnecessary diffs \
             in version control and hinder code reviews. \
             Removing trailing whitespace keeps configuration files clean.",
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
        match item {
            ConfigItem::Directive(directive) => {
                // Check trailing whitespace after the directive terminator (; or {)
                if !directive.trailing_whitespace.is_empty() {
                    let error = err
                        .warning(
                            "trailing whitespace at end of line",
                            directive.span.start.line,
                            1,
                        )
                        .with_fix(create_fix_for_directive(directive));
                    errors.push(error);
                }

                // Check trailing whitespace on the closing brace line
                if let Some(block) = &directive.block {
                    if !block.trailing_whitespace.is_empty() {
                        // The closing brace is on a separate line
                        let closing_line = block.span.end.line;
                        let error = err
                            .warning("trailing whitespace at end of line", closing_line, 1)
                            .with_fix(create_fix_for_closing_brace(block));
                        errors.push(error);
                    }

                    // Recursively check nested blocks
                    check_items(&block.items, err, errors);
                }
            }
            ConfigItem::Comment(comment) => {
                if !comment.trailing_whitespace.is_empty() {
                    let error = err
                        .warning(
                            "trailing whitespace at end of line",
                            comment.span.start.line,
                            1,
                        )
                        .with_fix(create_fix_for_comment(comment));
                    errors.push(error);
                }
            }
            ConfigItem::BlankLine(blank) => {
                // BlankLine content is just whitespace - if it's not empty, it has trailing whitespace
                if !blank.content.is_empty() {
                    // Use range-based fix to remove the whitespace content
                    let start = blank.span.start.offset;
                    let end = start + blank.content.len();
                    let error = err
                        .warning(
                            "trailing whitespace at end of line",
                            blank.span.start.line,
                            1,
                        )
                        .with_fix(Fix::replace_range(start, end, ""));
                    errors.push(error);
                }
            }
        }
    }
}

fn create_fix_for_directive(directive: &Directive) -> Fix {
    // Use range-based fix to remove only the trailing whitespace
    // For directives with blocks: trailing whitespace is after the opening '{',
    //   which is at block.span.start.offset
    // For directives without blocks: trailing whitespace is after ';',
    //   which is at span.end.offset
    let start = if let Some(block) = &directive.block {
        // Position after the opening '{'
        block.span.start.offset + 1
    } else {
        // Position after the ';'
        directive.span.end.offset
    };
    let end = start + directive.trailing_whitespace.len();
    Fix::replace_range(start, end, "")
}

fn create_fix_for_closing_brace(block: &Block) -> Fix {
    // Use range-based fix to remove only the trailing whitespace after }
    // span.end.offset is right after the closing brace
    // trailing_whitespace immediately follows
    let start = block.span.end.offset;
    let end = start + block.trailing_whitespace.len();
    Fix::replace_range(start, end, "")
}

fn create_fix_for_comment(comment: &Comment) -> Fix {
    // Use range-based fix to remove only the trailing whitespace after comment
    // span.end.offset is right after the comment text
    // trailing_whitespace immediately follows
    let start = comment.span.end.offset;
    let end = start + comment.trailing_whitespace.len();
    Fix::replace_range(start, end, "")
}

nginx_lint_plugin::export_component_plugin!(TrailingWhitespacePlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::PluginTestRunner;

    #[test]
    fn test_no_trailing_whitespace() {
        let runner = PluginTestRunner::new(TrailingWhitespacePlugin);

        runner.assert_no_errors(
            r#"
server {
    listen 80;
}
"#,
        );
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(TrailingWhitespacePlugin);
        runner.test_examples_with_fix(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(TrailingWhitespacePlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
