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

/// Result of running `nginx -t` on a configuration.
#[derive(Debug)]
pub struct NginxConfigTestResult {
    /// Whether `nginx -t` exited successfully (exit code 0).
    pub success: bool,
    /// Combined stdout and stderr output from `nginx -t`.
    pub output: String,
}

impl NginxConfigTestResult {
    /// Assert that `nginx -t` rejected the configuration and the output contains
    /// the expected error message.
    ///
    /// # Panics
    ///
    /// Panics if `nginx -t` succeeded or the output does not contain `expected`.
    pub fn assert_fails_with(&self, expected: &str) {
        assert!(
            !self.success,
            "Expected nginx -t to fail, but it succeeded. Output:\n{}",
            self.output
        );
        assert!(
            self.output.contains(expected),
            "Expected output to contain {:?}, got:\n{}",
            expected,
            self.output
        );
    }

    /// Assert that `nginx -t` accepted the configuration.
    ///
    /// # Panics
    ///
    /// Panics if `nginx -t` failed.
    pub fn assert_success(&self) {
        assert!(
            self.success,
            "Expected nginx -t to succeed, but it failed. Output:\n{}",
            self.output
        );
    }

    /// Assert that `nginx -t` succeeded but emitted a warning containing the expected message.
    ///
    /// # Panics
    ///
    /// Panics if `nginx -t` failed or the output does not contain `expected`.
    pub fn assert_warns_with(&self, expected: &str) {
        assert!(
            self.success,
            "Expected nginx -t to succeed with warnings, but it failed. Output:\n{}",
            self.output
        );
        assert!(
            self.output.contains(expected),
            "Expected output to contain {:?}, got:\n{}",
            expected,
            self.output
        );
    }

    /// Assert that `nginx -t` succeeded without any warnings (`[warn]`).
    ///
    /// # Panics
    ///
    /// Panics if `nginx -t` failed or the output contains `[warn]`.
    pub fn assert_success_without_warnings(&self) {
        assert!(
            self.success,
            "Expected nginx -t to succeed, but it failed. Output:\n{}",
            self.output
        );
        assert!(
            !self.output.contains("[warn]"),
            "Expected no warnings, but got:\n{}",
            self.output
        );
    }
}

/// Run `nginx -t` on a configuration string and return the result.
///
/// This is useful for verifying that nginx rejects certain invalid configurations
/// (e.g., duplicate locations) without needing to start a full container.
///
/// # Example
///
/// ```rust,ignore
/// use nginx_lint_plugin::container_testing::nginx_config_test;
///
/// #[test]
/// #[ignore]
/// fn duplicate_location_rejected() {
///     let result = nginx_config_test(r#"
/// events { worker_connections 1024; }
/// http {
///     server {
///         listen 80;
///         location /api { return 200; }
///         location /api { return 201; }
///     }
/// }
/// "#);
///     result.assert_fails_with("duplicate location");
/// }
/// ```
pub fn nginx_config_test(config: &str) -> NginxConfigTestResult {
    let version = nginx_version();
    let image = format!("nginx:{version}");

    let mut tmpfile = std::env::temp_dir();
    tmpfile.push(format!(
        "nginx-lint-container-test-{}-{:?}.conf",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&tmpfile, config).expect("Failed to write temp nginx config");

    let result = std::process::Command::new("docker")
        .args([
            "run",
            "--rm",
            "-v",
            &format!("{}:/etc/nginx/nginx.conf:ro", tmpfile.display()),
            &image,
            "nginx",
            "-t",
        ])
        .output()
        .expect("Failed to run docker (is Docker installed and running?)");

    let _ = std::fs::remove_file(&tmpfile);

    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);

    NginxConfigTestResult {
        success: result.status.success(),
        output: format!("{stdout}{stderr}"),
    }
}
