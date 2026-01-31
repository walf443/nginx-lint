use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

/// Check if autoindex is enabled (can expose directory contents)
pub struct AutoindexEnabled;

impl LintRule for AutoindexEnabled {
    fn name(&self) -> &'static str {
        "autoindex-enabled"
    }

    fn description(&self) -> &'static str {
        "Detects when autoindex is enabled (can expose directory contents)"
    }

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("autoindex") && directive.first_arg_is("on") {
                errors.push(
                    LintError::new(
                        self.name(),
                        "autoindex is enabled, which can expose directory contents",
                        Severity::Warning,
                    )
                    .with_location(directive.span.start.line, directive.span.start.column),
                );
            }
        }

        errors
    }
}
