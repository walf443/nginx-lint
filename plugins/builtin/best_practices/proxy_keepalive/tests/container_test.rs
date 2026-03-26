//! Container-based integration tests for the proxy-keepalive rule.
//!
//! Verifies that without `proxy_set_header Connection ""`, the upstream
//! receives `Connection: close` which prevents keepalive connection reuse.
//!
//! Each test uses an upstream + two server blocks in the same nginx:
//! - Port 8080 (backend): echoes the Connection header via `return 200 $http_connection`
//! - Port 80 (frontend): proxies to the backend via the upstream
//!
//! Run with:
//!   cargo test -p proxy-keepalive-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p proxy-keepalive-plugin --test container_test -- --ignored
//!
//! ## nginx 1.29.7+ compatibility
//!
//! These tests are skipped on nginx >= 1.29 because the default proxy behavior changed:
//! - `proxy_http_version` now defaults to `1.1` (was `1.0`)
//! - Keep-alive to upstreams is enabled by default (`proxy_set_header Connection ""` is no longer needed)
//!
//! See: <https://blog.nginx.org/blog/keep-alive-to-upstreams-is-now-default-in-nginx-1-29-7>

use nginx_lint_plugin::container_testing::{NginxContainer, nginx_version_at_least, reqwest};

/// Returns true if the nginx version is >= 1.29, where keepalive defaults changed.
fn should_skip() -> bool {
    nginx_version_at_least(1, 29)
}

/// Without proxy_set_header Connection, the upstream receives "close",
/// preventing keepalive even with proxy_http_version 1.1.
///
/// Skipped on nginx >= 1.29: keepalive to upstreams is now default,
/// so Connection is no longer set to "close" automatically.
#[tokio::test]
#[ignore]
async fn missing_connection_header_sends_close() {
    if should_skip() {
        eprintln!("Skipping: nginx >= 1.29 defaults to keepalive for upstreams");
        return;
    }
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    upstream backend {
        server 127.0.0.1:8080;
        keepalive 32;
    }

    server {
        listen 8080;
        location / {
            return 200 "connection=$http_connection";
        }
    }

    server {
        listen 80;
        location / {
            proxy_http_version 1.1;
            proxy_pass http://backend;
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    let body = resp.text().await.unwrap();

    // Without clearing Connection, upstream receives "close"
    assert_eq!(
        body, "connection=close",
        "Expected upstream to receive Connection: close without proxy_set_header"
    );
}

/// With proxy_set_header Connection "", the upstream receives an empty
/// Connection header, enabling keepalive connection reuse.
///
/// This test works on all nginx versions because the explicit
/// `proxy_set_header Connection ""` overrides any default behavior.
#[tokio::test]
#[ignore]
async fn cleared_connection_header_enables_keepalive() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    upstream backend {
        server 127.0.0.1:8080;
        keepalive 32;
    }

    server {
        listen 8080;
        location / {
            return 200 "connection=$http_connection";
        }
    }

    server {
        listen 80;
        location / {
            proxy_http_version 1.1;
            proxy_set_header Connection "";
            proxy_pass http://backend;
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    let body = resp.text().await.unwrap();

    // With Connection cleared, upstream receives empty value
    assert_eq!(
        body, "connection=",
        "Expected upstream to receive empty Connection header for keepalive"
    );
}

/// Default proxy_http_version (1.0) also sends Connection: close to upstream.
/// This is expected behavior for HTTP/1.0 — no keepalive needed.
///
/// Skipped on nginx >= 1.29: proxy_http_version now defaults to 1.1
/// with keepalive enabled, so Connection: close is no longer sent.
#[tokio::test]
#[ignore]
async fn default_http_10_sends_close() {
    if should_skip() {
        eprintln!("Skipping: nginx >= 1.29 defaults proxy_http_version to 1.1");
        return;
    }
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    upstream backend {
        server 127.0.0.1:8080;
        keepalive 32;
    }

    server {
        listen 8080;
        location / {
            return 200 "connection=$http_connection";
        }
    }

    server {
        listen 80;
        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    let body = resp.text().await.unwrap();

    // Default HTTP/1.0 sends Connection: close (expected, no lint warning)
    assert_eq!(
        body, "connection=close",
        "Expected upstream to receive Connection: close with default HTTP/1.0"
    );
}
