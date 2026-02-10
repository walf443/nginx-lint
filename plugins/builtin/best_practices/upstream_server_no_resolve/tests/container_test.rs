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

use nginx_lint_plugin::container_testing::{self, reqwest, testcontainers};
use std::time::Duration;
use testcontainers::core::{IntoContainerPort, WaitFor, wait::HttpWaitStrategy};
use testcontainers::{GenericImage, ImageExt, runners::AsyncRunner};

/// Generate a minimal nginx config that returns a fixed body identifying this backend.
fn backend_config(name: &str) -> Vec<u8> {
    format!(
        r#"events {{ worker_connections 64; }}
http {{
    server {{
        listen 80;
        location / {{
            return 200 '{name}';
        }}
    }}
}}
"#
    )
    .into_bytes()
}

/// Generate the CoreDNS Corefile.
fn corefile() -> String {
    r#".:53 {
    hosts /etc/coredns/hosts {
        reload 1s
        ttl 1
        fallthrough
    }
    log
}
"#
    .to_string()
}

/// Generate a CoreDNS hosts file mapping backend.test to the given IP.
fn hosts_file(ip: &str) -> String {
    format!("{ip} backend.test\n")
}

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

/// Build a startup script that overrides /etc/resolv.conf to use our CoreDNS,
/// then starts nginx in the foreground.
///
/// Docker sets /etc/resolv.conf at container creation time (after `with_copy_to`),
/// so we must override it at runtime via the entrypoint.
fn nginx_startup_script(dns_ip: &str) -> String {
    format!(
        "echo 'nameserver {dns_ip}' > /etc/resolv.conf && \
         exec nginx -g 'daemon off; error_log /dev/stderr notice;'"
    )
}

/// Overwrite the CoreDNS hosts file inside the container using `docker cp`.
///
/// CoreDNS uses a scratch base image (no shell), so `docker exec` cannot be
/// used. Instead we write a temp file on the host and copy it in.
fn docker_cp_hosts(container_id: &str, content: &str) {
    let mut tmpfile = std::env::temp_dir();
    tmpfile.push(format!("coredns-hosts-{}", std::process::id()));
    std::fs::write(&tmpfile, content).expect("Failed to write temp hosts file");

    let output = std::process::Command::new("docker")
        .args([
            "cp",
            &tmpfile.display().to_string(),
            &format!("{container_id}:/etc/coredns/hosts"),
        ])
        .output()
        .expect("Failed to run docker cp");

    let _ = std::fs::remove_file(&tmpfile);

    assert!(
        output.status.success(),
        "docker cp failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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

    let network = format!("nginx-upstream-dns-test-{}", std::process::id());

    // --- Start backend containers ---

    let nginx_version = container_testing::nginx_version();

    let backend_a = GenericImage::new("nginx", &nginx_version)
        .with_exposed_port(80.tcp())
        .with_wait_for(WaitFor::http(
            HttpWaitStrategy::new("/").with_expected_status_code(200u16),
        ))
        .with_copy_to("/etc/nginx/nginx.conf", backend_config("backend-a"))
        .with_network(&network)
        .with_startup_timeout(Duration::from_secs(120))
        .start()
        .await
        .expect("Failed to start backend-a");

    let backend_b = GenericImage::new("nginx", &nginx_version)
        .with_exposed_port(80.tcp())
        .with_wait_for(WaitFor::http(
            HttpWaitStrategy::new("/").with_expected_status_code(200u16),
        ))
        .with_copy_to("/etc/nginx/nginx.conf", backend_config("backend-b"))
        .with_network(&network)
        .with_startup_timeout(Duration::from_secs(120))
        .start()
        .await
        .expect("Failed to start backend-b");

    let backend_a_ip = backend_a
        .get_bridge_ip_address()
        .await
        .expect("Failed to get backend-a IP");
    let backend_b_ip = backend_b
        .get_bridge_ip_address()
        .await
        .expect("Failed to get backend-b IP");

    eprintln!("backend-a IP: {backend_a_ip}");
    eprintln!("backend-b IP: {backend_b_ip}");

    // --- Start CoreDNS pointing backend.test → backend-a ---

    let coredns = GenericImage::new("coredns/coredns", "latest")
        .with_wait_for(WaitFor::message_on_stdout("CoreDNS-"))
        .with_copy_to("/etc/coredns/Corefile", corefile().into_bytes())
        .with_copy_to(
            "/etc/coredns/hosts",
            hosts_file(&backend_a_ip.to_string()).into_bytes(),
        )
        .with_cmd(vec!["-conf", "/etc/coredns/Corefile"])
        .with_network(&network)
        .with_startup_timeout(Duration::from_secs(60))
        .start()
        .await
        .expect("Failed to start CoreDNS");

    let coredns_ip = coredns
        .get_bridge_ip_address()
        .await
        .expect("Failed to get CoreDNS IP");

    eprintln!("CoreDNS IP: {coredns_ip}");

    // --- Start nginx frontends ---
    // Docker overwrites /etc/resolv.conf at container creation time, so we use
    // a custom entrypoint to set resolv.conf at runtime before starting nginx.

    let (img_name, img_tag) = container_testing::nginx_image();
    let conf_path = container_testing::nginx_conf_path();
    let startup_script = nginx_startup_script(&coredns_ip.to_string());

    // Frontend 1: upstream WITHOUT resolve (DNS cached at startup)
    let frontend_no_resolve = GenericImage::new(&img_name, &img_tag)
        .with_exposed_port(80.tcp())
        .with_entrypoint("sh")
        .with_wait_for(WaitFor::http(
            HttpWaitStrategy::new("/").with_expected_status_code(200u16),
        ))
        .with_copy_to(&conf_path, no_resolve_config())
        .with_cmd(vec!["-c", &startup_script])
        .with_network(&network)
        .with_startup_timeout(Duration::from_secs(120))
        .start()
        .await
        .expect("Failed to start nginx frontend (no resolve)");

    // Frontend 2: upstream WITH resolve + zone (DNS re-resolved dynamically)
    let frontend_resolve = GenericImage::new(&img_name, &img_tag)
        .with_exposed_port(80.tcp())
        .with_entrypoint("sh")
        .with_wait_for(WaitFor::http(
            HttpWaitStrategy::new("/").with_expected_status_code(200u16),
        ))
        .with_copy_to(
            &conf_path,
            resolve_config(&coredns_ip.to_string()),
        )
        .with_cmd(vec!["-c", &startup_script])
        .with_network(&network)
        .with_startup_timeout(Duration::from_secs(120))
        .start()
        .await
        .expect("Failed to start nginx frontend (resolve)");

    let host = frontend_no_resolve.get_host().await.unwrap().to_string();
    let port_no_resolve = frontend_no_resolve.get_host_port_ipv4(80).await.unwrap();
    let port_resolve = frontend_resolve.get_host_port_ipv4(80).await.unwrap();

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

    docker_cp_hosts(coredns.id(), &hosts_file(&backend_b_ip.to_string()));

    // Wait for CoreDNS to reload (reload 1s) + DNS TTL (1s) + resolver valid (1s) + margin
    tokio::time::sleep(Duration::from_secs(5)).await;

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
