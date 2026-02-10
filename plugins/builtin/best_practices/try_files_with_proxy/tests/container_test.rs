//! Container-based integration tests for the try-files-with-proxy rule.
//!
//! Verifies that when `try_files` and `proxy_pass` are in the same location
//! block, `proxy_pass` becomes the content handler and `try_files` only
//! rewrites the URI - files are never served directly from disk.
//!
//! Each test uses two server blocks in the same nginx:
//! - Port 8080 (backend): echoes the request URI via `return 200 $request_uri`
//! - Port 80 (frontend): serves static files and/or proxies to the backend
//!
//! The default nginx container has `/usr/share/nginx/html/index.html` and
//! `/usr/share/nginx/html/50x.html` which are used as known static files.
//!
//! Run with:
//!   cargo test -p try-files-with-proxy-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p try-files-with-proxy-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// When try_files and proxy_pass are in the same block, proxy_pass handles
/// all requests. try_files only rewrites URIs - files are never served locally.
#[tokio::test]
#[ignore]
async fn try_files_with_proxy_pass_never_serves_files() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 8080;
        location / {
            return 200 "uri=$request_uri";
        }
    }

    server {
        listen 80;
        root /usr/share/nginx/html;

        location / {
            try_files $uri $uri/ /index.html;
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
    )
    .await;

    // Even for existing file (50x.html), proxy_pass handles the request
    let resp = reqwest::get(nginx.url("/50x.html")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "uri=/50x.html",
        "Expected proxy_pass to handle request even for existing file"
    );

    // For non-existent file, try_files rewrites to /index.html, then proxy_pass sends it
    let resp = reqwest::get(nginx.url("/nonexistent")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "uri=/index.html",
        "Expected try_files to rewrite URI to fallback, then proxy_pass forwards it"
    );
}

/// Named location fallback (@backend) correctly serves static files locally
/// and only proxies when no file is found.
#[tokio::test]
#[ignore]
async fn named_location_fallback_serves_files_and_proxies() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 8080;
        location / {
            return 200 'from-proxy';
        }
    }

    server {
        listen 80;
        root /usr/share/nginx/html;

        location / {
            try_files $uri @backend;
        }

        location @backend {
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
    )
    .await;

    // Existing file: served directly from disk (not proxied)
    let resp = reqwest::get(nginx.url("/index.html")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("Welcome to nginx"),
        "Expected existing file to be served locally, not proxied"
    );

    // Non-existent file: falls through to @backend proxy
    let resp = reqwest::get(nginx.url("/nonexistent")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "from-proxy",
        "Expected named location fallback to proxy non-existent files"
    );
}
