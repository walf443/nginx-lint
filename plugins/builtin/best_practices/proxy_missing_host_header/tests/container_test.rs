//! Container-based integration tests for the proxy-missing-host-header rule.
//!
//! Verifies that without `proxy_set_header Host`, the backend receives
//! `$proxy_host` (the upstream address) instead of the client's original hostname.
//!
//! Each test uses two server blocks in the same nginx:
//! - Port 8080 (backend): echoes the Host header it received via `return 200 $http_host`
//! - Port 80 (frontend): proxies requests to the backend
//!
//! Run with:
//!   cargo test -p proxy-missing-host-header-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p proxy-missing-host-header-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// Without proxy_set_header Host, the backend receives $proxy_host (upstream address).
#[tokio::test]
#[ignore]
async fn missing_host_header_sends_proxy_host() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 8080;
        location / {
            return 200 $http_host;
        }
    }

    server {
        listen 80;
        server_name _;
        location / {
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(nginx.url("/"))
        .header("Host", "example.com")
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();

    // Without proxy_set_header Host, backend receives the proxy_pass target address
    assert_eq!(
        body, "127.0.0.1:8080",
        "Expected backend to receive $proxy_host (127.0.0.1:8080), got: {body}"
    );
}

/// With proxy_set_header Host $host, the backend receives the original client hostname.
#[tokio::test]
#[ignore]
async fn with_host_header_forwards_original_host() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 8080;
        location / {
            return 200 $http_host;
        }
    }

    server {
        listen 80;
        server_name _;
        location / {
            proxy_pass http://127.0.0.1:8080;
            proxy_set_header Host $host;
        }
    }
}
"#,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(nginx.url("/"))
        .header("Host", "example.com")
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();

    assert_eq!(
        body, "example.com",
        "Expected backend to receive the original client Host header"
    );
}

/// With proxy_set_header Host at server level, all locations inherit it.
#[tokio::test]
#[ignore]
async fn server_level_host_header_inherited_by_locations() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 8080;
        location / {
            return 200 $http_host;
        }
    }

    server {
        listen 80;
        server_name _;
        proxy_set_header Host $host;

        location /api {
            proxy_pass http://127.0.0.1:8080;
        }

        location / {
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
    )
    .await;

    let client = reqwest::Client::new();

    // Both locations should inherit the server-level Host header
    let resp = client
        .get(nginx.url("/api"))
        .header("Host", "api.example.com")
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "api.example.com",
        "Expected /api location to inherit server-level Host header"
    );

    let resp = client
        .get(nginx.url("/"))
        .header("Host", "www.example.com")
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "www.example.com",
        "Expected / location to inherit server-level Host header"
    );
}
