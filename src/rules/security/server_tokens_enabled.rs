use crate::docs::RuleDoc;
use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "server-tokens-enabled",
    category: "security",
    description: "Detects when server_tokens is enabled",
    severity: "warning",
    why: r#"When server_tokens is set to 'on', nginx exposes version information
in response headers and error pages. Attackers can use this information
to target known vulnerabilities for that specific version.

Hiding version information raises the difficulty of targeted attacks."#,
    bad_example: include_str!("server_tokens_enabled/bad.conf"),
    good_example: include_str!("server_tokens_enabled/good.conf"),
    references: &[
        "https://nginx.org/en/docs/http/ngx_http_core_module.html#server_tokens",
    ],
};

/// Check if server_tokens is enabled
pub struct ServerTokensEnabled;

impl LintRule for ServerTokensEnabled {
    fn name(&self) -> &'static str {
        "server-tokens-enabled"
    }

    fn category(&self) -> &'static str {
        "security"
    }

    fn description(&self) -> &'static str {
        "Detects when server_tokens is enabled (exposes nginx version)"
    }

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("server_tokens") && directive.first_arg_is("on") {
                let fix = Fix::replace(
                    directive.span.start.line,
                    "server_tokens on",
                    "server_tokens off",
                );
                errors.push(
                    LintError::new(
                        self.name(),
                        self.category(),
                        "server_tokens should be 'off' to hide nginx version",
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
