//! Container test: upstream server without `resolve` caches DNS at startup.
//!
//! Demonstrates that `upstream { server domain; }` resolves DNS once at startup
//! and caches the result, while `upstream { zone ...; server domain resolve; }`
//! with a `resolver` directive dynamically re-resolves DNS.
//!
//! Architecture:
//! ```text
//! [CoreDNS]  <-- DNS queries --  [nginx-no-resolve]   upstream { server backend.test; }
//!            <-- DNS queries --  [nginx-resolve]       upstream { zone ...; server backend.test resolve; }
//! [backend-a] <- returns "backend-a"
//! [backend-b] <- returns "backend-b"
//! ```
//!
//! Two separate nginx frontends are used because upstream blocks with the same
//! name cannot coexist, and sharing a process would cause server group
//! cross-contamination.

use nginx_lint_plugin::container_testing::{self, DnsTestEnv, reqwest};
use std::time::Duration;

/// Generate nginx config for upstream WITHOUT `resolve` (DNS cached at startup).
fn no_resolve_config() -> Vec<u8> {
    br#"events { worker_connections 64; }
http {
    upstream backend {
        server backend.test:80;
    }
    server {
        listen 80;
        location / {
            proxy_pass http://backend;
        }
    }
}
"#
    .to_vec()
}

/// Generate nginx config for upstream WITH `resolve` + `zone` (DNS re-resolved).
///
/// Requires nginx 1.27.3+ or nginx Plus.
fn resolve_config(resolver_ip: &str) -> Vec<u8> {
    format!(
        r#"events {{ worker_connections 64; }}
http {{
    resolver {resolver_ip} valid=1s;
    upstream backend {{
        zone backend_zone 64k;
        server backend.test:80 resolve;
    }}
    server {{
        listen 80;
        location / {{
            proxy_pass http://backend;
        }}
    }}
}}
"#
    )
    .into_bytes()
}

/// Verify that `upstream { server domain; }` caches DNS at startup while
/// `upstream { zone ...; server domain resolve; }` re-resolves dynamically.
///
/// Uses two separate nginx instances to avoid upstream name collisions.
///
/// **Note**: The `resolve` parameter requires nginx 1.27.3+ (OSS) or nginx Plus.
/// OpenResty (based on nginx 1.27.1) does not support it, so this test is skipped
/// for OpenResty images.
#[tokio::test]
#[ignore]
async fn upstream_no_resolve_caches_dns_while_resolve_re_resolves() {
    // The upstream `resolve` parameter requires nginx 1.27.3+ (OSS).
    // OpenResty is based on nginx 1.27.1 and does not support it.
    if container_testing::nginx_server_name() == "openresty" {
        eprintln!("Skipping: OpenResty does not support upstream 'resolve' parameter");
        return;
    }

    let env = DnsTestEnv::start("nginx-upstream-dns-test").await;

    // --- Start nginx frontends ---
    // Frontend 1: upstream WITHOUT resolve (DNS cached at startup)
    let frontend_no_resolve = env.start_nginx(no_resolve_config()).await;
    // Frontend 2: upstream WITH resolve + zone (DNS re-resolved dynamically)
    let frontend_resolve = env.start_nginx(resolve_config(env.coredns_ip())).await;

    let host = frontend_no_resolve.host().to_string();
    let port_no_resolve = frontend_no_resolve.port();
    let port_resolve = frontend_resolve.port();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // --- Phase 1: Both frontends should reach backend-a ---

    let body_no_resolve = client
        .get(format!("http://{host}:{port_no_resolve}/"))
        .send()
        .await
        .expect("Phase 1: no-resolve request failed")
        .text()
        .await
        .unwrap();

    let body_resolve = client
        .get(format!("http://{host}:{port_resolve}/"))
        .send()
        .await
        .expect("Phase 1: resolve request failed")
        .text()
        .await
        .unwrap();

    eprintln!("Phase 1 - no resolve: {body_no_resolve}");
    eprintln!("Phase 1 - resolve:    {body_resolve}");

    assert_eq!(
        body_no_resolve, "backend-a",
        "Phase 1: upstream without resolve should reach backend-a"
    );
    assert_eq!(
        body_resolve, "backend-a",
        "Phase 1: upstream with resolve should reach backend-a"
    );

    // --- Update DNS: backend.test → backend-b ---

    env.switch_to_backend_b().await;

    // --- Phase 2: no-resolve should still reach backend-a (stale), resolve should reach backend-b ---

    let body_no_resolve = client
        .get(format!("http://{host}:{port_no_resolve}/"))
        .send()
        .await
        .expect("Phase 2: no-resolve request failed")
        .text()
        .await
        .unwrap();

    let body_resolve = client
        .get(format!("http://{host}:{port_resolve}/"))
        .send()
        .await
        .expect("Phase 2: resolve request failed")
        .text()
        .await
        .unwrap();

    eprintln!("Phase 2 - no resolve: {body_no_resolve}");
    eprintln!("Phase 2 - resolve:    {body_resolve}");

    // Without 'resolve', DNS is cached from startup → still backend-a.
    // This is the problem that the upstream-server-no-resolve lint rule warns about.
    assert_eq!(
        body_no_resolve, "backend-a",
        "Phase 2: upstream without resolve should STILL reach backend-a (stale DNS cache - \
         this is the problem the lint rule detects)"
    );

    // With 'resolve' + 'zone', upstream re-resolves DNS → now backend-b.
    // This is the recommended solution (nginx 1.27.3+).
    assert_eq!(
        body_resolve, "backend-b",
        "Phase 2: upstream with resolve should reach backend-b (DNS re-resolved - \
         this is the recommended solution)"
    );
}
