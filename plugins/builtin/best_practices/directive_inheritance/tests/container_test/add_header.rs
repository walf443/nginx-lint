use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// Helper to get a header value from a response.
fn get_header(resp: &reqwest::Response, name: &str) -> Option<String> {
    resp.headers()
        .get(name)
        .map(|v| v.to_str().unwrap().to_string())
}

#[tokio::test]
#[ignore]
async fn parent_inherited_when_no_child() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;
        add_header X-Server-Header "from-server";

        location / {
            return 200 'OK';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        get_header(&resp, "x-server-header").as_deref(),
        Some("from-server"),
        "Parent header should be inherited when location has no add_header"
    );
}

#[tokio::test]
#[ignore]
async fn parent_lost_when_child_has_add_header() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;
        add_header X-Server-Header "from-server";

        location / {
            add_header X-Location-Header "from-location";
            return 200 'OK';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    assert_eq!(resp.status(), 200);

    assert_eq!(
        get_header(&resp, "x-location-header").as_deref(),
        Some("from-location"),
        "Child header should be present"
    );
    assert_eq!(
        get_header(&resp, "x-server-header"),
        None,
        "Parent header should be LOST when child block has its own add_header"
    );
}

#[tokio::test]
#[ignore]
async fn preserved_when_explicitly_repeated() {
    let nginx = NginxContainer::start(
        br#"
events {
    worker_connections 1024;
}
http {
    server {
        listen 80;
        add_header X-Server-Header "from-server";

        location / {
            add_header X-Server-Header "from-server";
            add_header X-Location-Header "from-location";
            return 200 'OK';
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/")).await.unwrap();
    assert_eq!(resp.status(), 200);

    assert_eq!(
        get_header(&resp, "x-server-header").as_deref(),
        Some("from-server"),
        "Explicitly repeated parent header should be present"
    );
    assert_eq!(
        get_header(&resp, "x-location-header").as_deref(),
        Some("from-location"),
        "Child header should also be present"
    );
}
