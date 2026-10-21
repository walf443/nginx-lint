//! deprecated-ssl-protocol plugin
//!
//! This plugin detects usage of deprecated SSL/TLS protocols (SSLv2, SSLv3, TLSv1, TLSv1.1).
//! These protocols have known vulnerabilities and should not be used.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

const DEPRECATED_PROTOCOLS: &[&str] = &["SSLv2", "SSLv3", "TLSv1", "TLSv1.1"];
const ALLOWED_PROTOCOLS: &[&str] = &["TLSv1.2", "TLSv1.3"];

/// Check for deprecated SSL/TLS protocols
#[derive(Default)]
pub struct DeprecatedSslProtocolPlugin;

impl Plugin for DeprecatedSslProtocolPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "deprecated-ssl-protocol",
            "security",
            "Detects usage of deprecated SSL/TLS protocols (SSLv3, TLSv1, TLSv1.1)",
        )
        .with_severity("warning")
        .with_why(
            "SSLv2, SSLv3, TLSv1.0, and TLSv1.1 have known vulnerabilities and are deprecated. \
             Using these protocols makes your server vulnerable to attacks like POODLE, BEAST, \
             and CRIME. Only TLSv1.2 and above should be used.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_ssl_module.html#ssl_protocols".to_string(),
            "https://wiki.mozilla.org/Security/Server_Side_TLS".to_string(),
            "https://github.com/walf443/nginx-lint/blob/main/plugins/builtin/security/deprecated_ssl_protocol/tests/container_test.rs".to_string(),
        ])
    }

    fn relevant_directives(&self) -> Option<&'static [&'static str]> {
        Some(&["ssl_protocols"])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();

        for directive in config.all_directives() {
            if !directive.is("ssl_protocols") {
                continue;
            }

            // Find deprecated protocols in arguments
            let deprecated_args: Vec<_> = directive
                .args
                .iter()
                .filter(|arg| is_deprecated(arg))
                .collect();

            if deprecated_args.is_empty() {
                continue;
            }

            // Generate the fixed protocol list
            let fixed_protocols = generate_fixed_protocols(&directive.args);

            // Use range-based fix to replace the directive content
            let fix = directive.replace_with(&format!("ssl_protocols {};", fixed_protocols));

            // Report each deprecated protocol but attach fix only to the first one
            for (i, arg) in deprecated_args.iter().enumerate() {
                let protocol = arg.as_str();
                let message = format!(
                    "Deprecated SSL/TLS protocol '{}' should not be used",
                    protocol
                );
                let mut error = err.warning(&message, arg.span.start.line, arg.span.start.column);

                // Attach fix only to the first error to avoid duplicate fixes
                if i == 0 {
                    error = error.with_fix(fix.clone());
                }

                errors.push(error);
            }
        }

        errors
    }
}

/// Whether `arg` names `protocol`. nginx matches the names
/// case-insensitively. A variable names no protocol, even when its name looks
/// like one.
fn names_protocol(arg: &Argument, protocol: &str) -> bool {
    !matches!(arg.value, ArgumentValue::Variable(_)) && arg.as_str().eq_ignore_ascii_case(protocol)
}

fn is_deprecated(arg: &Argument) -> bool {
    DEPRECATED_PROTOCOLS
        .iter()
        .any(|protocol| names_protocol(arg, protocol))
}

/// Generate the fixed protocol list by removing deprecated protocols
///
/// Arguments that stay are written as they appear in the source
/// ([`ArgumentExt::to_source`]), and only compared by their decoded value:
/// writing the decoded value unquoted would let a quoted `;` in it end the
/// directive and turn the rest into directives.
fn generate_fixed_protocols(current: &[Argument]) -> String {
    let order = ["TLSv1.2", "TLSv1.3"];
    // (position in `order`, text to write) of each protocol that stays
    let mut protocols: Vec<(usize, String)> = current
        .iter()
        .filter(|arg| !is_deprecated(arg))
        .map(|arg| {
            let position = order
                .iter()
                .position(|protocol| names_protocol(arg, protocol))
                .unwrap_or(usize::MAX);
            (position, arg.to_source())
        })
        .collect();

    // Add allowed protocols that aren't already present
    for proto in ALLOWED_PROTOCOLS {
        if !current.iter().any(|arg| names_protocol(arg, proto)) {
            let position = order.iter().position(|x| x == proto).unwrap_or(usize::MAX);
            protocols.push((position, proto.to_string()));
        }
    }

    // Sort protocols for consistent output
    protocols.sort_by_key(|(position, _)| *position);

    protocols
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

nginx_lint_plugin::export_component_plugin!(DeprecatedSslProtocolPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::PluginTestRunner;

    #[test]
    fn test_deprecated_sslv3() {
        let runner = PluginTestRunner::new(DeprecatedSslProtocolPlugin);

        runner.assert_has_errors(
            r#"
server {
    ssl_protocols SSLv3 TLSv1.2;
}
"#,
        );
    }

    #[test]
    fn test_deprecated_tlsv1() {
        let runner = PluginTestRunner::new(DeprecatedSslProtocolPlugin);

        runner.assert_has_errors(
            r#"
server {
    ssl_protocols TLSv1;
}
"#,
        );
    }

    #[test]
    fn test_deprecated_tlsv1_1() {
        let runner = PluginTestRunner::new(DeprecatedSslProtocolPlugin);

        runner.assert_has_errors(
            r#"
server {
    ssl_protocols TLSv1.1;
}
"#,
        );
    }

    #[test]
    fn test_multiple_deprecated() {
        let runner = PluginTestRunner::new(DeprecatedSslProtocolPlugin);

        let errors = runner
            .check_string(
                r#"
server {
    ssl_protocols SSLv3 TLSv1 TLSv1.1 TLSv1.2;
}
"#,
            )
            .unwrap();

        // Should have 3 errors (one for each deprecated protocol)
        assert_eq!(
            errors.len(),
            3,
            "Expected 3 errors for SSLv3, TLSv1, TLSv1.1"
        );
    }

    #[test]
    fn test_safe_protocols() {
        let runner = PluginTestRunner::new(DeprecatedSslProtocolPlugin);

        runner.assert_no_errors(
            r#"
server {
    ssl_protocols TLSv1.2 TLSv1.3;
}
"#,
        );
    }

    #[test]
    fn test_only_tlsv1_2() {
        let runner = PluginTestRunner::new(DeprecatedSslProtocolPlugin);

        runner.assert_no_errors(
            r#"
server {
    ssl_protocols TLSv1.2;
}
"#,
        );
    }

    #[test]
    fn test_only_tlsv1_3() {
        let runner = PluginTestRunner::new(DeprecatedSslProtocolPlugin);

        runner.assert_no_errors(
            r#"
server {
    ssl_protocols TLSv1.3;
}
"#,
        );
    }

    #[test]
    fn test_fix_generates_safe_protocols() {
        let runner = PluginTestRunner::new(DeprecatedSslProtocolPlugin);

        let errors = runner
            .check_string(
                r#"
server {
    ssl_protocols SSLv3 TLSv1;
}
"#,
            )
            .unwrap();

        assert!(!errors.is_empty());
        // First error should have a fix
        let fix = errors[0]
            .fixes
            .first()
            .expect("Expected fix on first error");
        assert!(fix.new_text.contains("TLSv1.2"));
        assert!(fix.new_text.contains("TLSv1.3"));
        assert!(!fix.new_text.contains("SSLv3"));
        assert!(!fix.new_text.contains("TLSv1 ") && !fix.new_text.ends_with("TLSv1"));
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(DeprecatedSslProtocolPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_generate_fixed_protocols() {
        let runner = PluginTestRunner::new(DeprecatedSslProtocolPlugin);
        for (before, after) in [
            // All deprecated -> use allowed
            (
                "ssl_protocols SSLv3 TLSv1;",
                "ssl_protocols TLSv1.2 TLSv1.3;",
            ),
            // Mix of deprecated and safe
            (
                "ssl_protocols SSLv3 TLSv1.2;",
                "ssl_protocols TLSv1.2 TLSv1.3;",
            ),
            // A quoted protocol stays quoted, and still counts as present
            (
                r#"ssl_protocols TLSv1 "TLSv1.2";"#,
                r#"ssl_protocols "TLSv1.2" TLSv1.3;"#,
            ),
            // Names match case-insensitively, as in nginx
            (
                "ssl_protocols TLSv1 tlsv1.2;",
                "ssl_protocols tlsv1.2 TLSv1.3;",
            ),
            // A variable is not the protocol its name looks like
            (
                "ssl_protocols $TLSv1 TLSv1;",
                "ssl_protocols TLSv1.2 TLSv1.3 $TLSv1;",
            ),
            // A quoted `;` stays inside the quotes
            (
                r#"ssl_protocols TLSv1 "TLSv1.2;autoindex on;#";"#,
                r#"ssl_protocols TLSv1.2 TLSv1.3 "TLSv1.2;autoindex on;#";"#,
            ),
        ] {
            runner.assert_fix_produces(before, after);
        }
    }

    #[test]
    fn test_fix_keeps_quoted_semicolon() {
        let runner = PluginTestRunner::new(DeprecatedSslProtocolPlugin);
        runner.assert_fix_produces(
            include_str!("../tests/fixtures/002_quoted_semicolon/error/nginx.conf"),
            include_str!("../tests/fixtures/002_quoted_semicolon/expected/nginx.conf"),
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(DeprecatedSslProtocolPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
