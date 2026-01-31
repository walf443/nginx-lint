use crate::linter::{LintError, LintRule, Severity};
use nginx_config::ast::{Item, Main, Value};
use std::path::Path;

/// Check if server_tokens is enabled
pub struct ServerTokensEnabled;

impl LintRule for ServerTokensEnabled {
    fn name(&self) -> &'static str {
        "server-tokens-enabled"
    }

    fn description(&self) -> &'static str {
        "Detects when server_tokens is enabled (exposes nginx version)"
    }

    fn check(&self, config: &Main, _path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if let Item::ServerTokens(ref value) = directive.item {
                if is_value_on(value) {
                    errors.push(
                        LintError::new(
                            self.name(),
                            "server_tokens should be 'off' to hide nginx version",
                            Severity::Warning,
                        )
                        .with_location(directive.position.line, directive.position.column),
                    );
                }
            }
        }

        errors
    }
}

fn is_value_on(value: &Value) -> bool {
    // Use Debug representation to check value
    let debug_str = format!("{:?}", value);
    debug_str.contains("\"on\"")
}
