//! trailing-whitespace plugin
//!
//! This plugin detects trailing whitespace at the end of lines.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;

/// Check for trailing whitespace
#[derive(Default)]
pub struct TrailingWhitespacePlugin;

impl Plugin for TrailingWhitespacePlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
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
        check_items(&config.items, &mut errors);
        errors
    }
}

fn check_items(items: &[ConfigItem], errors: &mut Vec<LintError>) {
    for item in items {
        match item {
            ConfigItem::Directive(directive) => {
                // Check trailing whitespace after the directive terminator (; or {)
                if !directive.trailing_whitespace.is_empty() {
                    let error = LintError::warning(
                        "trailing-whitespace",
                        "style",
                        "trailing whitespace at end of line",
                        directive.span.start.line,
                        1, // Column doesn't matter much for trailing whitespace
                    )
                    .with_fix(create_fix_for_directive(directive));
                    errors.push(error);
                }

                // Check trailing whitespace on the closing brace line
                if let Some(block) = &directive.block {
                    if !block.trailing_whitespace.is_empty() {
                        // The closing brace is on a separate line
                        let closing_line = block.span.end.line;
                        let error = LintError::warning(
                            "trailing-whitespace",
                            "style",
                            "trailing whitespace at end of line",
                            closing_line,
                            1,
                        )
                        .with_fix(create_fix_for_closing_brace(block));
                        errors.push(error);
                    }

                    // Recursively check nested blocks
                    check_items(&block.items, errors);
                }
            }
            ConfigItem::Comment(comment) => {
                if !comment.trailing_whitespace.is_empty() {
                    let error = LintError::warning(
                        "trailing-whitespace",
                        "style",
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
                    let error = LintError::warning(
                        "trailing-whitespace",
                        "style",
                        "trailing whitespace at end of line",
                        blank.span.start.line,
                        1,
                    )
                    .with_fix(Fix::replace_line(blank.span.start.line, ""));
                    errors.push(error);
                }
            }
        }
    }
}

fn create_fix_for_directive(directive: &Directive) -> Fix {
    // Reconstruct the directive line without trailing whitespace
    let mut line = String::new();
    line.push_str(&directive.leading_whitespace);
    line.push_str(&directive.name);

    for arg in &directive.args {
        line.push(' ');
        line.push_str(&arg.raw);
    }

    line.push_str(&directive.space_before_terminator);

    if directive.block.is_some() {
        line.push('{');
    } else {
        line.push(';');
    }

    Fix::replace_line(directive.span.start.line, &line)
}

fn create_fix_for_closing_brace(block: &Block) -> Fix {
    // Reconstruct the closing brace line without trailing whitespace
    let mut line = String::new();
    line.push_str(&block.closing_brace_leading_whitespace);
    line.push('}');

    Fix::replace_line(block.span.end.line, &line)
}

fn create_fix_for_comment(comment: &Comment) -> Fix {
    // Reconstruct the comment line without trailing whitespace
    let mut line = String::new();
    line.push_str(&comment.leading_whitespace);
    line.push_str(&comment.text);

    Fix::replace_line(comment.span.start.line, &line)
}

// Export the plugin
nginx_lint::export_plugin!(TrailingWhitespacePlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::{PluginTestRunner, TestCase};

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
}
