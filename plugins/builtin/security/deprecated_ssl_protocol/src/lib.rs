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
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
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
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.info().error_builder();

        for directive in config.all_directives() {
            if !directive.is("ssl_protocols") {
                continue;
            }

            // Find deprecated protocols in arguments
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

/// Generate the fixed protocol list by removing deprecated protocols
fn generate_fixed_protocols(current: &[&str]) -> String {
    // Filter out deprecated protocols from current
    let safe_current: Vec<&str> = current
        .iter()
        .filter(|p| !DEPRECATED_PROTOCOLS.contains(p))
        .copied()
        .collect();

    // Start with safe current protocols
    let mut protocols: Vec<String> = safe_current.iter().map(|s| s.to_string()).collect();

    // Add allowed protocols that aren't already present
    for proto in ALLOWED_PROTOCOLS {
        if !protocols.iter().any(|p| p == *proto) {
            protocols.push(proto.to_string());
        }
    }

    // If still empty, use all allowed protocols
    if protocols.is_empty() {
        return ALLOWED_PROTOCOLS.join(" ");
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

// Export the plugin
nginx_lint_plugin::export_plugin!(DeprecatedSslProtocolPlugin);

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

        let errors = runner.check_string(
            r#"
server {
    ssl_protocols SSLv3 TLSv1 TLSv1.1 TLSv1.2;
}
"#,
        ).unwrap();

        // Should have 3 errors (one for each deprecated protocol)
        assert_eq!(errors.len(), 3, "Expected 3 errors for SSLv3, TLSv1, TLSv1.1");
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

        let errors = runner.check_string(
            r#"
server {
    ssl_protocols SSLv3 TLSv1;
}
"#,
        ).unwrap();

        assert!(!errors.is_empty());
        // First error should have a fix
        let fix = errors[0].fix.as_ref().expect("Expected fix on first error");
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
        // All deprecated -> use allowed
        assert_eq!(
            generate_fixed_protocols(&["SSLv3", "TLSv1"]),
            "TLSv1.2 TLSv1.3"
        );

        // Mix of deprecated and safe
        assert_eq!(
            generate_fixed_protocols(&["SSLv3", "TLSv1.2"]),
            "TLSv1.2 TLSv1.3"
        );

        // Only TLSv1.2 -> add TLSv1.3
        assert_eq!(
            generate_fixed_protocols(&["TLSv1.2"]),
            "TLSv1.2 TLSv1.3"
        );
    }
}
