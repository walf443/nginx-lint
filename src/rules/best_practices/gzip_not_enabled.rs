use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

/// Check if gzip compression is enabled
pub struct GzipNotEnabled;

impl LintRule for GzipNotEnabled {
    fn name(&self) -> &'static str {
        "gzip-not-enabled"
    }

    fn category(&self) -> &'static str {
        "best-practices"
    }

    fn description(&self) -> &'static str {
        "Suggests enabling gzip compression for better performance"
    }

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        let mut gzip_on = false;

        for directive in config.all_directives() {
            if directive.is("gzip") && directive.first_arg_is("on") {
                gzip_on = true;
                break;
            }
        }

        if !gzip_on {
            vec![LintError::new(
                self.name(),
                self.category(),
                "Consider enabling gzip compression for better performance",
                Severity::Info,
            )]
        } else {
            vec![]
        }
    }
}
