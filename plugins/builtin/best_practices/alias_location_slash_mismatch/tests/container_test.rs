//! Container-based integration tests for the alias-location-slash-mismatch rule.
//!
//! Verifies that a trailing-slash mismatch between `location` and `alias`
//! actually causes 404 errors in a real nginx instance.
//!
//! When `location /files/` has `alias /etc/nginx` (no trailing slash),
//! a request to `/files/mime.types` resolves to `/etc/nginxmime.types` (broken).
//!
//! Run with:
//!   cargo test -p alias-location-slash-mismatch-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p alias-location-slash-mismatch-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, nginx_conf_dir, reqwest};

#[tokio::test]
#[ignore]
async fn mismatch_causes_404() {
    // location ends with / but alias does NOT
    // /files/mime.types → strip "/files/" → "mime.types" → "{conf_dir}" + "mime.types"
    //   = "{conf_dir}mime.types" → 404!
    let conf_dir = nginx_conf_dir();
    let config = format!(
        r#"
events {{
    worker_connections 1024;
}}
http {{
    server {{
        listen 80;

        location /healthz {{
            return 200 'OK';
        }}

        location /files/ {{
            alias {conf_dir};
        }}
    }}
}}
"#
    );
    let nginx = NginxContainer::builder().health_path("/healthz").start(config.as_bytes()).await;

    let resp = reqwest::get(nginx.url("/files/mime.types")).await.unwrap();
    assert_eq!(
        resp.status(),
        404,
        "Expected 404 because alias path becomes {conf_dir}mime.types (missing slash)"
    );
}

#[tokio::test]
#[ignore]
async fn matching_slash_serves_file() {
    // location ends with / and alias also ends with /
    // /files/mime.types → strip "/files/" → "mime.types" → "{conf_dir}/" + "mime.types"
    //   = "{conf_dir}/mime.types" → 200
    let conf_dir = nginx_conf_dir();
    let config = format!(
        r#"
events {{
    worker_connections 1024;
}}
http {{
    server {{
        listen 80;

        location /healthz {{
            return 200 'OK';
        }}

        location /files/ {{
            alias {conf_dir}/;
        }}
    }}
}}
"#
    );
    let nginx = NginxContainer::builder().health_path("/healthz").start(config.as_bytes()).await;

    let resp = reqwest::get(nginx.url("/files/mime.types")).await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "Expected 200 because alias path correctly becomes {conf_dir}/mime.types"
    );
}

#[tokio::test]
#[ignore]
async fn no_trailing_slash_on_location_works_without_mismatch() {
    // Neither location nor alias ends with /
    // /files/mime.types → strip "/files" → "/mime.types" → "{conf_dir}" + "/mime.types"
    //   = "{conf_dir}/mime.types" → 200
    // This works because the remaining URI retains its leading slash.
    let conf_dir = nginx_conf_dir();
    let config = format!(
        r#"
events {{
    worker_connections 1024;
}}
http {{
    server {{
        listen 80;

        location /healthz {{
            return 200 'OK';
        }}

        location /files {{
            alias {conf_dir};
        }}
    }}
}}
"#
    );
    let nginx = NginxContainer::builder().health_path("/healthz").start(config.as_bytes()).await;

    let resp = reqwest::get(nginx.url("/files/mime.types")).await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "Expected 200 because without trailing slash on location, the URI boundary is preserved"
    );
}
