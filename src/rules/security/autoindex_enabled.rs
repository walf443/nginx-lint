use crate::docs::RuleDoc;
use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "autoindex-enabled",
    category: "security",
    description: "Detects when autoindex is enabled",
    severity: "warning",
    why: r#"When autoindex is enabled, directory contents are listed publicly.
This can expose unintended files and directory structures, potentially
leading to information disclosure and security risks.

Autoindex should be disabled unless explicitly required."#,
    bad_example: include_str!("autoindex_enabled/bad.conf"),
    good_example: include_str!("autoindex_enabled/good.conf"),
    references: &[
        "https://nginx.org/en/docs/http/ngx_http_autoindex_module.html",
    ],
};

/// Check if autoindex is enabled (can expose directory contents)
pub struct AutoindexEnabled;

impl LintRule for AutoindexEnabled {
    fn name(&self) -> &'static str {
        "autoindex-enabled"
    }

    fn category(&self) -> &'static str {
        "security"
    }

    fn description(&self) -> &'static str {
        "Detects when autoindex is enabled (can expose directory contents)"
    }

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("autoindex") && directive.first_arg_is("on") {
                let fix = Fix::replace(
                    directive.span.start.line,
                    "autoindex on",
                    "autoindex off",
                );
                errors.push(
                    LintError::new(
                        self.name(),
                        self.category(),
                        "autoindex is enabled, which can expose directory contents",
                        Severity::Warning,
                    )
                    .with_location(directive.span.start.line, directive.span.start.column)
                    .with_fix(fix),
                );
            }
        }

        errors
    }
}
