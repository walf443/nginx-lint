use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

/// Check if server_tokens is enabled
pub struct ServerTokensEnabled;

impl LintRule for ServerTokensEnabled {
    fn name(&self) -> &'static str {
        "server-tokens-enabled"
    }

    fn category(&self) -> &'static str {
        "security"
    }

    fn description(&self) -> &'static str {
        "Detects when server_tokens is enabled (exposes nginx version)"
    }

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("server_tokens") && directive.first_arg_is("on") {
                errors.push(
                    LintError::new(
                        self.name(),
                        self.category(),
                        "server_tokens should be 'off' to hide nginx version",
                        Severity::Warning,
                    )
                    .with_location(directive.span.start.line, directive.span.start.column),
                );
            }
        }

        errors
    }
}
