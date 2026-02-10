//! Container-based integration tests for the listen-http2-deprecated rule.
//!
//! Verifies that `listen ... http2` emits a deprecation warning in nginx 1.25.1+,
//! while the new `http2 on;` directive does not.
//!
//! Run with:
//!   cargo test -p listen-http2-deprecated-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p listen-http2-deprecated-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::nginx_config_test;

/// `listen 80 http2;` emits a deprecation warning in nginx 1.25.1+.
#[test]
#[ignore]
fn listen_http2_emits_deprecation_warning() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80 http2;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    result.assert_warns_with("the \"listen ... http2\" directive is deprecated");
}

/// `http2 on;` does not produce any deprecation warning.
#[test]
#[ignore]
fn http2_directive_no_warning() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80;
        http2 on;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    result.assert_success_without_warnings();
}

/// Multiple `listen` directives with `http2` all produce the warning.
#[test]
#[ignore]
fn multiple_listen_http2_all_warn() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80 http2;
        listen [::]:80 http2;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    result.assert_warns_with("the \"listen ... http2\" directive is deprecated");
}

/// `listen` without `http2` produces no warnings.
#[test]
#[ignore]
fn listen_without_http2_no_warning() {
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
