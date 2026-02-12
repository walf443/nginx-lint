//! Container-based integration tests for the server-tokens-enabled rule.
//!
//! Verifies that `server_tokens on` actually exposes version information
//! in a real nginx instance.
//!
//! Run with:
//!   cargo test -p server-tokens-enabled-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p server-tokens-enabled-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, nginx_server_name, reqwest};

// ============================================================================
// Server header version exposure
// ============================================================================

#[tokio::test]
#[ignore]
async fn server_tokens_on_exposes_version_in_header() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server_tokens on;
    server {
        listen 80;
        location / {
            return 200 'OK';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    let server_header = resp.headers().get("server").unwrap().to_str().unwrap();

    assert!(
        server_header.contains('/'),
        "Expected Server header to contain version (e.g., '{}/x.y.z'), got: '{}'",
        nginx_server_name(),
        server_header
    );
}

#[tokio::test]
#[ignore]
async fn server_tokens_off_hides_version_in_header() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server_tokens off;
    server {
        listen 80;
        location / {
            return 200 'OK';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    let server_header = resp.headers().get("server").unwrap().to_str().unwrap();

    let expected = nginx_server_name();
    assert_eq!(
        server_header, expected,
        "Expected Server header to be exactly '{expected}' (no version)"
    );
}

// ============================================================================
// Error page version exposure
// ============================================================================

#[tokio::test]
#[ignore]
async fn server_tokens_on_exposes_version_in_error_page() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server_tokens on;
    server {
        listen 80;
        location = / {
            return 200 'OK';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/nonexistent")).await.unwrap();
    assert_eq!(resp.status(), 404);

    let body = resp.text().await.unwrap();
    let name = nginx_server_name();
    assert!(
        body.contains(&format!("{name}/")),
        "Expected error page to contain version info ('{name}/x.y.z'), got:\n{}",
        body
    );
}

#[tokio::test]
#[ignore]
async fn server_tokens_off_hides_version_in_error_page() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server_tokens off;
    server {
        listen 80;
        location = / {
            return 200 'OK';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/nonexistent")).await.unwrap();
    assert_eq!(resp.status(), 404);

    let body = resp.text().await.unwrap();
    let name = nginx_server_name();
    assert!(
        !body.contains(&format!("{name}/")),
        "Expected error page NOT to contain version info, got:\n{}",
        body
    );
}

// ============================================================================
// Default behavior
// ============================================================================

#[tokio::test]
#[ignore]
async fn server_tokens_default_exposes_version() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;
        location / {
            return 200 'OK';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    let server_header = resp.headers().get("server").unwrap().to_str().unwrap();

    assert!(
        server_header.contains('/'),
        "Expected default config to expose version (server_tokens defaults to on), got: '{}'",
        server_header
    );
}
