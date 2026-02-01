use crate::docs::RuleDoc;
use crate::linter::{Fix, LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "weak-ssl-ciphers",
    category: "security",
    description: "Detects weak SSL/TLS cipher suites",
    severity: "warning",
    why: r#"Weak cipher suites (NULL, EXPORT, DES, RC4, MD5, etc.) have
insufficient cryptographic strength or known vulnerabilities.

Using only strong cipher suites and explicitly excluding weak ones
ensures secure communication."#,
    bad_example: include_str!("weak_ssl_ciphers/bad.conf"),
    good_example: include_str!("weak_ssl_ciphers/good.conf"),
    references: &[
        "https://nginx.org/en/docs/http/ngx_http_ssl_module.html#ssl_ciphers",
        "https://wiki.mozilla.org/Security/Server_Side_TLS",
        "https://ssl-config.mozilla.org/",
    ],
};

/// Check for weak SSL/TLS cipher suites
pub struct WeakSslCiphers {
    /// Cipher patterns that are considered weak
    pub weak_ciphers: Vec<String>,
    /// Required exclusion patterns (e.g., !aNULL, !MD5)
    pub required_exclusions: Vec<String>,
}

/// Default weak cipher patterns
const DEFAULT_WEAK_CIPHERS: &[&str] = &[
    "NULL",   // No encryption
    "EXPORT", // Export-grade (weak)
    "DES",    // Weak block cipher (includes 3DES)
    "RC4",    // Vulnerable stream cipher
    "MD5",    // Weak hash algorithm
    "aNULL",  // Anonymous (no authentication)
    "eNULL",  // No encryption
    "ADH",    // Anonymous Diffie-Hellman
    "AECDH",  // Anonymous ECDH
    "PSK",    // Pre-Shared Key (often misconfigured)
    "SRP",    // Secure Remote Password (rarely needed)
    "CAMELLIA", // Less common, potential compatibility issues
];

/// Default required exclusions
const DEFAULT_REQUIRED_EXCLUSIONS: &[&str] = &["!aNULL", "!eNULL", "!EXPORT", "!DES", "!RC4", "!MD5"];

impl Default for WeakSslCiphers {
    fn default() -> Self {
        Self {
            weak_ciphers: DEFAULT_WEAK_CIPHERS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            required_exclusions: DEFAULT_REQUIRED_EXCLUSIONS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

impl LintRule for WeakSslCiphers {
    fn name(&self) -> &'static str {
        "weak-ssl-ciphers"
    }

    fn category(&self) -> &'static str {
        "security"
    }

    fn description(&self) -> &'static str {
        "Detects weak or insecure SSL/TLS cipher suites"
    }

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("ssl_ciphers")
                && let Some(cipher_arg) = directive.args.first()
            {
                let cipher_string = cipher_arg.as_str();

                // Check for weak cipher patterns
                let weak_ciphers = self.find_weak_ciphers(cipher_string);
                for weak in &weak_ciphers {
                    let message = format!("Weak cipher suite '{}' should not be used", weak);
                    errors.push(
                        LintError::new(self.name(), self.category(), &message, Severity::Warning)
                            .with_location(cipher_arg.span.start.line, cipher_arg.span.start.column),
                    );
                }

                // Check for missing exclusions
                let missing_exclusions = self.find_missing_exclusions(cipher_string);
                if !missing_exclusions.is_empty() {
                    // Generate fix with required exclusions added
                    let fixed_cipher_string =
                        self.generate_fixed_cipher_string(cipher_string, &missing_exclusions);
                    let indent = " ".repeat(directive.span.start.column.saturating_sub(1));

                    // Determine if original uses quotes (check raw value for quotes)
                    let quote_char = if cipher_arg.raw.starts_with('\'') {
                        "'"
                    } else if cipher_arg.raw.starts_with('"') {
                        "\""
                    } else {
                        ""
                    };

                    let fixed_line = format!(
                        "{}ssl_ciphers {}{}{};",
                        indent, quote_char, fixed_cipher_string, quote_char
                    );
                    let fix = Fix::replace_line(directive.span.start.line, &fixed_line);

                    let message = format!(
                        "Missing cipher exclusions: {}",
                        missing_exclusions.join(", ")
                    );
                    errors.push(
                        LintError::new(self.name(), self.category(), &message, Severity::Warning)
                            .with_location(cipher_arg.span.start.line, cipher_arg.span.start.column)
                            .with_fix(fix),
                    );
                }
            }
        }

        errors
    }
}

impl WeakSslCiphers {
    /// Find weak cipher patterns in the cipher string
    fn find_weak_ciphers(&self, cipher_string: &str) -> Vec<String> {
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
            for weak_pattern in &self.weak_ciphers {
                // Match the pattern in the cipher spec
                // Handle both direct matches and compound cipher names
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
    fn find_missing_exclusions(&self, cipher_string: &str) -> Vec<String> {
        let upper_cipher = cipher_string.to_uppercase();

        self.required_exclusions
            .iter()
            .filter(|exclusion| {
                // Check if the exclusion is present (case-insensitive)
                !upper_cipher.contains(&exclusion.to_uppercase())
            })
            .cloned()
            .collect()
    }

    /// Generate a fixed cipher string with missing exclusions added
    fn generate_fixed_cipher_string(&self, original: &str, missing: &[String]) -> String {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_weak_ciphers() {
        let rule = WeakSslCiphers::default();

        // Direct weak cipher
        let weak = rule.find_weak_ciphers("RC4:AES256");
        assert!(weak.contains(&"RC4".to_string()));

        // Weak cipher in compound name
        let weak = rule.find_weak_ciphers("DES-CBC3-SHA:AES128-SHA");
        assert!(!weak.is_empty());

        // No weak ciphers
        let weak = rule.find_weak_ciphers("ECDHE-RSA-AES128-GCM-SHA256");
        assert!(weak.is_empty());

        // Excluded weak cipher should not be reported
        let weak = rule.find_weak_ciphers("HIGH:!RC4");
        assert!(!weak.iter().any(|c| c == "HIGH"));
    }

    #[test]
    fn test_find_missing_exclusions() {
        let rule = WeakSslCiphers::default();

        // All exclusions missing
        let missing = rule.find_missing_exclusions("HIGH");
        assert!(!missing.is_empty());

        // Some exclusions present
        let missing = rule.find_missing_exclusions("HIGH:!aNULL:!MD5");
        assert!(!missing.contains(&"!aNULL".to_string()));
        assert!(!missing.contains(&"!MD5".to_string()));
        assert!(missing.contains(&"!RC4".to_string()));

        // All exclusions present
        let missing = rule.find_missing_exclusions("HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5");
        assert!(missing.is_empty());
    }

    #[test]
    fn test_generate_fixed_cipher_string() {
        let rule = WeakSslCiphers::default();

        let fixed = rule.generate_fixed_cipher_string("HIGH", &["!RC4".to_string(), "!MD5".to_string()]);
        assert_eq!(fixed, "HIGH:!RC4:!MD5");

        let fixed = rule.generate_fixed_cipher_string("", &["!RC4".to_string()]);
        assert_eq!(fixed, "!RC4");
    }
}
