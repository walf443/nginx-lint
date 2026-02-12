//! Container-based integration tests for the root-in-location rule.
//!
//! Demonstrates the pitfall of defining `root` inside location blocks:
//! locations without `root` use nginx's compile-time default path, which
//! may not contain the expected files.
//!
//! The default nginx container has `{html_root}/index.html` and
//! `{html_root}/50x.html` as known static files.
//!
//! Run with:
//!   cargo test -p root-in-location-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p root-in-location-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, nginx_html_root, reqwest};

/// When `root` is at server level, all locations inherit it and serve files.
#[tokio::test]
#[ignore]
async fn root_at_server_level_all_locations_serve_files() {
    let html_root = nginx_html_root();
    let config = format!(
        r#"
events {{
    worker_connections 1024;
}}
http {{
    server {{
        listen 80;
        root {html_root};

        location / {{
            try_files $uri $uri/ =404;
        }}

        location /other/ {{
            try_files $uri =404;
        }}
    }}
}}
"#
    );
    let nginx = NginxContainer::start(config).await;

    // / location serves files (root inherited from server)
    let resp = reqwest::get(nginx.url("/index.html")).await.unwrap();
    assert_eq!(resp.status(), 200, "/ location should serve index.html");

    // /other/ location also serves files (root inherited from server)
    let resp = reqwest::get(nginx.url("/50x.html")).await.unwrap();
    assert_eq!(resp.status(), 200, "/ location should serve 50x.html");
}

/// When `root` is only in one location, other locations use the default root
/// path and may fail to serve expected files.
#[tokio::test]
#[ignore]
async fn root_only_in_one_location_other_locations_fail() {
    let html_root = nginx_html_root();
    let config = format!(
        r#"
events {{
    worker_connections 1024;
}}
http {{
    server {{
        listen 80;

        location / {{
            root {html_root};
            try_files $uri $uri/ =404;
        }}

        # This location has no root - uses nginx default (html relative to prefix)
        location /noroot/ {{
            try_files $uri =404;
        }}
    }}
}}
"#
    );
    let nginx = NginxContainer::start(config).await;

    // / location has root set - serves files
    let resp = reqwest::get(nginx.url("/index.html")).await.unwrap();
    assert_eq!(resp.status(), 200, "Location with root should serve files");

    // /noroot/ location has no root - can't find files
    let resp = reqwest::get(nginx.url("/noroot/index.html")).await.unwrap();
    assert_eq!(
        resp.status(),
        404,
        "Location without root should fail to find files"
    );
}

/// When all locations define their own `root`, it works but is error-prone.
/// Adding a new location without `root` is easy to forget.
#[tokio::test]
#[ignore]
async fn root_in_every_location_works_but_fragile() {
    let html_root = nginx_html_root();
    let config = format!(
        r#"
events {{
    worker_connections 1024;
}}
http {{
    server {{
        listen 80;

        location / {{
            root {html_root};
            try_files $uri $uri/ =404;
        }}

        location /alt/ {{
            root {html_root};
            try_files $uri =404;
        }}

        # Simulates a newly added location where root was forgotten
        location /new/ {{
            try_files $uri =404;
        }}
    }}
}}
"#
    );
    let nginx = NginxContainer::start(config).await;

    // Both locations with root work
    let resp = reqwest::get(nginx.url("/index.html")).await.unwrap();
    assert_eq!(resp.status(), 200, "/ with root should serve files");

    // /new/ without root fails - demonstrates the fragility
    let resp = reqwest::get(nginx.url("/new/index.html")).await.unwrap();
    assert_eq!(
        resp.status(),
        404,
        "Newly added location without root fails to serve files"
    );
}
