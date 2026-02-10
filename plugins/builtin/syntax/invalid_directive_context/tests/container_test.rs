//! Container-based integration tests for the invalid-directive-context rule.
//!
//! Verifies that nginx rejects directives placed in invalid parent contexts
//! with `"<directive>" directive is not allowed here`.
//!
//! Run with:
//!   cargo test -p invalid-directive-context-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p invalid-directive-context-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::nginx_config_test;

/// `server` at root level (outside http/stream/mail) is rejected.
#[test]
#[ignore]
fn server_at_root_rejected() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
server {
    listen 80;
}
"#,
    );
    result.assert_fails_with("\"server\" directive is not allowed here");
}

/// `location` directly inside `http` (not inside `server`) is rejected.
#[test]
#[ignore]
fn location_in_http_rejected() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    location / {
        return 200 "ok";
    }
}
"#,
    );
    result.assert_fails_with("\"location\" directive is not allowed here");
}

/// `upstream` inside `location` is rejected.
#[test]
#[ignore]
fn upstream_in_location_rejected() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80;
        location / {
            upstream backend {
                server 127.0.0.1:8080;
            }
        }
    }
}
"#,
    );
    result.assert_fails_with("\"upstream\" directive is not allowed here");
}

/// `events` inside `http` is rejected.
#[test]
#[ignore]
fn events_in_http_rejected() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    events {
        worker_connections 512;
    }
}
"#,
    );
    result.assert_fails_with("\"events\" directive is not allowed here");
}

/// `limit_except` inside `server` (not `location`) is rejected.
#[test]
#[ignore]
fn limit_except_in_server_rejected() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80;
        limit_except GET {
            deny all;
        }
    }
}
"#,
    );
    result.assert_fails_with("\"limit_except\" directive is not allowed here");
}

/// `map` inside `server` (not `http`) is rejected.
#[test]
#[ignore]
fn map_in_server_rejected() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80;
        map $uri $new {
            default /;
        }
    }
}
"#,
    );
    result.assert_fails_with("\"map\" directive is not allowed here");
}

/// Valid config with correct directive contexts passes.
#[test]
#[ignore]
fn valid_contexts_accepted() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    map $uri $new {
        default /;
    }

    upstream backend {
        server 127.0.0.1:8080;
    }

    server {
        listen 80;
        location / {
            limit_except GET {
                deny all;
            }
            return 200 "ok";
        }
    }
}
"#,
    );
    result.assert_success();
}
