use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// Without limit_req in the child block, parent limit_req is inherited.
///
/// Server block has a strict `limit_req zone=strict` (1r/m). The /test/ location
/// has no limit_req, so the parent's strict limit is inherited.
/// Uses proxy_pass because `return` runs in the rewrite phase and bypasses
/// the preaccess phase where limit_req is checked.
#[tokio::test]
#[ignore]
async fn no_child_override_inherits_parent() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(
            br#"
events {
    worker_connections 1024;
}
http {
    limit_req_zone $binary_remote_addr zone=strict:1m rate=1r/m;
    limit_req_zone $binary_remote_addr zone=loose:1m rate=1000r/s;

    server {
        listen 8080;
        location / {
            return 200 'ok';
        }
    }

    server {
        listen 80;
        limit_req zone=strict;

        location = /healthz {
            limit_req zone=loose;
            proxy_pass http://127.0.0.1:8080;
        }

        location /test/ {
            # No limit_req here - parent's strict limit is inherited
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
        )
        .await;

    let client = reqwest::Client::new();

    // First request should succeed
    let resp1 = client.get(nginx.url("/test/")).send().await.unwrap();
    assert_eq!(resp1.status(), 200, "First request should succeed");

    // Second request should be rate-limited (inherited strict: 1r/m)
    let resp2 = client.get(nginx.url("/test/")).send().await.unwrap();
    assert_eq!(
        resp2.status(),
        503,
        "Expected 503 because parent limit_req zone=strict (1r/m) should be inherited"
    );
}

/// When a child block has limit_req, parent limit_req directives are lost.
///
/// Server block has `limit_req zone=strict` (1r/m). Location block only
/// defines `limit_req zone=loose` (1000r/s), so the parent's strict limit is lost.
#[tokio::test]
#[ignore]
async fn child_overrides_parent() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(
            br#"
events {
    worker_connections 1024;
}
http {
    limit_req_zone $binary_remote_addr zone=strict:1m rate=1r/m;
    limit_req_zone $binary_remote_addr zone=loose:1m rate=1000r/s;

    server {
        listen 8080;
        location / {
            return 200 'ok';
        }
    }

    server {
        listen 80;
        limit_req zone=strict;

        location = /healthz {
            limit_req zone=loose;
            proxy_pass http://127.0.0.1:8080;
        }

        location /test/ {
            # Only loose limit - parent's strict limit is lost
            limit_req zone=loose;
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
        )
        .await;

    let client = reqwest::Client::new();

    // Both requests should succeed (strict limit is lost, loose allows 1000r/s)
    let resp1 = client.get(nginx.url("/test/")).send().await.unwrap();
    assert_eq!(resp1.status(), 200, "First request should succeed");

    let resp2 = client.get(nginx.url("/test/")).send().await.unwrap();
    assert_eq!(
        resp2.status(),
        200,
        "Expected 200 because parent limit_req zone=strict should be lost when child has its own limit_req"
    );
}

/// When parent limit_req is explicitly repeated in the child, it is preserved.
#[tokio::test]
#[ignore]
async fn repeated_in_child_preserves_all() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(
            br#"
events {
    worker_connections 1024;
}
http {
    limit_req_zone $binary_remote_addr zone=strict:1m rate=1r/m;
    limit_req_zone $binary_remote_addr zone=loose:1m rate=1000r/s;

    server {
        listen 8080;
        location / {
            return 200 'ok';
        }
    }

    server {
        listen 80;
        limit_req zone=strict;

        location = /healthz {
            limit_req zone=loose;
            proxy_pass http://127.0.0.1:8080;
        }

        location /test/ {
            # Good: parent limit_req repeated, plus new one
            limit_req zone=strict;
            limit_req zone=loose;
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
        )
        .await;

    let client = reqwest::Client::new();

    // First request should succeed
    let resp1 = client.get(nginx.url("/test/")).send().await.unwrap();
    assert_eq!(resp1.status(), 200, "First request should succeed");

    // Second request should be rate-limited (strict limit is preserved)
    let resp2 = client.get(nginx.url("/test/")).send().await.unwrap();
    assert_eq!(
        resp2.status(),
        503,
        "Expected 503 because limit_req zone=strict is explicitly repeated in child"
    );
}
