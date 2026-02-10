//! Container-based integration tests for the ssl-on-deprecated rule.
//!
//! The `ssl on;` directive was deprecated in nginx 1.15.0 and removed in
//! nginx 1.25.1.
//!
//! - nginx < 1.25.1: `ssl on;` produces a deprecation warning
//! - nginx >= 1.25.1: `ssl on;` is an unknown directive (config error)
//!
//! The default test version (1.27) rejects `ssl on;` entirely.
//!
//! Run with:
//!   cargo test -p ssl-on-deprecated-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.24 cargo test -p ssl-on-deprecated-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::nginx_config_test;

/// `ssl on;` is rejected as an unknown directive in nginx 1.25.1+.
#[test]
#[ignore]
fn ssl_on_rejected_in_modern_nginx() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80;
        ssl on;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    // nginx 1.25.1+ removed the ssl directive entirely
    result.assert_fails_with("unknown directive");
}

/// `ssl off;` is also rejected as an unknown directive in nginx 1.25.1+.
#[test]
#[ignore]
fn ssl_off_also_rejected_in_modern_nginx() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80;
        ssl off;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    // The entire `ssl` directive was removed, not just `ssl on;`
    result.assert_fails_with("unknown directive");
}

/// Plain `listen` without `ssl` and without `ssl on;` has no warnings.
#[test]
#[ignore]
fn listen_without_ssl_no_warning() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    result.assert_success_without_warnings();
}
