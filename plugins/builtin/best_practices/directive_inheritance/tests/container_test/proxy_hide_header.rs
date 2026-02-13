use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// Helper to get a header value from a response.
fn get_header(resp: &reqwest::Response, name: &str) -> Option<String> {
    resp.headers()
        .get(name)
        .map(|v| v.to_str().unwrap().to_string())
}

/// When a child block has proxy_hide_header, parent directives are lost.
///
/// Server block hides X-Powered-By. Location block only hides X-Custom,
/// so X-Powered-By is no longer hidden and becomes visible to the client.
#[tokio::test]
#[ignore]
async fn child_overrides_parent() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 8080;
        location / {
            add_header X-Powered-By "PHP/8.3";
            add_header X-Custom "secret";
            return 200 'OK';
        }
    }

    server {
        listen 80;
        server_name _;
        proxy_hide_header X-Powered-By;

        location / {
            # Only hides X-Custom - parent's X-Powered-By hiding is lost
            proxy_hide_header X-Custom;
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();

    // X-Powered-By is visible because parent proxy_hide_header is overridden
    assert!(
        get_header(&resp, "x-powered-by").is_some(),
        "Expected X-Powered-By to be visible when parent proxy_hide_header is overridden"
    );
    // X-Custom is hidden by the child block
    assert_eq!(
        get_header(&resp, "x-custom"),
        None,
        "Expected X-Custom to be hidden by child block"
    );
}

/// When all parent directives are repeated in the child block, both are hidden.
#[tokio::test]
#[ignore]
async fn repeated_in_child_preserves_all() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 8080;
        location / {
            add_header X-Powered-By "PHP/8.3";
            add_header X-Custom "secret";
            return 200 'OK';
        }
    }

    server {
        listen 80;
        server_name _;
        proxy_hide_header X-Powered-By;

        location / {
            # Good: parent directive repeated
            proxy_hide_header X-Powered-By;
            proxy_hide_header X-Custom;
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();

    assert_eq!(
        get_header(&resp, "x-powered-by"),
        None,
        "Expected X-Powered-By to be hidden when explicitly repeated"
    );
    assert_eq!(
        get_header(&resp, "x-custom"),
        None,
        "Expected X-Custom to be hidden"
    );
}

/// Without proxy_hide_header in the child block, parent directives are inherited.
#[tokio::test]
#[ignore]
async fn no_child_override_inherits_parent() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 8080;
        location / {
            add_header X-Powered-By "PHP/8.3";
            return 200 'OK';
        }
    }

    server {
        listen 80;
        server_name _;
        proxy_hide_header X-Powered-By;

        location / {
            # No proxy_hide_header here - parent directive is inherited
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();

    assert_eq!(
        get_header(&resp, "x-powered-by"),
        None,
        "Expected X-Powered-By to be hidden via inherited parent proxy_hide_header"
    );
}
