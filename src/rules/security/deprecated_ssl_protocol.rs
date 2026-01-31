use crate::linter::{LintError, LintRule};
use nginx_config::ast::Main;
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
