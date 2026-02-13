use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// Without limit_conn in the child block, parent limit_conn is inherited.
///
/// Server block has `limit_conn strict 1` (1 concurrent connection per IP).
/// The /test/ location has no limit_conn, so the parent's limit is inherited.
/// Uses `limit_rate 100` to keep the first connection open while testing the second.
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
    limit_conn_zone $binary_remote_addr zone=strict:1m;
    limit_conn_zone $binary_remote_addr zone=loose:1m;

    server {
        listen 8080;
        location / {
            return 200 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
        }
    }

    server {
        listen 80;
        limit_conn strict 1;

        location = /healthz {
            limit_conn loose 100;
            proxy_pass http://127.0.0.1:8080;
        }

        location /test/ {
            # No limit_conn here - parent's strict limit (1 conn) is inherited
            limit_rate 100;
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
        )
        .await;

    let client = reqwest::Client::new();

    // First request starts streaming body at 100 bytes/sec, holding the connection open
    let resp1 = client.get(nginx.url("/test/")).send().await.unwrap();
    assert_eq!(resp1.status(), 200, "First request should succeed");

    // Second request while first is still active - should be rejected
    let resp2 = client.get(nginx.url("/test/")).send().await.unwrap();
    assert_eq!(
        resp2.status(),
        503,
        "Expected 503 because parent limit_conn strict 1 should be inherited"
    );

    drop(resp1);
}

/// When a child block has limit_conn, parent limit_conn directives are lost.
///
/// Server block has `limit_conn strict 1`. Location block only defines
/// `limit_conn loose 100`, so the parent's strict limit is lost.
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
    limit_conn_zone $binary_remote_addr zone=strict:1m;
    limit_conn_zone $binary_remote_addr zone=loose:1m;

    server {
        listen 8080;
        location / {
            return 200 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
        }
    }

    server {
        listen 80;
        limit_conn strict 1;

        location = /healthz {
            limit_conn loose 100;
            proxy_pass http://127.0.0.1:8080;
        }

        location /test/ {
            # Only loose limit (100 conns) - parent's strict (1 conn) is lost
            limit_conn loose 100;
            limit_rate 100;
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
        )
        .await;

    let client = reqwest::Client::new();

    // First request holds the connection open
    let resp1 = client.get(nginx.url("/test/")).send().await.unwrap();
    assert_eq!(resp1.status(), 200, "First request should succeed");

    // Second concurrent request should also succeed (loose limit allows 100)
    let resp2 = client.get(nginx.url("/test/")).send().await.unwrap();
    assert_eq!(
        resp2.status(),
        200,
        "Expected 200 because parent limit_conn strict 1 should be lost when child has its own limit_conn"
    );

    drop(resp1);
    drop(resp2);
}

/// When parent limit_conn is explicitly repeated in the child, it is preserved.
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
    limit_conn_zone $binary_remote_addr zone=strict:1m;
    limit_conn_zone $binary_remote_addr zone=loose:1m;

    server {
        listen 8080;
        location / {
            return 200 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
        }
    }

    server {
        listen 80;
        limit_conn strict 1;

        location = /healthz {
            limit_conn loose 100;
            proxy_pass http://127.0.0.1:8080;
        }

        location /test/ {
            # Good: parent limit_conn repeated, plus new one
            limit_conn strict 1;
            limit_conn loose 100;
            limit_rate 100;
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
        )
        .await;

    let client = reqwest::Client::new();

    // First request holds the connection open
    let resp1 = client.get(nginx.url("/test/")).send().await.unwrap();
    assert_eq!(resp1.status(), 200, "First request should succeed");

    // Second request should be rejected (strict 1 is preserved)
    let resp2 = client.get(nginx.url("/test/")).send().await.unwrap();
    assert_eq!(
        resp2.status(),
        503,
        "Expected 503 because limit_conn strict 1 is explicitly repeated in child"
    );

    drop(resp1);
}
