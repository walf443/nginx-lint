//! Container-based integration tests for the proxy-pass-with-uri rule.
//!
//! Verifies that when `proxy_pass` has a URI component, nginx performs URI
//! rewriting - replacing the matched location prefix with the proxy_pass URI.
//!
//! Each test uses two server blocks in the same nginx:
//! - Port 8080 (backend): echoes the request URI via `return 200 $request_uri`
//! - Port 80 (frontend): proxies to the backend with various proxy_pass URIs
//!
//! Run with:
//!   cargo test -p proxy-pass-with-uri-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p proxy-pass-with-uri-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// Trailing slash in proxy_pass strips the location prefix.
/// `location /api/` + `proxy_pass http://backend/` rewrites `/api/foo` to `/foo`.
#[tokio::test]
#[ignore]
async fn trailing_slash_strips_location_prefix() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 8080;
        location / {
            return 200 $request_uri;
        }
    }

    server {
        listen 80;
        location / {
            return 200 'root';
        }
        location /api/ {
            proxy_pass http://127.0.0.1:8080/;
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/api/users")).await.unwrap();
    let body = resp.text().await.unwrap();

    // /api/users -> strip /api/ -> users -> prepend / -> /users
    assert_eq!(
        body, "/users",
        "Expected trailing slash to strip location prefix from URI"
    );
}

/// Without URI in proxy_pass, the original request path is preserved.
/// `location /api/` + `proxy_pass http://backend` keeps `/api/foo` as-is.
#[tokio::test]
#[ignore]
async fn no_uri_preserves_original_path() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 8080;
        location / {
            return 200 $request_uri;
        }
    }

    server {
        listen 80;
        location / {
            return 200 'root';
        }
        location /api/ {
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/api/users")).await.unwrap();
    let body = resp.text().await.unwrap();

    // Without URI, the full original path is sent to the backend
    assert_eq!(
        body, "/api/users",
        "Expected original path to be preserved without URI in proxy_pass"
    );
}

/// Path in proxy_pass replaces the location prefix.
/// `location /web/` + `proxy_pass http://backend/app/` rewrites `/web/foo` to `/app/foo`.
#[tokio::test]
#[ignore]
async fn path_uri_replaces_location_prefix() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 8080;
        location / {
            return 200 $request_uri;
        }
    }

    server {
        listen 80;
        location / {
            return 200 'root';
        }
        location /web/ {
            proxy_pass http://127.0.0.1:8080/app/;
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/web/dashboard")).await.unwrap();
    let body = resp.text().await.unwrap();

    // /web/dashboard -> strip /web/ -> dashboard -> prepend /app/ -> /app/dashboard
    assert_eq!(
        body, "/app/dashboard",
        "Expected proxy_pass path to replace location prefix"
    );
}
