use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

/// Check for deprecated SSL/TLS protocols
pub struct DeprecatedSslProtocol;

const DEPRECATED_PROTOCOLS: &[&str] = &["SSLv2", "SSLv3", "TLSv1", "TLSv1.1"];

impl LintRule for DeprecatedSslProtocol {
    fn name(&self) -> &'static str {
        "deprecated-ssl-protocol"
    }

    fn description(&self) -> &'static str {
        "Detects usage of deprecated SSL/TLS protocols (SSLv3, TLSv1, TLSv1.1)"
    }

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("ssl_protocols") {
                for arg in &directive.args {
                    let protocol = arg.as_str();
                    if DEPRECATED_PROTOCOLS.contains(&protocol) {
                        errors.push(
                            LintError::new(
                                self.name(),
                                &format!(
                                    "Deprecated SSL/TLS protocol '{}' should not be used",
                                    protocol
                                ),
                                Severity::Warning,
                            )
                            .with_location(arg.span.start.line, arg.span.start.column),
                        );
                    }
                }
            }
        }

        errors
    }
}
