//! Container-based testing utilities using Testcontainers.
//!
//! Provides helpers for running nginx in Docker containers to verify
//! that lint rules detect real, observable problems.
//!
//! Requires the `container-testing` feature and Docker to be running.
//!
//! # Example
//!
//! ```rust,ignore
//! use nginx_lint_plugin::container_testing::NginxContainer;
//!
//! #[tokio::test]
//! #[ignore]
//! async fn test_my_rule() {
//!     let nginx = NginxContainer::start(br#"
//! events { worker_connections 1024; }
//! http {
//!     server {
//!         listen 80;
//!         location / { return 200 'OK'; }
//!     }
//! }
//! "#).await;
//!
//!     let resp = reqwest::get(nginx.url("/")).await.unwrap();
//!     assert_eq!(resp.status(), 200);
//! }
//! ```

pub mod coredns;
pub mod nginx;

pub use reqwest;
pub use testcontainers;

// Re-export nginx module items for backward compatibility.
pub use nginx::*;

use std::time::Duration;

use coredns::CoreDnsContainer;

// ---------------------------------------------------------------------------
// DnsTestEnv — reusable CoreDNS + two-backend test environment
// ---------------------------------------------------------------------------

/// A DNS test environment with CoreDNS and two backend nginx containers.
///
/// Provides a reusable setup for testing DNS caching behaviour in nginx.
/// The environment consists of:
/// - Two backend nginx containers (`backend-a`, `backend-b`) that each return
///   their own name as the response body.
/// - A CoreDNS container that initially resolves `backend.test` to `backend-a`.
/// - A shared Docker network connecting all containers.
///
/// Use [`DnsTestEnv::start_nginx`] to launch nginx frontend containers on the
/// same network, and [`DnsTestEnv::switch_to_backend_b`] to change DNS
/// resolution from `backend-a` to `backend-b`.
pub struct DnsTestEnv {
    #[allow(dead_code)]
    backend_a: NginxContainer,
    #[allow(dead_code)]
    backend_b: NginxContainer,
    coredns: CoreDnsContainer,
    backend_b_ip: String,
    network: String,
}

/// Generate a minimal nginx config that returns a fixed body identifying a backend.
fn dns_backend_config(name: &str) -> Vec<u8> {
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

impl DnsTestEnv {
    /// Start backend-a, backend-b, and CoreDNS on a shared Docker network.
    ///
    /// Initially `backend.test` resolves to `backend-a`.
    pub async fn start(network_prefix: &str) -> Self {
        let network = format!("{network_prefix}-{}", std::process::id());

        let backend_a = NginxContainer::builder()
            .network(&network)
            .start(&dns_backend_config("backend-a"))
            .await;

        let backend_b = NginxContainer::builder()
            .network(&network)
            .start(&dns_backend_config("backend-b"))
            .await;

        let backend_a_ip = backend_a
            .bridge_ip()
            .expect("Failed to get backend-a IP")
            .to_string();
        let backend_b_ip = backend_b
            .bridge_ip()
            .expect("Failed to get backend-b IP")
            .to_string();

        eprintln!("backend-a IP: {backend_a_ip}");
        eprintln!("backend-b IP: {backend_b_ip}");

        let coredns = CoreDnsContainer::start(&network, &backend_a_ip).await;

        Self {
            backend_a,
            backend_b,
            coredns,
            backend_b_ip,
            network,
        }
    }

    /// CoreDNS IP address (for nginx `resolver` directive).
    pub fn coredns_ip(&self) -> &str {
        self.coredns.ip()
    }

    /// Docker network name (for `with_network()`).
    pub fn network(&self) -> &str {
        &self.network
    }

    /// Start an nginx frontend container on the same network with
    /// `/etc/resolv.conf` overridden to use CoreDNS.
    ///
    /// The returned container exposes port 80 and is ready to serve requests.
    pub async fn start_nginx(&self, config: Vec<u8>) -> NginxContainer {
        let startup_script = self.nginx_startup_script();

        NginxContainer::builder()
            .network(&self.network)
            .entrypoint("sh")
            .cmd(vec!["-c", &startup_script])
            .start(&config)
            .await
    }

    /// Switch DNS from `backend-a` to `backend-b` and wait for propagation.
    ///
    /// Waits 5 seconds for CoreDNS reload (1s) + DNS TTL (1s) + resolver
    /// valid (1s) + margin.
    pub async fn switch_to_backend_b(&self) {
        self.coredns.update_hosts(&self.backend_b_ip);
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    /// Startup script that overrides `/etc/resolv.conf` to use CoreDNS,
    /// then starts nginx in the foreground.
    pub fn nginx_startup_script(&self) -> String {
        format!(
            "echo 'nameserver {}' > /etc/resolv.conf && \
             exec nginx -g 'daemon off; error_log /dev/stderr notice;'",
            self.coredns_ip()
        )
    }
}
