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

pub use reqwest;
pub use testcontainers;

use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor, wait::HttpWaitStrategy},
    runners::AsyncRunner,
};

/// Get the nginx image tag from the `NGINX_VERSION` environment variable.
/// Defaults to `"1.27"` if not set.
pub fn nginx_version() -> String {
    std::env::var("NGINX_VERSION").unwrap_or_else(|_| "1.27".to_string())
}

/// A running nginx container for integration testing.
///
/// The container is automatically stopped and removed when this value is dropped.
pub struct NginxContainer {
    _container: ContainerAsync<GenericImage>,
    host: String,
    port: u16,
}

impl NginxContainer {
    /// Start an nginx container with the given config.
    ///
    /// Waits until `GET /` returns HTTP 200 before returning.
    /// Use [`Self::start_with_health_path`] if `/` is not suitable as a health check.
    pub async fn start(config: &[u8]) -> Self {
        Self::start_with_health_path(config, "/").await
    }

    /// Start an nginx container with the given config, using a custom health check path.
    ///
    /// This is useful when `GET /` may not return 200 (e.g., when testing
    /// autoindex off which returns 403 for directories).
    pub async fn start_with_health_path(config: &[u8], health_path: &str) -> Self {
        let version = nginx_version();
        let container = GenericImage::new("nginx", &version)
            .with_exposed_port(80.tcp())
            .with_wait_for(WaitFor::http(
                HttpWaitStrategy::new(health_path).with_expected_status_code(200u16),
            ))
            .with_copy_to("/etc/nginx/nginx.conf", config.to_vec())
            .start()
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to start nginx:{} container (is Docker running?): {}",
                    version, e
                )
            });

        let host = container.get_host().await.unwrap().to_string();
        let port = container.get_host_port_ipv4(80).await.unwrap();

        Self {
            _container: container,
            host,
            port,
        }
    }

    /// Build a full URL for the given path on this container.
    ///
    /// ```rust,ignore
    /// let url = nginx.url("/api/v1/health");
    /// // => "http://127.0.0.1:32768/api/v1/health"
    /// ```
    pub fn url(&self, path: &str) -> String {
        format!("http://{}:{}{}", self.host, self.port, path)
    }

    /// Get the host address of the container.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Get the mapped port for port 80 on the container.
    pub fn port(&self) -> u16 {
        self.port
    }
}
