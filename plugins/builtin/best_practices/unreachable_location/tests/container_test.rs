//! Container-based integration tests for the unreachable-location rule.
//!
//! Verifies that nginx's location matching priority causes certain location
//! blocks to be effectively unreachable, confirming the lint rule's warnings.
//!
//! Key findings verified here:
//! - Duplicate prefix/exact locations: nginx rejects the config entirely (`emerg` error)
//! - Duplicate regex locations: nginx accepts but the second one is unreachable
//! - Broad regex before specific: the specific regex never matches
//! - `^~` prefix modifier: prevents regex matching for paths under it
//!
//! Run with:
//!   cargo test -p unreachable-location-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p unreachable-location-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, nginx_config_test, reqwest};

/// Duplicate prefix locations cause nginx to fail with "duplicate location" error.
#[test]
#[ignore]
fn duplicate_prefix_location_rejected_by_nginx() {
    nginx_config_test(
        r#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location /api {
            return 200 'first-api';
        }
        location /api {
            return 200 'second-api';
        }
    }
}
"#,
    )
    .assert_fails_with("duplicate location");
}

/// Duplicate exact locations cause nginx to fail with "duplicate location" error.
#[test]
#[ignore]
fn duplicate_exact_location_rejected_by_nginx() {
    nginx_config_test(
        r#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location = /favicon.ico {
            return 200 'first-favicon';
        }
        location = /favicon.ico {
            return 200 'second-favicon';
        }
    }
}
"#,
    )
    .assert_fails_with("duplicate location");
}

/// Duplicate regex locations: nginx accepts the config but the second one is unreachable.
/// Unlike prefix/exact duplicates, regex duplicates don't cause a startup error.
#[tokio::test]
#[ignore]
async fn duplicate_regex_location_second_unreachable() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location / {
            return 200 'root';
        }

        location ~ /api {
            return 200 'first-regex-api';
        }
        location ~ /api {
            return 200 'second-regex-api';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/api/test")).await.unwrap();
    let body = resp.text().await.unwrap();

    // nginx uses the first matching regex in config order
    assert_eq!(
        body, "first-regex-api",
        "Expected first regex to match, making the duplicate second regex unreachable"
    );
}

/// Regex location order: the first matching regex wins.
/// A broad regex before a specific one makes the specific one unreachable.
#[tokio::test]
#[ignore]
async fn broad_regex_before_specific_shadows() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location / {
            return 200 'root';
        }

        # Broad regex matches first
        location ~ /api {
            return 200 'broad-api';
        }
        # This specific regex is unreachable for /api/v1 paths
        location ~ /api/v1 {
            return 200 'specific-api-v1';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/api/v1/users")).await.unwrap();
    let body = resp.text().await.unwrap();

    // The broad regex ~ /api matches /api/v1/users first, so ~ /api/v1 never fires
    assert_eq!(
        body, "broad-api",
        "Expected broad regex to match first, making the specific regex unreachable"
    );
}

/// When specific regex comes before broad regex, both are reachable.
#[tokio::test]
#[ignore]
async fn specific_regex_before_broad_both_reachable() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location / {
            return 200 'root';
        }

        # Specific regex first - correctly ordered
        location ~ /api/v1 {
            return 200 'specific-api-v1';
        }
        location ~ /api {
            return 200 'broad-api';
        }
    }
}
"#,
    )
    .await;

    // /api/v1 path should match the specific regex
    let resp = reqwest::get(nginx.url("/api/v1/users")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "specific-api-v1",
        "Expected specific regex to match /api/v1 paths"
    );

    // /api/other should match the broad regex
    let resp = reqwest::get(nginx.url("/api/other")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "broad-api",
        "Expected broad regex to match non-v1 /api paths"
    );
}

/// ^~ prefix modifier prevents regex matching for paths under it.
#[tokio::test]
#[ignore]
async fn prefix_no_regex_shadows_regex() {
    let nginx = NginxContainer::start_with_health_path(
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

        # ^~ stops regex search for /images/ paths
        location ^~ /images/ {
            return 200 'prefix-images';
        }

        # This regex will NOT match /images/photo.jpg because ^~ takes priority
        location ~* \.(jpg|png|gif)$ {
            return 200 'regex-image-ext';
        }
    }
}
"#,
        "/healthz",
    )
    .await;

    // /images/photo.jpg matches ^~ /images/ prefix, so regex is not checked
    let resp = reqwest::get(nginx.url("/images/photo.jpg")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "prefix-images",
        "Expected ^~ prefix to prevent regex match for paths under /images/"
    );

    // /other/photo.jpg should match the regex (no ^~ applies)
    let resp = reqwest::get(nginx.url("/other/photo.jpg")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "regex-image-ext",
        "Expected regex to match .jpg outside of ^~ prefix path"
    );
}

/// ^~ /static (without trailing slash) also shadows file extension regex.
#[tokio::test]
#[ignore]
async fn prefix_no_regex_without_trailing_slash_shadows_regex() {
    let nginx = NginxContainer::start_with_health_path(
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

        # ^~ without trailing slash still matches /static/*
        location ^~ /static {
            return 200 'prefix-static';
        }

        # This regex will NOT match /static/style.css because ^~ takes priority
        location ~* \.(css|js)$ {
            return 200 'regex-css-js';
        }
    }
}
"#,
        "/healthz",
    )
    .await;

    // /static/style.css matches ^~ /static prefix, so regex is not checked
    let resp = reqwest::get(nginx.url("/static/style.css")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "prefix-static",
        "Expected ^~ /static (no trailing slash) to prevent regex match"
    );

    // /other/style.css should match the regex (no ^~ applies)
    let resp = reqwest::get(nginx.url("/other/style.css")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "regex-css-js",
        "Expected regex to match .css outside of ^~ prefix path"
    );
}

/// ^~ / shadows ALL regex locations since every URI starts with /.
#[tokio::test]
#[ignore]
async fn prefix_no_regex_root_shadows_all_regex() {
    let nginx = NginxContainer::start_with_health_path(
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

        # ^~ / matches every URI, preventing all regex evaluation
        location ^~ / {
            return 200 'prefix-root';
        }

        location ~ /api {
            return 200 'regex-api';
        }
    }
}
"#,
        "/healthz",
    )
    .await;

    // /api/test should match ^~ / (not the regex) because ^~ stops regex search
    let resp = reqwest::get(nginx.url("/api/test")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "prefix-root",
        "Expected ^~ / to shadow all regex locations"
    );
}

/// ^~ /images/photos/ (longer prefix) shadows ~ /images/ (shorter regex literal).
#[tokio::test]
#[ignore]
async fn prefix_no_regex_longer_shadows_shorter_regex() {
    let nginx = NginxContainer::start_with_health_path(
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

        location ^~ /images/photos/ {
            return 200 'prefix-photos';
        }

        # This regex matches /images/ but for /images/photos/* the ^~ wins
        location ~ /images/ {
            return 200 'regex-images';
        }
    }
}
"#,
        "/healthz",
    )
    .await;

    // /images/photos/vacation.jpg matches ^~ /images/photos/, regex not checked
    let resp = reqwest::get(nginx.url("/images/photos/vacation.jpg"))
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "prefix-photos",
        "Expected ^~ /images/photos/ to shadow regex for paths under it"
    );

    // /images/icons/logo.png should match the regex (^~ doesn't apply)
    let resp = reqwest::get(nginx.url("/images/icons/logo.png"))
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "regex-images",
        "Expected regex to match /images/ paths not under ^~ prefix"
    );
}
