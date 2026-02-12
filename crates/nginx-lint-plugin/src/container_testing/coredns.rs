//! CoreDNS container helpers for DNS-based integration testing.
//!
//! Provides [`CoreDnsContainer`] for running CoreDNS in Docker with dynamic
//! hosts file updates.

use std::time::Duration;

use testcontainers::{ContainerAsync, GenericImage, ImageExt, core::WaitFor, runners::AsyncRunner};

/// A running CoreDNS container for integration testing.
///
/// Wraps a `ContainerAsync<GenericImage>` and provides helpers for updating
/// the hosts file at runtime. The container is automatically stopped and
/// removed when this value is dropped.
pub struct CoreDnsContainer {
    container: ContainerAsync<GenericImage>,
    ip: String,
}

/// Generate the CoreDNS Corefile.
fn dns_corefile() -> Vec<u8> {
    br#".:53 {
    hosts /etc/coredns/hosts {
        reload 1s
        ttl 1
        fallthrough
    }
    log
}
"#
    .to_vec()
}

/// Generate a CoreDNS hosts file mapping `backend.test` to the given IP.
fn dns_hosts_file(ip: &str) -> Vec<u8> {
    format!("{ip} backend.test\n").into_bytes()
}

/// Overwrite the CoreDNS hosts file inside the container using `docker cp`.
///
/// CoreDNS uses a scratch base image (no shell), so `docker exec` cannot be
/// used. Instead we write a temp file on the host and copy it in.
fn docker_cp_hosts(container_id: &str, content: &[u8]) {
    let mut tmpfile = std::env::temp_dir();
    tmpfile.push(format!(
        "coredns-hosts-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
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

impl CoreDnsContainer {
    /// Start a CoreDNS container on the given network, initially resolving
    /// `backend.test` to `backend_ip`.
    pub async fn start(network: &str, backend_ip: &str) -> Self {
        let container = GenericImage::new("coredns/coredns", "latest")
            .with_wait_for(WaitFor::message_on_stdout("CoreDNS-"))
            .with_copy_to("/etc/coredns/Corefile", dns_corefile())
            .with_copy_to("/etc/coredns/hosts", dns_hosts_file(backend_ip))
            .with_cmd(vec!["-conf", "/etc/coredns/Corefile"])
            .with_network(network)
            .with_startup_timeout(Duration::from_secs(60))
            .start()
            .await
            .expect("Failed to start CoreDNS");

        let ip = container
            .get_bridge_ip_address()
            .await
            .expect("Failed to get CoreDNS IP");

        eprintln!("CoreDNS IP: {ip}");

        Self {
            container,
            ip: ip.to_string(),
        }
    }

    /// CoreDNS IP address (for nginx `resolver` directive).
    pub fn ip(&self) -> &str {
        &self.ip
    }

    /// Update the hosts file to resolve `backend.test` to the given IP.
    pub fn update_hosts(&self, ip: &str) {
        docker_cp_hosts(self.container.id(), &dns_hosts_file(ip));
    }
}
