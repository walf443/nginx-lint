use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

/// Check if error_log is configured
pub struct MissingErrorLog;

impl LintRule for MissingErrorLog {
    fn name(&self) -> &'static str {
        "missing-error-log"
    }

    fn description(&self) -> &'static str {
        "Checks if error_log is configured"
    }

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        let mut error_log_found = false;

        for directive in config.all_directives() {
            if directive.is("error_log") {
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
