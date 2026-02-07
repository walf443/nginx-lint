//! upstream-server-no-resolve plugin
//!
//! This plugin warns when upstream server uses a domain name without the 'resolve' parameter,
//! or when 'resolve' is used without a 'zone' directive in the upstream block.
//!
//! When upstream server specifies a domain name directly without 'resolve',
//! nginx resolves the DNS at startup and caches the IP address. If the IP
//! address changes, nginx will continue using the old IP until restarted.
//!
//! When using 'resolve', a 'zone' directive is required in the upstream block
//! to store the dynamically resolved addresses in shared memory.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;
use std::collections::HashSet;

/// Check if upstream server uses a domain without 'resolve' parameter,
/// or if 'resolve' is used without 'zone' directive
#[derive(Default)]
pub struct UpstreamServerNoResolvePlugin;

impl UpstreamServerNoResolvePlugin {
    /// Check if an upstream block has a 'zone' directive
    fn upstream_has_zone(directive: &Directive) -> bool {
        if let Some(block) = &directive.block {
            for item in &block.items {
                if let ConfigItem::Directive(d) = item {
                    if d.name == "zone" {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Collect upstream names that have 'zone' directive
    fn collect_upstreams_with_zone(config: &Config) -> HashSet<String> {
        let mut upstreams_with_zone = HashSet::new();

        for directive in config.all_directives() {
            if directive.is("upstream") {
                if let Some(name) = directive.first_arg() {
                    if Self::upstream_has_zone(directive) {
                        upstreams_with_zone.insert(name.to_string());
                    }
                }
            }
        }

        upstreams_with_zone
    }
}

impl Plugin for UpstreamServerNoResolvePlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "upstream-server-no-resolve",
            "best-practices",
            "Warns when upstream server uses a domain without 'resolve' or when 'resolve' is used without 'zone'",
        )
        .with_severity("warning")
        .with_why(
            "When upstream server specifies a domain name directly without 'resolve', nginx resolves \
             the DNS at startup and caches the IP address. If the IP address changes, nginx \
             will continue using the old IP until restarted.\n\n\
             Solutions:\n\
             1. Add 'resolve' parameter and 'zone' directive (nginx 1.27.3+ or nginx Plus)\n\
             2. For older nginx: Use a variable with 'set' and 'resolver' directive to force \
             re-resolution on each request: set $backend \"domain:port\"; proxy_pass http://$backend;\n\n\
             When using 'resolve', a 'zone' directive is also required in the upstream block to store \
             the dynamically resolved addresses in shared memory.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_upstream_module.html#server".to_string(),
            "https://nginx.org/en/docs/http/ngx_http_upstream_module.html#zone".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();

        // First pass: collect upstream names that have 'zone' directive
        let upstreams_with_zone = Self::collect_upstreams_with_zone(config);

        // Track current upstream name while iterating
        let mut current_upstream_name: Option<String> = None;

        for ctx in config.all_directives_with_context() {
            // Track which upstream we're currently in
            if ctx.directive.is("upstream") {
                current_upstream_name = ctx.directive.first_arg().map(|s| s.to_string());
            }

            // Check server directive inside upstream block
            if ctx.directive.is("server") && ctx.is_inside("upstream") {
                if let Some(address) = ctx.directive.first_arg() {
                    // The address is already just the host (first_arg returns only the first argument)
                    if helpers::is_domain_name(address) {
                        // Check if 'resolve' parameter is present in any argument
                        let has_resolve = ctx.directive.args.iter().any(|arg| {
                            arg.as_str() == "resolve"
                        });

                        let domain = helpers::extract_domain(address);

                        if !has_resolve {
                            errors.push(err.warning_at(
                                &format!(
                                    "upstream server uses domain '{}' without 'resolve' parameter; \
                                     DNS is resolved at startup and cached. \
                                     Add 'resolve' parameter and 'zone' directive (nginx 1.27.3+/Plus), \
                                     or use 'set $var \"{}\"' with 'resolver' for older versions",
                                    domain, domain
                                ),
                                ctx.directive,
                            ));
                        } else {
                            // has_resolve is true, check if zone exists
                            let has_zone = current_upstream_name
                                .as_ref()
                                .map(|name| upstreams_with_zone.contains(name))
                                .unwrap_or(false);

                            if !has_zone {
                                errors.push(err.warning_at(
                                    &format!(
                                        "upstream server uses 'resolve' for domain '{}' but upstream block has no 'zone' directive; \
                                         'zone' is required for runtime DNS resolution to store addresses in shared memory",
                                        domain
                                    ),
                                    ctx.directive,
                                ));
                            }
                        }
                    }
                }
            }
        }

        errors
    }
}

// Export the plugin
nginx_lint_plugin::export_plugin!(UpstreamServerNoResolvePlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::PluginTestRunner;

    #[test]
    fn test_upstream_server_without_resolve() {
        let runner = PluginTestRunner::new(UpstreamServerNoResolvePlugin);

        runner.assert_has_errors(
            r#"
http {
    upstream backend {
        server api.example.com:80;
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
    fn test_upstream_server_with_resolve_and_zone() {
        let runner = PluginTestRunner::new(UpstreamServerNoResolvePlugin);

        runner.assert_no_errors(
            r#"
http {
    upstream backend {
        zone backend_zone 64k;
        server api.example.com:80 resolve;
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
    fn test_upstream_server_with_resolve_but_no_zone() {
        use nginx_lint_plugin::parse_string;

        let config = parse_string(
            r#"
http {
    upstream backend {
        server api.example.com:80 resolve;
    }
}
"#,
        )
        .unwrap();

        let plugin = UpstreamServerNoResolvePlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error for missing zone, got: {:?}", errors);
        assert!(errors[0].message.contains("zone"), "Expected warning about missing zone, got: {}", errors[0].message);
    }

    #[test]
    fn test_upstream_server_with_ip_address() {
        let runner = PluginTestRunner::new(UpstreamServerNoResolvePlugin);

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
    fn test_upstream_server_with_unix_socket() {
        let runner = PluginTestRunner::new(UpstreamServerNoResolvePlugin);

        runner.assert_no_errors(
            r#"
http {
    upstream backend {
        server unix:/var/run/app.sock;
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
    fn test_upstream_server_localhost_without_resolve() {
        let runner = PluginTestRunner::new(UpstreamServerNoResolvePlugin);

        runner.assert_has_errors(
            r#"
http {
    upstream backend {
        server localhost:8080;
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
    fn test_upstream_server_with_resolve_zone_and_other_params() {
        let runner = PluginTestRunner::new(UpstreamServerNoResolvePlugin);

        runner.assert_no_errors(
            r#"
http {
    upstream backend {
        zone backend_zone 64k;
        server api.example.com:80 weight=5 resolve max_fails=3;
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
    fn test_upstream_multiple_servers_mixed() {
        use nginx_lint_plugin::parse_string;

        let config = parse_string(
            r#"
http {
    upstream backend {
        zone backend_zone 64k;
        server api.example.com:80;
        server 127.0.0.1:8080;
        server backup.example.com:80 resolve;
    }
}
"#,
        )
        .unwrap();

        let plugin = UpstreamServerNoResolvePlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should only warn for api.example.com (no resolve), not for IP or backup (has resolve with zone)
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("api.example.com"));
    }

    #[test]
    fn test_upstream_multiple_servers_no_zone() {
        use nginx_lint_plugin::parse_string;

        let config = parse_string(
            r#"
http {
    upstream backend {
        server api.example.com:80;
        server backup.example.com:80 resolve;
    }
}
"#,
        )
        .unwrap();

        let plugin = UpstreamServerNoResolvePlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should warn for api.example.com (no resolve) and backup.example.com (resolve but no zone)
        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
        assert!(errors[0].message.contains("api.example.com"));
        assert!(errors[1].message.contains("zone"));
    }

    #[test]
    fn test_multiple_upstreams_with_different_zone_configs() {
        use nginx_lint_plugin::parse_string;

        let config = parse_string(
            r#"
http {
    upstream backend1 {
        zone backend1_zone 64k;
        server api1.example.com:80 resolve;
    }
    upstream backend2 {
        server api2.example.com:80 resolve;
    }
}
"#,
        )
        .unwrap();

        let plugin = UpstreamServerNoResolvePlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should only warn for backend2 (resolve but no zone)
        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("api2.example.com"));
        assert!(errors[0].message.contains("zone"));
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(UpstreamServerNoResolvePlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(UpstreamServerNoResolvePlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}