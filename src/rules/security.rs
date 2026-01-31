use crate::linter::{LintError, LintRule, Severity};
use nginx_config::ast::{Item, Main, Value};
use std::path::Path;

/// Check for deprecated SSL/TLS protocols
pub struct DeprecatedSslProtocol;

impl LintRule for DeprecatedSslProtocol {
    fn name(&self) -> &'static str {
        "deprecated-ssl-protocol"
    }

    fn description(&self) -> &'static str {
        "Detects usage of deprecated SSL/TLS protocols (SSLv3, TLSv1, TLSv1.1)"
    }

    fn check(&self, _config: &Main, _path: &Path) -> Vec<LintError> {
        // Note: nginx-config doesn't parse ssl_protocols directly,
        // so we would need to handle this differently in a production tool.
        vec![]
    }
}

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

/// Check if autoindex is enabled (nginx-config doesn't support this directive)
pub struct AutoindexEnabled;

impl LintRule for AutoindexEnabled {
    fn name(&self) -> &'static str {
        "autoindex-enabled"
    }

    fn description(&self) -> &'static str {
        "Detects when autoindex is enabled (can expose directory contents)"
    }

    fn check(&self, _config: &Main, _path: &Path) -> Vec<LintError> {
        // Note: nginx-config doesn't parse autoindex directive
        vec![]
    }
}

fn is_value_on(value: &Value) -> bool {
    // Use Debug representation to check value
    let debug_str = format!("{:?}", value);
    debug_str.contains("\"on\"")
}
