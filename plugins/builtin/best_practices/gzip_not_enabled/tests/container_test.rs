//! Container-based integration tests for the gzip-not-enabled rule.
//!
//! Verifies that gzip compression actually affects response encoding
//! in a real nginx instance.
//!
//! Run with:
//!   cargo test -p gzip-not-enabled-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p gzip-not-enabled-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// Build a reqwest client with automatic decompression disabled,
/// so we can inspect the raw Content-Encoding header.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder().no_gzip().build().unwrap()
}

#[tokio::test]
#[ignore]
async fn gzip_on_compresses_text_response() {
    let nginx = NginxContainer::start(br#"
events {
    worker_connections 1024;
}
http {
    gzip on;
    gzip_types text/plain text/html application/json;
    gzip_min_length 20;

    server {
        listen 80;
        default_type text/plain;

        location / {
            return 200 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA';
        }
    }
}
"#).await;

    let resp = http_client()
        .get(nginx.url("/"))
        .header("Accept-Encoding", "gzip")
        .send()
        .await
        .unwrap();

    let content_encoding = resp
        .headers()
        .get("content-encoding")
        .map(|v| v.to_str().unwrap().to_string());

    assert_eq!(
        content_encoding.as_deref(),
        Some("gzip"),
        "Expected Content-Encoding: gzip when gzip is enabled"
    );
}

#[tokio::test]
#[ignore]
async fn gzip_off_does_not_compress() {
    let nginx = NginxContainer::start(br#"
events {
    worker_connections 1024;
}
http {
    gzip off;

    server {
        listen 80;
        default_type text/plain;

        location / {
            return 200 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA';
        }
    }
}
"#).await;

    let resp = http_client()
        .get(nginx.url("/"))
        .header("Accept-Encoding", "gzip")
        .send()
        .await
        .unwrap();

    let content_encoding = resp
        .headers()
        .get("content-encoding")
        .map(|v| v.to_str().unwrap().to_string());

    assert!(
        content_encoding.is_none(),
        "Expected no Content-Encoding header when gzip is off, got: {:?}",
        content_encoding
    );
}

#[tokio::test]
#[ignore]
async fn gzip_default_does_not_compress() {
    let nginx = NginxContainer::start(br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;
        default_type text/plain;

        location / {
            return 200 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA';
        }
    }
}
"#).await;

    let resp = http_client()
        .get(nginx.url("/"))
        .header("Accept-Encoding", "gzip")
        .send()
        .await
        .unwrap();

    let content_encoding = resp
        .headers()
        .get("content-encoding")
        .map(|v| v.to_str().unwrap().to_string());

    assert!(
        content_encoding.is_none(),
        "Expected no Content-Encoding when gzip is not configured (defaults to off), got: {:?}",
        content_encoding
    );
}

#[tokio::test]
#[ignore]
async fn gzip_does_not_compress_without_accept_encoding() {
    let nginx = NginxContainer::start(br#"
events {
    worker_connections 1024;
}
http {
    gzip on;
    gzip_types text/plain;
    gzip_min_length 20;

    server {
        listen 80;
        default_type text/plain;

        location / {
            return 200 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA';
        }
    }
}
"#).await;

    let resp = http_client()
        .get(nginx.url("/"))
        // No Accept-Encoding header
        .send()
        .await
        .unwrap();

    let content_encoding = resp
        .headers()
        .get("content-encoding")
        .map(|v| v.to_str().unwrap().to_string());

    assert!(
        content_encoding.is_none(),
        "Expected no compression without Accept-Encoding header, got: {:?}",
        content_encoding
    );
}
