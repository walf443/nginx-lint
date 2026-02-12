//! Container-based integration tests for the autoindex-enabled rule.
//!
//! Verifies that `autoindex on` actually exposes directory listings
//! in a real nginx instance.
//!
//! Run with:
//!   cargo test -p autoindex-enabled-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p autoindex-enabled-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

#[tokio::test]
#[ignore]
async fn autoindex_on_shows_directory_listing() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(
            br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location /healthz {
            return 200 'OK';
        }

        location /files/ {
            alias /etc/nginx/;
            autoindex on;
        }
    }
}
"#,
        )
        .await;

    let resp = reqwest::get(nginx.url("/files/")).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(
        body.contains("Index of"),
        "Expected directory listing with 'Index of', got:\n{}",
        body
    );
}

#[tokio::test]
#[ignore]
async fn autoindex_off_returns_403_for_directory() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(
            br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location /healthz {
            return 200 'OK';
        }

        location /files/ {
            alias /etc/nginx/;
            autoindex off;
        }
    }
}
"#,
        )
        .await;

    let resp = reqwest::get(nginx.url("/files/")).await.unwrap();
    assert_eq!(
        resp.status(),
        403,
        "Expected 403 Forbidden when autoindex is off"
    );
}

#[tokio::test]
#[ignore]
async fn autoindex_default_returns_403_for_directory() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(
            br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location /healthz {
            return 200 'OK';
        }

        location /files/ {
            alias /etc/nginx/;
        }
    }
}
"#,
        )
        .await;

    let resp = reqwest::get(nginx.url("/files/")).await.unwrap();
    assert_eq!(
        resp.status(),
        403,
        "Expected 403 Forbidden when autoindex is not set (defaults to off)"
    );
}
