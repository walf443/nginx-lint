use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// Helper to extract a named value from "key=value\nkey=value" response body.
fn get_value(body: &str, key: &str) -> String {
    body.lines()
        .find_map(|line| {
            let (k, v) = line.split_once('=')?;
            if k == key { Some(v.to_string()) } else { None }
        })
        .unwrap_or_default()
}

/// When a child block has proxy_set_header, parent headers are lost.
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
            return 200 "host=$http_host\nx-real-ip=$http_x_real_ip\nx-custom=$http_x_custom";
        }
    }

    server {
        listen 80;
        server_name _;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;

        location / {
            # Only sets X-Custom - parent headers are lost
            proxy_set_header X-Custom "value";
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(nginx.url("/"))
        .header("Host", "example.com")
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();

    // Host reverts to $proxy_host because parent proxy_set_header is overridden
    assert_eq!(
        get_value(&body, "host"),
        "127.0.0.1:8080",
        "Expected Host to revert to $proxy_host when parent headers are overridden"
    );
    // X-Real-IP is lost
    assert_eq!(
        get_value(&body, "x-real-ip"),
        "",
        "Expected X-Real-IP to be lost when parent headers are overridden"
    );
    // X-Custom is set by the child block
    assert_eq!(
        get_value(&body, "x-custom"),
        "value",
        "Expected X-Custom to be set by child block"
    );
}

/// When all parent headers are repeated in the child block, they are preserved.
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
            return 200 "host=$http_host\nx-real-ip=$http_x_real_ip\nx-custom=$http_x_custom";
        }
    }

    server {
        listen 80;
        server_name _;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;

        location / {
            # Good: all parent headers repeated
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Custom "value";
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(nginx.url("/"))
        .header("Host", "example.com")
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();

    assert_eq!(
        get_value(&body, "host"),
        "example.com",
        "Expected Host to be preserved when explicitly repeated"
    );
    assert!(
        !get_value(&body, "x-real-ip").is_empty(),
        "Expected X-Real-IP to be preserved when explicitly repeated"
    );
    assert_eq!(
        get_value(&body, "x-custom"),
        "value",
        "Expected X-Custom to be set"
    );
}

/// Without proxy_set_header in the child block, parent headers are inherited normally.
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
            return 200 "host=$http_host\nx-real-ip=$http_x_real_ip";
        }
    }

    server {
        listen 80;
        server_name _;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;

        location / {
            # No proxy_set_header here - parent headers are inherited
            proxy_pass http://127.0.0.1:8080;
        }
    }
}
"#,
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(nginx.url("/"))
        .header("Host", "example.com")
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();

    assert_eq!(
        get_value(&body, "host"),
        "example.com",
        "Expected Host to be inherited from parent when no child override"
    );
    assert!(
        !get_value(&body, "x-real-ip").is_empty(),
        "Expected X-Real-IP to be inherited from parent when no child override"
    );
}
