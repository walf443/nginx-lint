use crate::docs::RuleDoc;
use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "gzip-not-enabled",
    category: "best_practices",
    description: "Suggests enabling gzip compression",
    severity: "info",
    why: r#"Enabling gzip compression significantly reduces response sizes,
improves page load times, and saves bandwidth.

It is especially effective for text-based content like HTML, CSS,
JavaScript, and JSON."#,
    bad_example: r#"http {
    # gzip not configured
    server {
        listen 80;
    }
}"#,
    good_example: r#"http {
    gzip on;
    gzip_types text/plain text/css application/json application/javascript;
    gzip_min_length 1000;

    server {
        listen 80;
    }
}"#,
    references: &[
        "https://nginx.org/en/docs/http/ngx_http_gzip_module.html",
    ],
};

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
