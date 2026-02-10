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
    // The hosts plugin with `reload 1s` re-reads the file every second.
    // `ttl 1` sets DNS TTL to 1 second, and `fallthrough` passes to the
    // next plugin for unmatched queries.
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

/// Verify that `proxy_pass http://domain` caches DNS at startup while
/// `set $var` + `resolver` re-resolves on each request.
///
/// Uses two separate nginx instances to avoid implicit server group sharing
/// between direct-domain and variable-based proxy_pass configurations.
#[tokio::test]
#[ignore]
async fn domain_proxy_pass_caches_dns_while_variable_re_resolves() {
    let network = format!("nginx-dns-test-{}", std::process::id());

    // --- Start backend containers ---
    // These use plain nginx with the same version as the test image.

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
    // Two separate nginx instances: one with direct domain, one with variable+resolver.
    // Using separate processes avoids implicit server group sharing.
    //
    // Docker overwrites /etc/resolv.conf at container creation time, so we use
    // a custom entrypoint to set resolv.conf at runtime before starting nginx.

    let (img_name, img_tag) = container_testing::nginx_image();
    let conf_path = container_testing::nginx_conf_path();
    let startup_script = nginx_startup_script(&coredns_ip.to_string());

    // Frontend 1: direct domain (DNS cached at startup via resolv.conf)
    let frontend_direct = GenericImage::new(&img_name, &img_tag)
        .with_exposed_port(80.tcp())
        .with_entrypoint("sh")
        .with_wait_for(WaitFor::http(
            HttpWaitStrategy::new("/").with_expected_status_code(200u16),
        ))
        .with_copy_to(&conf_path, direct_domain_config())
        .with_cmd(vec!["-c", &startup_script])
        .with_network(&network)
        .with_startup_timeout(Duration::from_secs(120))
        .start()
        .await
        .expect("Failed to start nginx frontend (direct domain)");

    // Frontend 2: variable + resolver (DNS re-resolved per request)
    let frontend_variable = GenericImage::new(&img_name, &img_tag)
        .with_exposed_port(80.tcp())
        .with_entrypoint("sh")
        .with_wait_for(WaitFor::http(
            HttpWaitStrategy::new("/").with_expected_status_code(200u16),
        ))
        .with_copy_to(
            &conf_path,
            variable_resolver_config(&coredns_ip.to_string()),
        )
        .with_cmd(vec!["-c", &startup_script])
        .with_network(&network)
        .with_startup_timeout(Duration::from_secs(120))
        .start()
        .await
        .expect("Failed to start nginx frontend (variable+resolver)");

    let host = frontend_direct.get_host().await.unwrap().to_string();
    let port_direct = frontend_direct.get_host_port_ipv4(80).await.unwrap();
    let port_variable = frontend_variable.get_host_port_ipv4(80).await.unwrap();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // --- Phase 1: Both frontends should reach backend-a ---

    let body_direct = client
        .get(format!("http://{host}:{port_direct}/"))
        .send()
        .await
        .expect("Phase 1: direct request failed")
        .text()
        .await
        .unwrap();

    let body_variable = client
        .get(format!("http://{host}:{port_variable}/"))
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

    docker_cp_hosts(coredns.id(), &hosts_file(&backend_b_ip.to_string()));

    // Wait for CoreDNS to reload (reload 1s) + DNS TTL (1s) + resolver valid (1s) + margin
    tokio::time::sleep(Duration::from_secs(5)).await;

    // --- Phase 2: Direct should still reach backend-a (stale), variable should reach backend-b ---

    let body_direct = client
        .get(format!("http://{host}:{port_direct}/"))
        .send()
        .await
        .expect("Phase 2: direct request failed")
        .text()
        .await
        .unwrap();

    let body_variable = client
        .get(format!("http://{host}:{port_variable}/"))
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
