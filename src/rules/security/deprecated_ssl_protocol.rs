use crate::docs::RuleDoc;
use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "deprecated-ssl-protocol",
    category: "security",
    description: "Detects deprecated SSL/TLS protocols",
    severity: "warning",
    why: r#"SSLv2, SSLv3, TLSv1.0, and TLSv1.1 have known vulnerabilities and
are deprecated. Using these protocols makes your server vulnerable to
attacks like POODLE, BEAST, and CRIME.

Only TLSv1.2 and above should be used."#,
    bad_example: include_str!("deprecated_ssl_protocol/bad.conf"),
    good_example: include_str!("deprecated_ssl_protocol/good.conf"),
    references: &[
        "https://nginx.org/en/docs/http/ngx_http_ssl_module.html#ssl_protocols",
        "https://wiki.mozilla.org/Security/Server_Side_TLS",
    ],
};

/// Check for deprecated SSL/TLS protocols
pub struct DeprecatedSslProtocol {
    /// Allowed protocols to use in fix (default: ["TLSv1.2", "TLSv1.3"])
    pub allowed_protocols: Vec<String>,
}

const DEPRECATED_PROTOCOLS: &[&str] = &["SSLv2", "SSLv3", "TLSv1", "TLSv1.1"];

impl Default for DeprecatedSslProtocol {
    fn default() -> Self {
        Self {
            allowed_protocols: vec!["TLSv1.2".to_string(), "TLSv1.3".to_string()],
        }
    }
}

impl LintRule for DeprecatedSslProtocol {
    fn name(&self) -> &'static str {
        "deprecated-ssl-protocol"
    }

    fn category(&self) -> &'static str {
        "security"
    }

    fn description(&self) -> &'static str {
        "Detects usage of deprecated SSL/TLS protocols (SSLv3, TLSv1, TLSv1.1)"
    }

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("ssl_protocols") {
                let deprecated_args: Vec<_> = directive
                    .args
                    .iter()
                    .filter(|arg| DEPRECATED_PROTOCOLS.contains(&arg.as_str()))
                    .collect();

                if deprecated_args.is_empty() {
                    continue;
                }

                // Generate the fixed protocol list
                let current_protocols: Vec<&str> =
                    directive.args.iter().map(|a| a.as_str()).collect();
                let fixed_protocols =
                    generate_fixed_protocols(&current_protocols, &self.allowed_protocols);

                // Use range-based fix to replace the directive content
                // Include leading whitespace for proper indentation
                let fixed_content = format!(
                    "{}ssl_protocols {};",
                    directive.leading_whitespace,
                    fixed_protocols
                );
                let start = directive.span.start.offset - directive.leading_whitespace.len();
                let end = directive.span.end.offset;
                let fix = Fix::replace_range(start, end, &fixed_content);

                // Report each deprecated protocol but attach fix only to the first one
                for (i, arg) in deprecated_args.iter().enumerate() {
                    let protocol = arg.as_str();
                    let message = format!(
                        "Deprecated SSL/TLS protocol '{}' should not be used",
                        protocol
                    );
                    let mut error =
                        LintError::new(self.name(), self.category(), &message, Severity::Warning)
                            .with_location(arg.span.start.line, arg.span.start.column);

                    // Attach fix only to the first error to avoid duplicate fixes
                    if i == 0 {
                        error = error.with_fix(fix.clone());
                    }

                    errors.push(error);
                }
            }
        }

        errors
    }
}

/// Generate the fixed protocol list by removing deprecated protocols
/// and using the allowed protocols
fn generate_fixed_protocols(current: &[&str], allowed: &[String]) -> String {
    // Filter out deprecated protocols from current
    let safe_current: Vec<&str> = current
        .iter()
        .filter(|p| !DEPRECATED_PROTOCOLS.contains(p))
        .copied()
        .collect();

    // Start with safe current protocols
    let mut protocols: Vec<String> = safe_current.iter().map(|s| s.to_string()).collect();

    // Add allowed protocols that aren't already present
    for proto in allowed {
        if !protocols.contains(proto) {
            protocols.push(proto.clone());
        }
    }

    // If still empty, use all allowed protocols
    if protocols.is_empty() {
        return allowed.join(" ");
    }

    // Sort protocols for consistent output
    protocols.sort_by(|a, b| {
        let order = ["TLSv1.2", "TLSv1.3"];
        let a_idx = order.iter().position(|x| x == a).unwrap_or(99);
        let b_idx = order.iter().position(|x| x == b).unwrap_or(99);
        a_idx.cmp(&b_idx)
    });

    protocols.join(" ")
}
