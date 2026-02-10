//! Container-based integration tests for the if-is-evil-in-location rule.
//!
//! Demonstrates that unsafe directives inside `if` blocks in location context
//! cause unexpected behavior because nginx creates an implicit "pseudo-location"
//! for the `if` block, which does not inherit directives from the parent location.
//!
//! Run with:
//!   cargo test -p if-is-evil-in-location-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p if-is-evil-in-location-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// `add_header` inside `if` causes headers defined outside `if` to be lost.
///
/// This is the classic "if is evil" problem: when the `if` condition matches
/// and the `if` block contains `add_header`, nginx enters the pseudo-location
/// created by `if`, which does NOT inherit `add_header` from the parent location.
#[tokio::test]
#[ignore]
async fn add_header_in_if_drops_outside_headers() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location / {
            add_header X-Outside "always-present" always;

            if ($arg_flag = "1") {
                # UNSAFE: add_header in if causes X-Outside to disappear
                add_header X-Inside "conditional" always;
                return 200 "flag=1";
            }
            return 200 "no-flag";
        }
    }
}
"#,
    )
    .await;

    // Without flag: X-Outside header is present
    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    assert_eq!(
        resp.headers().get("X-Outside").map(|v| v.to_str().unwrap()),
        Some("always-present"),
        "X-Outside should be present when if does not match"
    );

    // With flag: if matches, X-Outside header DISAPPEARS (the evil behavior)
    let resp = reqwest::get(nginx.url("/?flag=1")).await.unwrap();
    assert!(
        resp.headers().get("X-Outside").is_none(),
        "X-Outside should disappear when if matches and contains add_header (if is evil)"
    );
    assert_eq!(
        resp.headers().get("X-Inside").map(|v| v.to_str().unwrap()),
        Some("conditional"),
        "X-Inside should be present when if matches"
    );
}

/// Safe: `return` inside `if` does NOT cause outside headers to be lost.
///
/// When the `if` block only contains safe directives (return, set, rewrite last/break),
/// the parent location's directives are preserved.
#[tokio::test]
#[ignore]
async fn return_in_if_preserves_outside_headers() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location / {
            add_header X-Outside "always-present" always;

            if ($arg_flag = "1") {
                # SAFE: only return inside if
                return 200 "flag=1";
            }
            return 200 "no-flag";
        }
    }
}
"#,
    )
    .await;

    // Without flag: X-Outside is present
    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    assert_eq!(
        resp.headers().get("X-Outside").map(|v| v.to_str().unwrap()),
        Some("always-present"),
    );

    // With flag: return is safe, so X-Outside is STILL present
    let resp = reqwest::get(nginx.url("/?flag=1")).await.unwrap();
    assert_eq!(
        resp.headers().get("X-Outside").map(|v| v.to_str().unwrap()),
        Some("always-present"),
        "X-Outside should be preserved when if only contains safe return"
    );
}

/// Safe: `set` inside `if` does NOT cause issues.
#[tokio::test]
#[ignore]
async fn set_in_if_is_safe() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location / {
            set $value "default";
            if ($arg_override = "1") {
                set $value "overridden";
            }
            return 200 "value=$value";
        }
    }
}
"#,
    )
    .await;

    // Without override: default value
    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(body, "value=default");

    // With override: set inside if works correctly
    let resp = reqwest::get(nginx.url("/?override=1")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(body, "value=overridden");
}
