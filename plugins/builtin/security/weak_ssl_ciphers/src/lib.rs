//! weak-ssl-ciphers plugin
//!
//! This plugin detects weak SSL/TLS cipher suites and missing cipher exclusions.
//! Weak ciphers include NULL, EXPORT, DES, RC4, MD5, and others.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Default weak cipher patterns
const DEFAULT_WEAK_CIPHERS: &[&str] = &[
    "NULL",     // No encryption
    "EXPORT",   // Export-grade (weak)
    "DES",      // Weak block cipher (includes 3DES)
    "RC4",      // Vulnerable stream cipher
    "MD5",      // Weak hash algorithm
    "aNULL",    // Anonymous (no authentication)
    "eNULL",    // No encryption
    "ADH",      // Anonymous Diffie-Hellman
    "AECDH",    // Anonymous ECDH
    "PSK",      // Pre-Shared Key (often misconfigured)
    "SRP",      // Secure Remote Password (rarely needed)
    "CAMELLIA", // Less common, potential compatibility issues
];

/// Default required exclusions
const DEFAULT_REQUIRED_EXCLUSIONS: &[&str] =
    &["!aNULL", "!eNULL", "!EXPORT", "!DES", "!RC4", "!MD5"];

/// Check for weak SSL/TLS cipher suites
#[derive(Default)]
pub struct WeakSslCiphersPlugin;

impl Plugin for WeakSslCiphersPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "weak-ssl-ciphers",
            "security",
            "Detects weak or insecure SSL/TLS cipher suites",
        )
        .with_severity("warning")
        .with_why(
            "Weak cipher suites (NULL, EXPORT, DES, RC4, MD5, etc.) have insufficient \
             cryptographic strength or known vulnerabilities. Using only strong cipher suites \
             and explicitly excluding weak ones ensures secure communication.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_ssl_module.html#ssl_ciphers".to_string(),
            "https://wiki.mozilla.org/Security/Server_Side_TLS".to_string(),
            "https://ssl-config.mozilla.org/".to_string(),
            "https://github.com/walf443/nginx-lint/blob/main/plugins/builtin/security/weak_ssl_ciphers/tests/container_test.rs".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();

        for directive in config.all_directives() {
            if !directive.is("ssl_ciphers") {
                continue;
            }

            let Some(cipher_arg) = directive.args.first() else {
                continue;
            };

            let cipher_string = cipher_arg.as_str();

            // Check for weak cipher patterns
            let weak_ciphers = find_weak_ciphers(cipher_string);
            for weak in &weak_ciphers {
                let message = format!("Weak cipher suite '{}' should not be used", weak);
                errors.push(err.warning(
                    &message,
                    cipher_arg.span.start.line,
                    cipher_arg.span.start.column,
                ));
            }

            // Check for missing exclusions
            let missing_exclusions = find_missing_exclusions(cipher_string);
            if !missing_exclusions.is_empty() {
                // Generate fix with required exclusions added
                let fixed_cipher_string =
                    generate_fixed_cipher_string(cipher_string, &missing_exclusions);

                // Determine if original uses quotes
                let quote_char = if cipher_arg.raw.starts_with('\'') {
                    "'"
                } else if cipher_arg.raw.starts_with('"') {
                    "\""
                } else {
                    ""
                };

                // Use range-based fix to replace the directive content
                let fix = directive.replace_with(&format!(
                    "ssl_ciphers {}{}{};",
                    quote_char, fixed_cipher_string, quote_char
                ));

                let message = format!(
                    "Missing cipher exclusions: {}",
                    missing_exclusions.join(", ")
                );
                errors.push(
                    err.warning(
                        &message,
                        cipher_arg.span.start.line,
                        cipher_arg.span.start.column,
                    )
                    .with_fix(fix),
                );
            }
        }

        errors
    }
}

/// Find weak cipher patterns in the cipher string
fn find_weak_ciphers(cipher_string: &str) -> Vec<String> {
    let mut found = Vec::new();

    // Split by colon to get individual cipher specs
    for cipher_spec in cipher_string.split(':') {
        let spec = cipher_spec.trim();

        // Skip exclusions (start with !)
        if spec.starts_with('!') {
            continue;
        }

        // Skip empty specs
        if spec.is_empty() {
            continue;
        }

        // Check if this spec contains any weak pattern
        for weak_pattern in DEFAULT_WEAK_CIPHERS {
            // Match the pattern in the cipher spec
            if spec.eq_ignore_ascii_case(weak_pattern)
                || spec.to_uppercase().contains(&weak_pattern.to_uppercase())
            {
                // Don't report if it's already being excluded elsewhere
                let exclusion = format!("!{}", weak_pattern);
                if !cipher_string.contains(&exclusion) {
                    if !found.contains(&spec.to_string()) {
                        found.push(spec.to_string());
                    }
                    break;
                }
            }
        }
    }

    found
}

/// Find required exclusions that are missing from the cipher string
fn find_missing_exclusions(cipher_string: &str) -> Vec<String> {
    let upper_cipher = cipher_string.to_uppercase();

    DEFAULT_REQUIRED_EXCLUSIONS
        .iter()
        .filter(|exclusion| !upper_cipher.contains(&exclusion.to_uppercase()))
        .map(|s| s.to_string())
        .collect()
}

/// Generate a fixed cipher string with missing exclusions added
fn generate_fixed_cipher_string(original: &str, missing: &[String]) -> String {
    if missing.is_empty() {
        return original.to_string();
    }

    // Add missing exclusions at the end
    let additions = missing.join(":");
    if original.is_empty() {
        additions
    } else {
        format!("{}:{}", original, additions)
    }
}

// Export the plugin
nginx_lint_plugin::export_plugin!(WeakSslCiphersPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::PluginTestRunner;

    #[test]
    fn test_weak_cipher_rc4() {
        let runner = PluginTestRunner::new(WeakSslCiphersPlugin);

        runner.assert_has_errors(
            r#"
server {
    ssl_ciphers RC4:AES256;
}
"#,
        );
    }

    #[test]
    fn test_weak_cipher_des() {
        let runner = PluginTestRunner::new(WeakSslCiphersPlugin);

        runner.assert_has_errors(
            r#"
server {
    ssl_ciphers DES-CBC3-SHA:AES128-SHA;
}
"#,
        );
    }

    #[test]
    fn test_missing_exclusions() {
        let runner = PluginTestRunner::new(WeakSslCiphersPlugin);

        runner.assert_has_errors(
            r#"
server {
    ssl_ciphers HIGH;
}
"#,
        );
    }

    #[test]
    fn test_safe_ciphers_with_exclusions() {
        let runner = PluginTestRunner::new(WeakSslCiphersPlugin);

        runner.assert_no_errors(
            r#"
server {
    ssl_ciphers ECDHE-RSA-AES128-GCM-SHA256:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5;
}
"#,
        );
    }

    #[test]
    fn test_excluded_weak_cipher_not_reported() {
        // If we exclude RC4 explicitly, it shouldn't be reported as weak
        let runner = PluginTestRunner::new(WeakSslCiphersPlugin);

        let errors = runner
            .check_string(
                r#"
server {
    ssl_ciphers HIGH:!RC4:!aNULL:!eNULL:!EXPORT:!DES:!MD5;
}
"#,
            )
            .unwrap();

        // Should have no weak cipher errors (only check for missing exclusions)
        let weak_cipher_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("Weak cipher suite"))
            .collect();
        assert!(
            weak_cipher_errors.is_empty(),
            "Excluded ciphers should not be reported: {:?}",
            weak_cipher_errors
        );
    }

    #[test]
    fn test_fix_adds_missing_exclusions() {
        let runner = PluginTestRunner::new(WeakSslCiphersPlugin);

        let errors = runner
            .check_string(
                r#"
server {
    ssl_ciphers HIGH;
}
"#,
            )
            .unwrap();

        // Find the error with fix
        let error_with_fix = errors.iter().find(|e| !e.fixes.is_empty());
        assert!(error_with_fix.is_some(), "Expected an error with fix");

        let fix = &error_with_fix.unwrap().fixes[0];
        assert!(fix.new_text.contains("!aNULL"));
        assert!(fix.new_text.contains("!eNULL"));
        assert!(fix.new_text.contains("!EXPORT"));
        assert!(fix.new_text.contains("!DES"));
        assert!(fix.new_text.contains("!RC4"));
        assert!(fix.new_text.contains("!MD5"));
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(WeakSslCiphersPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_find_weak_ciphers() {
        // Direct weak cipher
        let weak = find_weak_ciphers("RC4:AES256");
        assert!(weak.contains(&"RC4".to_string()));

        // No weak ciphers
        let weak = find_weak_ciphers("ECDHE-RSA-AES128-GCM-SHA256");
        assert!(weak.is_empty());

        // Excluded weak cipher should not be reported
        let weak = find_weak_ciphers("HIGH:!RC4");
        assert!(!weak.iter().any(|c| c == "HIGH"));
    }

    #[test]
    fn test_find_missing_exclusions() {
        // All exclusions missing
        let missing = find_missing_exclusions("HIGH");
        assert!(!missing.is_empty());

        // Some exclusions present
        let missing = find_missing_exclusions("HIGH:!aNULL:!MD5");
        assert!(!missing.contains(&"!aNULL".to_string()));
        assert!(!missing.contains(&"!MD5".to_string()));
        assert!(missing.contains(&"!RC4".to_string()));

        // All exclusions present
        let missing = find_missing_exclusions("HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5");
        assert!(missing.is_empty());
    }

    #[test]
    fn test_generate_fixed_cipher_string() {
        let fixed = generate_fixed_cipher_string("HIGH", &["!RC4".to_string(), "!MD5".to_string()]);
        assert_eq!(fixed, "HIGH:!RC4:!MD5");

        let fixed = generate_fixed_cipher_string("", &["!RC4".to_string()]);
        assert_eq!(fixed, "!RC4");
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(WeakSslCiphersPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
