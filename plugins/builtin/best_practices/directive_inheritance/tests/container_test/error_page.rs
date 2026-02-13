use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// Without error_page in the child block, parent error_page is inherited.
///
/// Server block has `error_page 404 =200 /custom-404;` which intercepts 404
/// responses and returns 200 with a custom body. The location has no error_page,
/// so the parent's is inherited.
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
        listen 80;
        error_page 404 =200 /custom-404;

        location = /custom-404 {
            return 200 'custom-404-page';
        }

        location /test/ {
            # No error_page here - parent is inherited
            return 404;
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/test/")).await.unwrap();

    // error_page 404 is inherited from server → 404 is intercepted → returns 200
    assert_eq!(
        resp.status(),
        200,
        "Expected 200 because parent error_page 404 should be inherited"
    );
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "custom-404-page",
        "Expected custom error page body from inherited error_page"
    );
}

/// When a child block has error_page, parent error_page directives are lost.
///
/// Server block has `error_page 404 =200 /custom-404;`. Location block only
/// defines `error_page 403`, so the parent's 404 handling is lost.
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
        listen 80;
        error_page 404 =200 /custom-404;

        location = /custom-404 {
            return 200 'custom-404-page';
        }

        location = /custom-403 {
            return 200 'custom-403-page';
        }

        location /test/ {
            # Only handles 403 - parent's 404 handling is lost
            error_page 403 =200 /custom-403;
            return 404;
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/test/")).await.unwrap();

    // error_page 404 is NOT inherited (overridden by child's error_page 403)
    // → 404 is NOT intercepted → returns 404
    assert_eq!(
        resp.status(),
        404,
        "Expected 404 because parent error_page should be lost when child has its own error_page"
    );
}

/// When parent error_page is explicitly repeated in the child, it is preserved.
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
        listen 80;
        error_page 404 =200 /custom-404;

        location = /custom-404 {
            return 200 'custom-404-page';
        }

        location = /custom-403 {
            return 200 'custom-403-page';
        }

        location /test/ {
            # Good: parent error_page repeated, plus new one
            error_page 404 =200 /custom-404;
            error_page 403 =200 /custom-403;
            return 404;
        }
    }
}
"#,
    )
    .await;

    let resp = reqwest::get(nginx.url("/test/")).await.unwrap();

    // error_page 404 is explicitly repeated → 404 is intercepted → returns 200
    assert_eq!(
        resp.status(),
        200,
        "Expected 200 because error_page 404 is explicitly repeated in child"
    );
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, "custom-404-page",
        "Expected custom error page body from repeated error_page"
    );
}
