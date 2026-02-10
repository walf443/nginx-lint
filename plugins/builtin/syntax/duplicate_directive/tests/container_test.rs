//! Container-based integration tests for the duplicate-directive rule.
//!
//! Verifies that nginx rejects duplicate directives with
//! `"<directive>" directive is duplicate`.
//!
//! Most duplicate directives cause an `[emerg]` error and config test failure.
//! Some (like load balancing methods in upstream) only produce a warning.
//!
//! Run with:
//!   cargo test -p duplicate-directive-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p duplicate-directive-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::nginx_config_test;

// =========================================================================
// Main context duplicates
// =========================================================================

/// Duplicate `worker_processes` in main context is rejected.
#[test]
#[ignore]
fn duplicate_worker_processes_rejected() {
    let result = nginx_config_test(
        r#"
worker_processes 4;
worker_processes 8;
events { worker_connections 1024; }
http {
    server {
        listen 80;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    result.assert_fails_with("\"worker_processes\" directive is duplicate");
}

/// Duplicate `pid` in main context is rejected.
#[test]
#[ignore]
fn duplicate_pid_rejected() {
    let result = nginx_config_test(
        r#"
pid /run/nginx.pid;
pid /var/run/nginx.pid;
events { worker_connections 1024; }
http {
    server {
        listen 80;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    result.assert_fails_with("\"pid\" directive is duplicate");
}

// =========================================================================
// HTTP context duplicates
// =========================================================================

/// Duplicate `sendfile` in http context is rejected.
#[test]
#[ignore]
fn duplicate_sendfile_in_http_rejected() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    sendfile on;
    sendfile off;
    server {
        listen 80;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    result.assert_fails_with("\"sendfile\" directive is duplicate");
}

// =========================================================================
// Server context duplicates
// =========================================================================

/// Duplicate `server_tokens` in server context is rejected.
#[test]
#[ignore]
fn duplicate_server_tokens_in_server_rejected() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80;
        server_tokens off;
        server_tokens on;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    result.assert_fails_with("\"server_tokens\" directive is duplicate");
}

// =========================================================================
// Location context duplicates
// =========================================================================

/// Duplicate `root` in location context is rejected.
#[test]
#[ignore]
fn duplicate_root_in_location_rejected() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80;
        location / {
            root /var/www;
            root /var/www/html;
            return 200 "ok";
        }
    }
}
"#,
    );
    result.assert_fails_with("\"root\" directive is duplicate");
}

// =========================================================================
// Upstream context duplicates
// =========================================================================

/// Duplicate load balancing method in upstream produces a warning (not error).
#[test]
#[ignore]
fn duplicate_ip_hash_in_upstream_warns() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    upstream backend {
        ip_hash;
        ip_hash;
        server 127.0.0.1:8080;
    }
    server {
        listen 80;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    result.assert_warns_with("load balancing method redefined");
}

// =========================================================================
// No duplicates (valid configs)
// =========================================================================

/// Same directive in different contexts is valid.
#[test]
#[ignore]
fn same_directive_different_contexts_accepted() {
    let result = nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80;
        location / {
            root /var/www;
        }
        location /static {
            root /var/static;
        }
    }
}
"#,
    );
    result.assert_success();
}
