//! Container-based integration tests for the add-header-inheritance rule.
//!
//! Verifies that `add_header` directives in child blocks completely override
//! (not inherit) parent block headers in a real nginx instance.
//!
//! Run with:
//!   cargo test -p add-header-inheritance-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p add-header-inheritance-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// Helper to get a header value from a response.
fn get_header(resp: &reqwest::Response, name: &str) -> Option<String> {
    resp.headers()
        .get(name)
        .map(|v| v.to_str().unwrap().to_string())
}

#[tokio::test]
#[ignore]
async fn parent_headers_inherited_when_no_child_add_header() {
    // When location has NO add_header, parent headers ARE inherited.
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;
        add_header X-Server-Header "from-server";

        location / {
            return 200 'OK';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        get_header(&resp, "x-server-header").as_deref(),
        Some("from-server"),
        "Parent header should be inherited when location has no add_header"
    );
}

#[tokio::test]
#[ignore]
async fn parent_headers_lost_when_child_has_add_header() {
    // When location has its own add_header, parent headers are NOT inherited.
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;
        add_header X-Server-Header "from-server";

        location / {
            add_header X-Location-Header "from-location";
            return 200 'OK';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    assert_eq!(resp.status(), 200);

    assert_eq!(
        get_header(&resp, "x-location-header").as_deref(),
        Some("from-location"),
        "Child header should be present"
    );
    assert_eq!(
        get_header(&resp, "x-server-header"),
        None,
        "Parent header should be LOST when child block has its own add_header"
    );
}

#[tokio::test]
#[ignore]
async fn parent_headers_preserved_when_explicitly_repeated() {
    // When location explicitly repeats parent headers, both are present.
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;
        add_header X-Server-Header "from-server";

        location / {
            add_header X-Server-Header "from-server";
            add_header X-Location-Header "from-location";
            return 200 'OK';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    assert_eq!(resp.status(), 200);

    assert_eq!(
        get_header(&resp, "x-server-header").as_deref(),
        Some("from-server"),
        "Explicitly repeated parent header should be present"
    );
    assert_eq!(
        get_header(&resp, "x-location-header").as_deref(),
        Some("from-location"),
        "Child header should also be present"
    );
}

#[tokio::test]
#[ignore]
async fn multiple_parent_headers_all_lost() {
    // All parent headers are lost, not just some.
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;
        add_header X-Frame-Options "DENY";
        add_header X-Content-Type-Options "nosniff";

        location / {
            add_header X-Custom "value";
            return 200 'OK';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    assert_eq!(resp.status(), 200);

    assert_eq!(
        get_header(&resp, "x-custom").as_deref(),
        Some("value"),
        "Child header should be present"
    );
    assert_eq!(
        get_header(&resp, "x-frame-options"),
        None,
        "X-Frame-Options from parent should be lost"
    );
    assert_eq!(
        get_header(&resp, "x-content-type-options"),
        None,
        "X-Content-Type-Options from parent should be lost"
    );
}
