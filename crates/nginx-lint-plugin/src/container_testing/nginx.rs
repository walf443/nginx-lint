//! Nginx container helpers for integration testing.
//!
//! Provides [`NginxContainer`] for running nginx in Docker, helper functions
//! for image configuration, and [`nginx_config_test`] for validating configs
//! with `nginx -t`.

use std::time::Duration;

use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{ExecCommand, IntoContainerPort, WaitFor, wait::HttpWaitStrategy},
    runners::AsyncRunner,
};

/// Describes the Docker image and paths for an nginx-compatible container.
struct NginxImageConfig {
    image_name: String,
    image_tag: String,
    conf_path: String,
}

impl NginxImageConfig {
    fn full_image(&self) -> String {
        format!("{}:{}", self.image_name, self.image_tag)
    }
}

/// Build an [`NginxImageConfig`] from environment variables.
///
/// - If `NGINX_IMAGE` is set (e.g. `openresty/openresty:noble`), it is split
///   into name and tag. Images whose name contains `"openresty"` automatically
///   use the OpenResty config path.
/// - Otherwise falls back to `nginx:$NGINX_VERSION` (default `"1.27"`).
fn nginx_image_config() -> NginxImageConfig {
    if let Ok(image) = std::env::var("NGINX_IMAGE") {
        let (name, tag) = match image.rsplit_once(':') {
            Some((n, t)) => (n.to_string(), t.to_string()),
            None => (image.clone(), "latest".to_string()),
        };
        let conf_path = if name.contains("openresty") {
            "/usr/local/openresty/nginx/conf/nginx.conf".to_string()
        } else {
            "/etc/nginx/nginx.conf".to_string()
        };
        NginxImageConfig {
            image_name: name,
            image_tag: tag,
            conf_path,
        }
    } else {
        let version = std::env::var("NGINX_VERSION").unwrap_or_else(|_| "1.27".to_string());
        NginxImageConfig {
            image_name: "nginx".to_string(),
            image_tag: version,
            conf_path: "/etc/nginx/nginx.conf".to_string(),
        }
    }
}

fn is_openresty_image() -> bool {
    std::env::var("NGINX_IMAGE")
        .map(|v| v.contains("openresty"))
        .unwrap_or(false)
}

/// Get the default HTML document root for the current container image.
///
/// - nginx: `/usr/share/nginx/html`
/// - openresty: `/usr/local/openresty/nginx/html`
pub fn nginx_html_root() -> &'static str {
    if is_openresty_image() {
        "/usr/local/openresty/nginx/html"
    } else {
        "/usr/share/nginx/html"
    }
}

/// Get the configuration directory for the current container image.
///
/// - nginx: `/etc/nginx`
/// - openresty: `/usr/local/openresty/nginx/conf`
pub fn nginx_conf_dir() -> &'static str {
    if is_openresty_image() {
        "/usr/local/openresty/nginx/conf"
    } else {
        "/etc/nginx"
    }
}

/// Get the server software name for the current container image.
///
/// - nginx: `"nginx"`
/// - openresty: `"openresty"`
pub fn nginx_server_name() -> &'static str {
    if is_openresty_image() {
        "openresty"
    } else {
        "nginx"
    }
}

/// Get the nginx image tag from the `NGINX_VERSION` environment variable.
/// Defaults to `"1.27"` if not set.
pub fn nginx_version() -> String {
    std::env::var("NGINX_VERSION").unwrap_or_else(|_| "1.27".to_string())
}

/// Get the (image_name, image_tag) for creating a [`GenericImage`] directly.
///
/// This is useful when you need full control over container creation
/// (e.g., custom networks, multiple containers) instead of using [`NginxContainer`].
pub fn nginx_image() -> (String, String) {
    let cfg = nginx_image_config();
    (cfg.image_name, cfg.image_tag)
}

/// Get the full path to nginx.conf inside the container.
pub fn nginx_conf_path() -> String {
    let cfg = nginx_image_config();
    cfg.conf_path
}

/// A running nginx container for integration testing.
///
/// The container is automatically stopped and removed when this value is dropped.
pub struct NginxContainer {
    container: ContainerAsync<GenericImage>,
    host: String,
    port: u16,
    tls: bool,
    bridge_ip: Option<String>,
}

/// Output from executing a command inside a container.
#[derive(Debug)]
pub struct ExecOutput {
    /// Standard output from the command.
    pub stdout: String,
    /// Standard error from the command.
    pub stderr: String,
    /// Exit code of the command.
    pub exit_code: i64,
}

impl ExecOutput {
    /// Get the combined stdout and stderr output.
    pub fn output(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

impl NginxContainer {
    /// Start an nginx container with the given config.
    ///
    /// Waits until `GET /` returns HTTP 200 before returning.
    /// Use [`NginxContainer::builder`] with [`NginxContainerBuilder::health_path`]
    /// if `/` is not suitable as a health check.
    pub async fn start(config: impl Into<Vec<u8>>) -> Self {
        Self::builder().start(config).await
    }

    /// Start an nginx container with the given config, using a custom health check path.
    ///
    /// This is useful when `GET /` may not return 200 (e.g., when testing
    /// autoindex off which returns 403 for directories).
    ///
    /// # Deprecated
    ///
    /// Use [`NginxContainer::builder`] with [`NginxContainerBuilder::health_path`] instead:
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use nginx_lint_plugin::container_testing::NginxContainer;
    ///
    /// let nginx = NginxContainer::builder()
    ///     .health_path("/healthz")
    ///     .start(b"events {} http { server { listen 80; } }")
    ///     .await;
    /// # }
    /// ```
    #[deprecated(note = "use NginxContainer::builder().health_path(...).start(...) instead")]
    pub async fn start_with_health_path(config: impl Into<Vec<u8>>, health_path: &str) -> Self {
        Self::builder().health_path(health_path).start(config).await
    }

    /// Create a builder for configuring an nginx container with advanced options.
    ///
    /// Use the builder when you need to attach the container to a Docker network
    /// or customise the health-check path.
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use nginx_lint_plugin::container_testing::NginxContainer;
    ///
    /// let nginx = NginxContainer::builder()
    ///     .network("my-network")
    ///     .start(b"events {} http { server { listen 80; } }")
    ///     .await;
    /// # }
    /// ```
    pub fn builder() -> NginxContainerBuilder {
        NginxContainerBuilder {
            network: None,
            health_path: "/".to_string(),
            entrypoint: None,
            cmd: None,
            wait_for: None,
            expose_port: Some(80),
        }
    }

    /// Start an nginx container with SSL support.
    ///
    /// Generates a self-signed certificate at `/tmp/cert.pem` and `/tmp/key.pem`,
    /// then starts nginx with the provided configuration.
    ///
    /// The configuration should reference these certificate paths:
    /// ```nginx
    /// ssl_certificate /tmp/cert.pem;
    /// ssl_certificate_key /tmp/key.pem;
    /// ```
    ///
    /// Use [`Self::exec`] or [`Self::exec_shell`] to run commands (like
    /// `openssl s_client`) inside the container for protocol-level testing.
    pub async fn start_ssl(config: impl Into<Vec<u8>>) -> Self {
        let startup_script = concat!(
            "openssl req -x509 -nodes -days 1 -newkey rsa:2048 ",
            "-keyout /tmp/key.pem -out /tmp/cert.pem ",
            "-subj '/CN=test' 2>/dev/null && ",
            "exec nginx -g 'daemon off; error_log /dev/stderr notice;'"
        );

        Self::builder()
            .entrypoint("sh")
            .cmd(vec!["-c", startup_script])
            .wait_for(WaitFor::message_on_stderr("start worker process"))
            .expose_port(Some(443))
            .start(config)
            .await
    }

    /// Execute a command inside the running container and return the output.
    pub async fn exec(&self, cmd: &[&str]) -> ExecOutput {
        let exec_cmd = ExecCommand::new(cmd.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let mut result = self
            .container
            .exec(exec_cmd)
            .await
            .expect("Failed to exec command in container");

        let stdout =
            String::from_utf8_lossy(&result.stdout_to_vec().await.unwrap_or_default()).to_string();
        let stderr =
            String::from_utf8_lossy(&result.stderr_to_vec().await.unwrap_or_default()).to_string();
        let exit_code = result.exit_code().await.ok().flatten().unwrap_or(-1);

        ExecOutput {
            stdout,
            stderr,
            exit_code,
        }
    }

    /// Execute a shell command inside the running container.
    ///
    /// This is a convenience wrapper around [`Self::exec`] that runs the
    /// script via `sh -c`.
    pub async fn exec_shell(&self, script: &str) -> ExecOutput {
        self.exec(&["sh", "-c", script]).await
    }

    /// Build a full URL for the given path on this container.
    ///
    /// Returns `https://` for SSL containers, `http://` otherwise.
    ///
    /// ```text
    /// let url = nginx.url("/api/v1/health");
    /// // => "http://127.0.0.1:32768/api/v1/health"
    /// ```
    pub fn url(&self, path: &str) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{scheme}://{}:{}{}", self.host, self.port, path)
    }

    /// Get the host address of the container.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Get the mapped port for port 80 on the container.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the bridge IP address of the container, if it was started on a network.
    ///
    /// This is only available when the container was created via
    /// [`NginxContainerBuilder`] with a network configured.
    pub fn bridge_ip(&self) -> Option<&str> {
        self.bridge_ip.as_deref()
    }
}

/// Builder for creating [`NginxContainer`] instances with advanced configuration.
///
/// Use [`NginxContainer::builder()`] to create a new builder.
pub struct NginxContainerBuilder {
    network: Option<String>,
    health_path: String,
    entrypoint: Option<String>,
    cmd: Option<Vec<String>>,
    wait_for: Option<WaitFor>,
    expose_port: Option<u16>,
}

impl NginxContainerBuilder {
    /// Attach the container to a Docker network.
    ///
    /// When a network is set, the container's bridge IP address is resolved
    /// after startup and available via [`NginxContainer::bridge_ip()`].
    pub fn network(mut self, network: &str) -> Self {
        self.network = Some(network.to_string());
        self
    }

    /// Set the HTTP health-check path (default: `"/"`).
    pub fn health_path(mut self, path: &str) -> Self {
        self.health_path = path.to_string();
        self
    }

    /// Override the container entrypoint (e.g. `"sh"`).
    pub fn entrypoint(mut self, entrypoint: &str) -> Self {
        self.entrypoint = Some(entrypoint.to_string());
        self
    }

    /// Override the container command (e.g. `vec!["-c", "script"]`).
    pub fn cmd(mut self, cmd: Vec<&str>) -> Self {
        self.cmd = Some(cmd.into_iter().map(|s| s.to_string()).collect());
        self
    }

    /// Override the wait-for strategy.
    ///
    /// By default the builder waits for an HTTP 200 on [`Self::health_path`].
    /// Use this to wait for a log message instead (e.g. for SSL containers
    /// that don't expose an HTTP port).
    pub fn wait_for(mut self, wait_for: WaitFor) -> Self {
        self.wait_for = Some(wait_for);
        self
    }

    /// Set the port to expose (default: `80`). Pass `None` to skip port
    /// exposure (e.g. for SSL containers that are tested via `exec_shell`).
    pub fn expose_port(mut self, port: Option<u16>) -> Self {
        self.expose_port = port;
        self
    }

    /// Build and start the nginx container with the given configuration.
    pub async fn start(self, config: impl Into<Vec<u8>>) -> NginxContainer {
        let img = nginx_image_config();
        let mut generic = GenericImage::new(&img.image_name, &img.image_tag);
        if let Some(ref entrypoint) = self.entrypoint {
            generic = generic.with_entrypoint(entrypoint);
        }

        let wait = self.wait_for.unwrap_or_else(|| {
            WaitFor::http(
                HttpWaitStrategy::new(&self.health_path).with_expected_status_code(200u16),
            )
        });

        if let Some(port) = self.expose_port {
            generic = generic.with_exposed_port(port.tcp());
        }

        let mut image = generic
            .with_wait_for(wait)
            .with_copy_to(&img.conf_path, config.into())
            .with_startup_timeout(Duration::from_secs(120));

        if let Some(ref cmd) = self.cmd {
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            image = image.with_cmd(cmd_refs);
        }
        if let Some(ref network) = self.network {
            image = image.with_network(network);
        }

        let container = image.start().await.unwrap_or_else(|e| {
            panic!(
                "Failed to start {} container (is Docker running?): {}",
                img.full_image(),
                e
            )
        });

        let host = container.get_host().await.unwrap().to_string();
        let port = if let Some(p) = self.expose_port {
            container.get_host_port_ipv4(p).await.unwrap()
        } else {
            0
        };

        let bridge_ip = if self.network.is_some() {
            Some(
                container
                    .get_bridge_ip_address()
                    .await
                    .expect("Failed to get bridge IP address")
                    .to_string(),
            )
        } else {
            None
        };

        NginxContainer {
            container,
            host,
            port,
            tls: self.expose_port == Some(443),
            bridge_ip,
        }
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
/// ```rust,no_run
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
    let img = nginx_image_config();

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
            &format!("{}:{}:ro", tmpfile.display(), img.conf_path),
            &img.full_image(),
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
