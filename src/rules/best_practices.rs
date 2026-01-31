use crate::linter::{LintError, LintRule, Severity};
use nginx_config::ast::{Item, Main};
use std::path::Path;

/// Check if gzip compression is enabled
pub struct GzipNotEnabled;

impl LintRule for GzipNotEnabled {
    fn name(&self) -> &'static str {
        "gzip-not-enabled"
    }

    fn description(&self) -> &'static str {
        "Suggests enabling gzip compression for better performance"
    }

    fn check(&self, config: &Main, _path: &Path) -> Vec<LintError> {
        let mut gzip_on = false;

        for directive in config.all_directives() {
            if let Item::Gzip(enabled) = directive.item {
                if enabled {
                    gzip_on = true;
                    break;
                }
            }
        }

        if !gzip_on {
            vec![LintError::new(
                self.name(),
                "Consider enabling gzip compression for better performance",
                Severity::Info,
            )]
        } else {
            vec![]
        }
    }
}

/// Check if error_log is configured
pub struct MissingErrorLog;

impl LintRule for MissingErrorLog {
    fn name(&self) -> &'static str {
        "missing-error-log"
    }

    fn description(&self) -> &'static str {
        "Checks if error_log is configured"
    }

    fn check(&self, config: &Main, _path: &Path) -> Vec<LintError> {
        let mut error_log_found = false;

        for directive in config.all_directives() {
            if let Item::ErrorLog { .. } = directive.item {
                error_log_found = true;
                break;
            }
        }

        if !error_log_found {
            vec![LintError::new(
                self.name(),
                "Consider configuring error_log for debugging",
                Severity::Info,
            )]
        } else {
            vec![]
        }
    }
}
