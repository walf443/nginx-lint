//! Rule documentation for nginx-lint
//!
//! This module provides detailed documentation for each lint rule,
//! explaining why the rule exists and what the recommended configuration is.

/// Documentation for a lint rule
pub struct RuleDoc {
    /// Rule name (e.g., "server-tokens-enabled")
    pub name: &'static str,
    /// Category (e.g., "security")
    pub category: &'static str,
    /// Short description
    pub description: &'static str,
    /// Severity level
    pub severity: &'static str,
    /// Why this rule exists
    pub why: &'static str,
    /// Example of bad configuration
    pub bad_example: &'static str,
    /// Example of good configuration
    pub good_example: &'static str,
    /// References (URLs, documentation links)
    pub references: &'static [&'static str],
}

/// Get documentation for a rule by name
pub fn get_rule_doc(name: &str) -> Option<&'static RuleDoc> {
    RULE_DOCS.iter().find(|doc| doc.name == name)
}

/// Get all rule documentation
pub fn all_rule_docs() -> &'static [RuleDoc] {
    RULE_DOCS
}

/// Get all rule names
pub fn all_rule_names() -> Vec<&'static str> {
    RULE_DOCS.iter().map(|doc| doc.name).collect()
}

static RULE_DOCS: &[RuleDoc] = &[
    // ==========================================================================
    // Security Rules
    // ==========================================================================
    RuleDoc {
        name: "server-tokens-enabled",
        category: "security",
        description: "Detects when server_tokens is enabled",
        severity: "warning",
        why: r#"When server_tokens is set to 'on', nginx exposes version information
in response headers and error pages. Attackers can use this information
to target known vulnerabilities for that specific version.

Hiding version information raises the difficulty of targeted attacks."#,
        bad_example: r#"http {
    server_tokens on;  # Version info exposed
}"#,
        good_example: r#"http {
    server_tokens off;  # Hide version info
}"#,
        references: &[
            "https://nginx.org/en/docs/http/ngx_http_core_module.html#server_tokens",
        ],
    },
    RuleDoc {
        name: "autoindex-enabled",
        category: "security",
        description: "Detects when autoindex is enabled",
        severity: "warning",
        why: r#"When autoindex is enabled, directory contents are listed publicly.
This can expose unintended files and directory structures, potentially
leading to information disclosure and security risks.

Autoindex should be disabled unless explicitly required."#,
        bad_example: r#"location /files {
    autoindex on;  # Directory listing exposed
}"#,
        good_example: r#"location /files {
    autoindex off;  # Disable directory listing
    # Or simply omit the directive (default is off)
}"#,
        references: &[
            "https://nginx.org/en/docs/http/ngx_http_autoindex_module.html",
        ],
    },
    RuleDoc {
        name: "deprecated-ssl-protocol",
        category: "security",
        description: "Detects deprecated SSL/TLS protocols",
        severity: "warning",
        why: r#"SSLv2, SSLv3, TLSv1.0, and TLSv1.1 have known vulnerabilities and
are deprecated. Using these protocols makes your server vulnerable to
attacks like POODLE, BEAST, and CRIME.

Only TLSv1.2 and above should be used."#,
        bad_example: r#"server {
    ssl_protocols SSLv3 TLSv1 TLSv1.1 TLSv1.2;
}"#,
        good_example: r#"server {
    ssl_protocols TLSv1.2 TLSv1.3;
}"#,
        references: &[
            "https://nginx.org/en/docs/http/ngx_http_ssl_module.html#ssl_protocols",
            "https://wiki.mozilla.org/Security/Server_Side_TLS",
        ],
    },
    RuleDoc {
        name: "weak-ssl-ciphers",
        category: "security",
        description: "Detects weak SSL/TLS cipher suites",
        severity: "warning",
        why: r#"Weak cipher suites (NULL, EXPORT, DES, RC4, MD5, etc.) have
insufficient cryptographic strength or known vulnerabilities.

Using only strong cipher suites and explicitly excluding weak ones
ensures secure communication."#,
        bad_example: r#"server {
    ssl_ciphers ALL;  # Includes weak ciphers
}"#,
        good_example: r#"server {
    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5;
    ssl_prefer_server_ciphers on;
}"#,
        references: &[
            "https://nginx.org/en/docs/http/ngx_http_ssl_module.html#ssl_ciphers",
            "https://wiki.mozilla.org/Security/Server_Side_TLS",
            "https://ssl-config.mozilla.org/",
        ],
    },
    // ==========================================================================
    // Syntax Rules
    // ==========================================================================
    RuleDoc {
        name: "unmatched-braces",
        category: "syntax",
        description: "Detects unmatched opening or closing braces",
        severity: "error",
        why: r#"When braces are unmatched, nginx cannot parse the configuration
file correctly and will fail to start.

This rule checks that opening braces '{' and closing braces '}'
are balanced, and that block directives have their opening brace."#,
        bad_example: r#"http {
    server {
        listen 80;
    # Missing closing brace
}"#,
        good_example: r#"http {
    server {
        listen 80;
    }
}"#,
        references: &[
            "https://nginx.org/en/docs/beginners_guide.html",
        ],
    },
    RuleDoc {
        name: "unclosed-quote",
        category: "syntax",
        description: "Detects unclosed quotes in directive values",
        severity: "error",
        why: r#"When quotes are not closed, nginx cannot parse the configuration
correctly and may fail to start or behave unexpectedly.

Strings enclosed in quotes must be closed with the same quote type."#,
        bad_example: r#"location / {
    add_header X-Custom "value;  # Missing closing quote
}"#,
        good_example: r#"location / {
    add_header X-Custom "value";
}"#,
        references: &[],
    },
    RuleDoc {
        name: "missing-semicolon",
        category: "syntax",
        description: "Detects missing semicolons at the end of directives",
        severity: "error",
        why: r#"In nginx configuration, each directive must end with a semicolon.
Without it, nginx cannot parse the configuration correctly.

Block directives (server, location, etc.) don't need semicolons,
but regular directives always require them."#,
        bad_example: r#"server {
    listen 80
    server_name example.com
}"#,
        good_example: r#"server {
    listen 80;
    server_name example.com;
}"#,
        references: &[],
    },
    RuleDoc {
        name: "duplicate-directive",
        category: "syntax",
        description: "Detects duplicate directives in the same context",
        severity: "warning",
        why: r#"Some directives cannot be specified multiple times in the same context.
When duplicated, nginx may use only the last value or throw an error.

Duplicate directives often indicate unintentional configuration mistakes
and should be reviewed."#,
        bad_example: r#"server {
    listen 80;
    listen 80;  # Duplicate
}"#,
        good_example: r#"server {
    listen 80;
}"#,
        references: &[],
    },
    // ==========================================================================
    // Style Rules
    // ==========================================================================
    RuleDoc {
        name: "indent",
        category: "style",
        description: "Detects inconsistent indentation",
        severity: "warning",
        why: r#"Consistent indentation improves readability of configuration files.
Properly indented nested blocks make the structure visually clear
and easier to understand.

Using spaces instead of tabs ensures consistent appearance
across different environments."#,
        bad_example: r#"http {
server {
listen 80;
}
}"#,
        good_example: r#"http {
  server {
    listen 80;
  }
}"#,
        references: &[],
    },
    RuleDoc {
        name: "trailing-whitespace",
        category: "style",
        description: "Detects trailing whitespace at the end of lines",
        severity: "warning",
        why: r#"Trailing whitespace is invisible and can cause unnecessary diffs
in version control and hinder code reviews.

Removing trailing whitespace keeps configuration files clean."#,
        bad_example: "listen 80;   \n# Trailing whitespace at end of line",
        good_example: "listen 80;\n# No trailing whitespace",
        references: &[],
    },
    RuleDoc {
        name: "space-before-semicolon",
        category: "style",
        description: "Detects spaces or tabs before semicolons",
        severity: "warning",
        why: r#"Spaces before semicolons violate common coding style conventions
and reduce readability.

Semicolons should be placed immediately after directive values."#,
        bad_example: r#"server {
    listen 80 ;  # Unnecessary space before semicolon
}"#,
        good_example: r#"server {
    listen 80;
}"#,
        references: &[],
    },
    // ==========================================================================
    // Best Practices Rules
    // ==========================================================================
    RuleDoc {
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
    },
    RuleDoc {
        name: "missing-error-log",
        category: "best_practices",
        description: "Suggests configuring error_log",
        severity: "info",
        why: r#"Configuring error_log allows you to record errors and issues
in log files for troubleshooting purposes.

Setting an appropriate log level helps capture necessary information
while managing disk usage."#,
        bad_example: r#"# error_log not configured
http {
    server {
        listen 80;
    }
}"#,
        good_example: r#"error_log /var/log/nginx/error.log warn;

http {
    server {
        listen 80;
    }
}"#,
        references: &[
            "https://nginx.org/en/docs/ngx_core_module.html#error_log",
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_rule_doc() {
        let doc = get_rule_doc("server-tokens-enabled");
        assert!(doc.is_some());
        let doc = doc.unwrap();
        assert_eq!(doc.name, "server-tokens-enabled");
        assert_eq!(doc.category, "security");
    }

    #[test]
    fn test_get_rule_doc_not_found() {
        let doc = get_rule_doc("nonexistent-rule");
        assert!(doc.is_none());
    }

    #[test]
    fn test_all_rule_names() {
        let names = all_rule_names();
        assert!(names.contains(&"server-tokens-enabled"));
        assert!(names.contains(&"indent"));
        assert!(names.contains(&"gzip-not-enabled"));
    }
}
