//! Container-based integration tests for the map-missing-default rule.
//!
//! Verifies that when a `map` block has no `default` entry, unmatched values
//! silently resolve to an empty string.
//!
//! Each test uses a `map` block that maps `$arg_key` to `$mapped_value`,
//! then returns the mapped value in the response body.
//!
//! Run with:
//!   cargo test -p map-missing-default-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p map-missing-default-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// Without a `default` entry, unmatched values resolve to an empty string.
#[tokio::test]
#[ignore]
async fn map_without_default_resolves_to_empty_string() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    map $arg_key $mapped_value {
        foo  matched_foo;
        bar  matched_bar;
    }

    server {
        listen 80;
        location / {
            return 200 "value=[$mapped_value]";
        }
    }
}
"#,
    )
    .await;

    // Matched value: should resolve correctly
    let resp = reqwest::get(nginx.url("/?key=foo")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "value=[matched_foo]",
        "Expected matched key to resolve to its mapped value"
    );

    // Unmatched value: resolves to empty string (no default)
    let resp = reqwest::get(nginx.url("/?key=unknown")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "value=[]",
        "Expected unmatched key to resolve to empty string when no default is set"
    );

    // Missing key entirely: also resolves to empty string
    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "value=[]",
        "Expected missing key to resolve to empty string when no default is set"
    );
}

/// With a `default` entry, unmatched values resolve to the default value.
#[tokio::test]
#[ignore]
async fn map_with_default_resolves_to_fallback() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    map $arg_key $mapped_value {
        default  fallback_value;
        foo      matched_foo;
        bar      matched_bar;
    }

    server {
        listen 80;
        location / {
            return 200 "value=[$mapped_value]";
        }
    }
}
"#,
    )
    .await;

    // Matched value: should resolve correctly
    let resp = reqwest::get(nginx.url("/?key=foo")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "value=[matched_foo]",
        "Expected matched key to resolve to its mapped value"
    );

    // Unmatched value: resolves to default
    let resp = reqwest::get(nginx.url("/?key=unknown")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "value=[fallback_value]",
        "Expected unmatched key to resolve to default value"
    );

    // Missing key entirely: also resolves to default
    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "value=[fallback_value]",
        "Expected missing key to resolve to default value"
    );
}

/// Without default, the empty string result can silently break add_header
/// or other directives that use the mapped variable.
#[tokio::test]
#[ignore]
async fn map_without_default_produces_empty_in_header() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    map $arg_key $mapped_value {
        foo  matched_foo;
        bar  matched_bar;
    }

    server {
        listen 80;
        location / {
            # add_header with empty value is silently omitted by nginx
            add_header X-Mapped $mapped_value always;
            return 200 "ok";
        }
    }
}
"#,
    )
    .await;

    // Matched value: header is present
    let resp = reqwest::get(nginx.url("/?key=foo")).await.unwrap();
    assert_eq!(
        resp.headers().get("X-Mapped").map(|v| v.to_str().unwrap()),
        Some("matched_foo"),
        "Expected X-Mapped header to be set for matched key"
    );

    // Unmatched value: header has empty value (or may be omitted)
    let resp = reqwest::get(nginx.url("/?key=unknown")).await.unwrap();
    let header_value = resp
        .headers()
        .get("X-Mapped")
        .map(|v| v.to_str().unwrap().to_string());
    // nginx may include the header with empty value or omit it entirely
    assert!(
        header_value.is_none() || header_value.as_deref() == Some(""),
        "Expected X-Mapped header to be empty or missing for unmatched key, got: {:?}",
        header_value
    );
}
