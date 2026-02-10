//! Container-based integration tests for the client-max-body-size-not-set rule.
//!
//! Verifies that client_max_body_size actually limits request body sizes
//! in a real nginx instance.
//!
//! Run with:
//!   cargo test -p client-max-body-size-not-set-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p client-max-body-size-not-set-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{reqwest, NginxContainer};

/// Generate a string of the given size in bytes.
fn make_body(size: usize) -> String {
    "X".repeat(size)
}

#[tokio::test]
#[ignore]
async fn default_rejects_body_over_1mb() {
    let nginx = NginxContainer::start(br#"
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
"#).await;

    let resp = reqwest::Client::new()
        .post(nginx.url("/"))
        .body(make_body(1024 * 1024 + 1))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        413,
        "Expected 413 Request Entity Too Large for body exceeding default 1m limit"
    );
}

#[tokio::test]
#[ignore]
async fn default_accepts_body_under_1mb() {
    let nginx = NginxContainer::start(br#"
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
"#).await;

    let resp = reqwest::Client::new()
        .post(nginx.url("/"))
        .body(make_body(512 * 1024))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "Expected 200 OK for body under default 1m limit"
    );
}

#[tokio::test]
#[ignore]
async fn custom_limit_rejects_body_over_limit() {
    let nginx = NginxContainer::start(br#"
events {
    worker_connections 1024;
}
http {
    client_max_body_size 100;

    server {
        listen 80;

        location / {
            return 200 'OK';
        }
    }
}
"#).await;

    let resp = reqwest::Client::new()
        .post(nginx.url("/"))
        .body(make_body(200))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        413,
        "Expected 413 for body exceeding custom 100-byte limit"
    );
}

#[tokio::test]
#[ignore]
async fn custom_limit_accepts_body_under_limit() {
    let nginx = NginxContainer::start(br#"
events {
    worker_connections 1024;
}
http {
    client_max_body_size 100;

    server {
        listen 80;

        location / {
            return 200 'OK';
        }
    }
}
"#).await;

    let resp = reqwest::Client::new()
        .post(nginx.url("/"))
        .body(make_body(50))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "Expected 200 OK for body under custom 100-byte limit"
    );
}

#[tokio::test]
#[ignore]
async fn zero_disables_limit() {
    let nginx = NginxContainer::start(br#"
events {
    worker_connections 1024;
}
http {
    client_max_body_size 0;

    server {
        listen 80;

        location / {
            return 200 'OK';
        }
    }
}
"#).await;

    let resp = reqwest::Client::new()
        .post(nginx.url("/"))
        .body(make_body(2 * 1024 * 1024))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "Expected 200 OK when client_max_body_size is 0 (disabled)"
    );
}
