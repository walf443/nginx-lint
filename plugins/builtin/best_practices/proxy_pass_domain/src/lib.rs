//! proxy-pass-domain plugin
//!
//! This plugin warns when proxy_pass uses a domain name directly.
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

impl ProxyPassDomainPlugin {
    /// Check if the host part is a domain name that should be warned about
    fn is_domain_name(host: &str) -> bool {
        // Empty host
        if host.is_empty() {
            return false;
        }

        // Variable reference (e.g., $backend)
        if host.starts_with('$') {
            return false;
        }

        // Unix socket
        if host.starts_with("unix:") {
            return false;
        }

        // IPv6 address (e.g., [::1])
        if host.starts_with('[') && host.contains(']') {
            return false;
        }

        // IPv4 address (all parts are numeric)
        let host_without_port = host.split(':').next().unwrap_or(host);
        if Self::is_ipv4_address(host_without_port) {
            return false;
        }

        // localhost or contains a dot (domain name)
        host_without_port == "localhost" || host_without_port.contains('.')
    }

    /// Check if the string is an IPv4 address
    fn is_ipv4_address(s: &str) -> bool {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 4 {
            return false;
        }
        parts.iter().all(|p| p.parse::<u8>().is_ok())
    }

    /// Extract host from proxy_pass URL
    /// e.g., "http://example.com:8080/path" -> "example.com:8080"
    fn extract_host(url: &str) -> Option<&str> {
        // Remove protocol
        let after_protocol = if let Some(pos) = url.find("://") {
            &url[pos + 3..]
        } else {
            return None;
        };

        // Remove path
        let host_and_port = if let Some(pos) = after_protocol.find('/') {
            &after_protocol[..pos]
        } else {
            after_protocol
        };

        Some(host_and_port)
    }
}

impl Plugin for ProxyPassDomainPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "proxy-pass-domain",
            "best-practices",
            "Warns when proxy_pass uses a domain name directly",
        )
        .with_severity("warning")
        .with_why(
            "When proxy_pass specifies a domain name directly, nginx resolves the DNS \
             at startup and caches the IP address. If the IP address changes, nginx \
             will continue using the old IP until restarted.\n\n\
             Use one of these alternatives:\n\
             - upstream block with 'resolve' parameter (requires nginx Plus or ngx_upstream_jdomain)\n\
             - Variable with 'resolver' directive for runtime DNS resolution",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_pass".to_string(),
            "https://nginx.org/en/docs/http/ngx_http_upstream_module.html#server".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("proxy_pass") {
                if let Some(url) = directive.first_arg() {
                    if let Some(host) = Self::extract_host(url) {
                        if Self::is_domain_name(host) {
                            let domain = host.split(':').next().unwrap_or(host);
                            errors.push(LintError::warning(
                                "proxy-pass-domain",
                                "best-practices",
                                &format!(
                                    "proxy_pass uses domain '{}' directly; DNS is resolved at startup and cached. \
                                     Use upstream with 'resolve' or a variable with 'resolver' instead",
                                    domain
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
        assert!(ProxyPassDomainPlugin::is_domain_name("example.com"));
        assert!(ProxyPassDomainPlugin::is_domain_name("api.example.com"));
        assert!(ProxyPassDomainPlugin::is_domain_name("localhost"));
        assert!(ProxyPassDomainPlugin::is_domain_name("backend.internal"));
        assert!(ProxyPassDomainPlugin::is_domain_name("example.com:8080"));

        // Should NOT be detected as domain
        assert!(!ProxyPassDomainPlugin::is_domain_name("127.0.0.1"));
        assert!(!ProxyPassDomainPlugin::is_domain_name("127.0.0.1:8080"));
        assert!(!ProxyPassDomainPlugin::is_domain_name("[::1]"));
        assert!(!ProxyPassDomainPlugin::is_domain_name("[::1]:8080"));
        assert!(!ProxyPassDomainPlugin::is_domain_name("$backend"));
        assert!(!ProxyPassDomainPlugin::is_domain_name("unix:/var/run/app.sock"));
        assert!(!ProxyPassDomainPlugin::is_domain_name("backend")); // upstream name
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
