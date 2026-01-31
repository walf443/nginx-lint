use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

/// Check for deprecated SSL/TLS protocols
pub struct DeprecatedSslProtocol;

const DEPRECATED_PROTOCOLS: &[&str] = &["SSLv2", "SSLv3", "TLSv1", "TLSv1.1"];
const RECOMMENDED_PROTOCOLS: &str = "TLSv1.2 TLSv1.3";

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
                let fixed_protocols = generate_fixed_protocols(&current_protocols);

                // Calculate indentation from the directive's position
                let indent = " ".repeat(directive.span.start.column.saturating_sub(1));
                let fixed_line = format!("{}ssl_protocols {};", indent, fixed_protocols);
                let fix = Fix::replace_line(directive.span.start.line, &fixed_line);

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
/// and ensuring recommended protocols are present
fn generate_fixed_protocols(current: &[&str]) -> String {
    // Filter out deprecated protocols
    let mut protocols: Vec<&str> = current
        .iter()
        .filter(|p| !DEPRECATED_PROTOCOLS.contains(p))
        .copied()
        .collect();

    // If no safe protocols remain, use the recommended ones
    if protocols.is_empty() {
        return RECOMMENDED_PROTOCOLS.to_string();
    }

    // Ensure we have at least TLSv1.2 and TLSv1.3
    if !protocols.contains(&"TLSv1.2") {
        protocols.push("TLSv1.2");
    }
    if !protocols.contains(&"TLSv1.3") {
        protocols.push("TLSv1.3");
    }

    // Sort protocols for consistent output
    protocols.sort_by(|a, b| {
        let order = ["TLSv1.2", "TLSv1.3"];
        let a_idx = order.iter().position(|&x| x == *a).unwrap_or(99);
        let b_idx = order.iter().position(|&x| x == *b).unwrap_or(99);
        a_idx.cmp(&b_idx)
    });

    protocols.join(" ")
}
