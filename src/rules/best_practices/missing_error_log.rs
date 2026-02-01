use crate::docs::RuleDoc;
use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "missing-error-log",
    category: "best_practices",
    description: "Suggests configuring error_log",
    severity: "info",
    why: r#"Configuring error_log allows you to record errors and issues
in log files for troubleshooting purposes.

Setting an appropriate log level helps capture necessary information
while managing disk usage."#,
    bad_example: include_str!("missing_error_log/bad.conf"),
    good_example: include_str!("missing_error_log/good.conf"),
    references: &[
        "https://nginx.org/en/docs/ngx_core_module.html#error_log",
    ],
};

/// Check if error_log is configured
pub struct MissingErrorLog;

impl LintRule for MissingErrorLog {
    fn name(&self) -> &'static str {
        "missing-error-log"
    }

    fn category(&self) -> &'static str {
        "best-practices"
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
                self.category(),
                "Consider configuring error_log for debugging",
                Severity::Info,
            )]
        } else {
            vec![]
        }
    }
}
