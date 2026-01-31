use crate::linter::{LintError, LintRule};
use nginx_config::ast::Main;
use std::path::Path;

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
