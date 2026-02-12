//! Container test: proxy_pass with domain names caches DNS at startup.
//!
//! Demonstrates that `proxy_pass http://domain` resolves DNS once at startup
//! and caches the result, while `set $var "domain"; proxy_pass http://$var`
//! with a `resolver` directive re-resolves on each request.
//!
//! Architecture:
//! ```text
//! [CoreDNS]  <-- DNS queries --  [nginx-direct]   proxy_pass http://backend.test
//!            <-- DNS queries --  [nginx-variable]  set $backend + resolver
//! [backend-a] <- returns "backend-a"
//! [backend-b] <- returns "backend-b"
//! ```
//!
//! Two separate nginx frontends are used because a direct `proxy_pass http://domain`
//! creates an implicit server group that would also be used by variable-based
//! `proxy_pass http://$var` in the same process, masking the resolver behavior.

use nginx_lint_plugin::container_testing::{DnsTestEnv, reqwest};
use std::time::Duration;

/// Generate nginx config for the "direct domain" approach (DNS cached at startup).
fn direct_domain_config() -> Vec<u8> {
    br#"events { worker_connections 64; }
http {
    server {
        listen 80;
        location / {
            proxy_pass http://backend.test;
        }
    }
}
"#
    .to_vec()
}

/// Generate nginx config for the "variable + resolver" approach (DNS re-resolved).
fn variable_resolver_config(resolver_ip: &str) -> Vec<u8> {
    format!(
        r#"events {{ worker_connections 64; }}
http {{
    server {{
        listen 80;
        resolver {resolver_ip} valid=1s;
        location / {{
            set $backend "backend.test";
            proxy_pass http://$backend;
        }}
    }}
}}
"#
    )
    .into_bytes()
}

/// Verify that `proxy_pass http://domain` caches DNS at startup while
/// `set $var` + `resolver` re-resolves on each request.
///
/// Uses two separate nginx instances to avoid implicit server group sharing
/// between direct-domain and variable-based proxy_pass configurations.
#[tokio::test]
#[ignore]
async fn domain_proxy_pass_caches_dns_while_variable_re_resolves() {
    let env = DnsTestEnv::start("nginx-dns-test").await;

    // --- Start nginx frontends ---
    // Frontend 1: direct domain (DNS cached at startup via resolv.conf)
    let frontend_direct = env.start_nginx(direct_domain_config()).await;
    // Frontend 2: variable + resolver (DNS re-resolved per request)
    let frontend_variable = env
        .start_nginx(variable_resolver_config(env.coredns_ip()))
        .await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // --- Phase 1: Both frontends should reach backend-a ---

    let body_direct = client
        .get(frontend_direct.url("/"))
        .send()
        .await
        .expect("Phase 1: direct request failed")
        .text()
        .await
        .unwrap();

    let body_variable = client
        .get(frontend_variable.url("/"))
        .send()
        .await
        .expect("Phase 1: variable request failed")
        .text()
        .await
        .unwrap();

    eprintln!("Phase 1 - direct:   {body_direct}");
    eprintln!("Phase 1 - variable: {body_variable}");

    assert_eq!(
        body_direct, "backend-a",
        "Phase 1: direct proxy should reach backend-a"
    );
    assert_eq!(
        body_variable, "backend-a",
        "Phase 1: variable proxy should reach backend-a"
    );

    // --- Update DNS: backend.test → backend-b ---

    env.switch_to_backend_b().await;

    // --- Phase 2: Direct should still reach backend-a (stale), variable should reach backend-b ---

    let body_direct = client
        .get(frontend_direct.url("/"))
        .send()
        .await
        .expect("Phase 2: direct request failed")
        .text()
        .await
        .unwrap();

    let body_variable = client
        .get(frontend_variable.url("/"))
        .send()
        .await
        .expect("Phase 2: variable request failed")
        .text()
        .await
        .unwrap();

    eprintln!("Phase 2 - direct:   {body_direct}");
    eprintln!("Phase 2 - variable: {body_variable}");

    // The direct proxy_pass caches the DNS from startup → still backend-a.
    // This is the problem that the proxy-pass-domain lint rule warns about.
    assert_eq!(
        body_direct, "backend-a",
        "Phase 2: direct proxy should STILL reach backend-a (stale DNS cache - \
         this is the problem the lint rule detects)"
    );

    // The variable + resolver approach re-resolves DNS → now backend-b.
    // This is the recommended solution.
    assert_eq!(
        body_variable, "backend-b",
        "Phase 2: variable proxy should reach backend-b (DNS re-resolved - \
         this is the recommended solution)"
    );
}
