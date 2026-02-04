//! proxy-pass-domain plugin
//!
//! This plugin warns when proxy_pass uses a domain name directly.
//!
//! When proxy_pass specifies a domain name directly, nginx resolves the DNS
//! at startup and caches the IP address. If the IP address changes, nginx
//! will continue using the old IP until restarted.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;

/// Check if proxy_pass uses a domain name directly
#[derive(Default)]
pub struct ProxyPassDomainPlugin;

impl Plugin for ProxyPassDomainPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "proxy-pass-domain",
            "best-practices",
            "Warns when proxy_pass uses a domain name directly without proper DNS handling",
        )
        .with_severity("warning")
        .with_why(
            "When proxy_pass specifies a domain name directly, nginx resolves \
             the DNS at startup and caches the IP address. If the IP address changes, nginx \
             will continue using the old IP until restarted.\n\n\
             Solutions:\n\
             1. upstream with 'resolve' and 'zone' (nginx 1.27.3+ or nginx Plus)\n\
             2. For older nginx: Use 'set $var \"domain\"' with 'resolver' directive \
             to force DNS re-resolution on each request",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_pass".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            // Check proxy_pass directive
            if directive.is("proxy_pass") {
                if let Some(url) = directive.first_arg() {
                    if let Some(host) = helpers::extract_host_from_url(url) {
                        if helpers::is_domain_name(host) {
                            let domain = helpers::extract_domain(host);
                            errors.push(LintError::warning(
                                "proxy-pass-domain",
                                "best-practices",
                                &format!(
                                    "proxy_pass uses domain '{}' directly; DNS is resolved at startup and cached. \
                                     Use upstream with 'resolve' (nginx 1.27.3+/Plus), \
                                     or use 'set $var \"{}\"' with 'resolver' for older nginx",
                                    domain, domain
                                ),
                                directive.span.start.line,
                                directive.span.start.column,
                            ));
                        }
                    }
                }
            }
        }

        errors
    }
}

// Export the plugin
nginx_lint::export_plugin!(ProxyPassDomainPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::PluginTestRunner;

    #[test]
    fn test_detects_domain_in_proxy_pass() {
        let runner = PluginTestRunner::new(ProxyPassDomainPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location / {
            proxy_pass http://api.example.com;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_detects_localhost() {
        let runner = PluginTestRunner::new(ProxyPassDomainPlugin);

        runner.assert_has_errors(
            r#"
http {
    server {
        location / {
            proxy_pass http://localhost:8080;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_allows_ip_address() {
        let runner = PluginTestRunner::new(ProxyPassDomainPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_allows_ipv6_address() {
        let runner = PluginTestRunner::new(ProxyPassDomainPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            proxy_pass http://[::1]:8080;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_allows_variable() {
        let runner = PluginTestRunner::new(ProxyPassDomainPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            set $backend "api.example.com";
            proxy_pass http://$backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_allows_upstream_name() {
        let runner = PluginTestRunner::new(ProxyPassDomainPlugin);

        runner.assert_no_errors(
            r#"
http {
    upstream backend {
        server 127.0.0.1:8080;
    }
    server {
        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_allows_unix_socket() {
        let runner = PluginTestRunner::new(ProxyPassDomainPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            proxy_pass http://unix:/var/run/app.sock;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_is_domain_name() {
        // Should be detected as domain
        assert!(helpers::is_domain_name("example.com"));
        assert!(helpers::is_domain_name("api.example.com"));
        assert!(helpers::is_domain_name("localhost"));
        assert!(helpers::is_domain_name("backend.internal"));
        assert!(helpers::is_domain_name("example.com:8080"));

        // Should NOT be detected as domain
        assert!(!helpers::is_domain_name("127.0.0.1"));
        assert!(!helpers::is_domain_name("127.0.0.1:8080"));
        assert!(!helpers::is_domain_name("[::1]"));
        assert!(!helpers::is_domain_name("[::1]:8080"));
        assert!(!helpers::is_domain_name("$backend"));
        assert!(!helpers::is_domain_name("unix:/var/run/app.sock"));
        assert!(!helpers::is_domain_name("backend")); // upstream name
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(ProxyPassDomainPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }
}
