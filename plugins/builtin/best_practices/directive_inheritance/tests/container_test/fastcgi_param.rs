use nginx_lint_plugin::container_testing::testcontainers::{
    ContainerAsync, GenericImage, ImageExt, core::WaitFor, runners::AsyncRunner,
};
use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// PHP script that echoes specific fastcgi_param values visible via $_SERVER.
const PHP_ECHO_PARAMS: &[u8] = b"<?php
header('Content-Type: text/plain');
$keys = ['SCRIPT_FILENAME', 'CUSTOM_PARENT', 'CUSTOM_CHILD'];
foreach ($keys as $k) {
    echo $k . '=' . ($_SERVER[$k] ?? '') . \"\\n\";
}
";

/// Helper to extract a named value from "KEY=value\nKEY=value" response body.
fn get_value(body: &str, key: &str) -> String {
    body.lines()
        .find_map(|line| {
            let (k, v) = line.split_once('=')?;
            if k == key {
                Some(v.to_string())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// Start a PHP-FPM container on the given network.
///
/// Returns the container (must be kept alive) and its bridge IP address.
async fn start_php_fpm(network: &str) -> (ContainerAsync<GenericImage>, String) {
    let php = GenericImage::new("php", "8.3-fpm")
        .with_wait_for(WaitFor::message_on_stderr("ready to handle connections"))
        .with_network(network)
        .with_copy_to("/var/www/html/test.php", PHP_ECHO_PARAMS.to_vec())
        .start()
        .await
        .expect("Failed to start PHP-FPM container");

    let ip = php
        .get_bridge_ip_address()
        .await
        .expect("Failed to get PHP-FPM bridge IP")
        .to_string();

    (php, ip)
}

/// When a child block has fastcgi_param, parent params are lost.
///
/// Server block defines SCRIPT_FILENAME, REQUEST_METHOD, and CUSTOM_PARENT.
/// Location block defines SCRIPT_FILENAME, REQUEST_METHOD (required for PHP-FPM),
/// and CUSTOM_CHILD, but does NOT repeat CUSTOM_PARENT → it is lost.
#[tokio::test]
#[ignore]
async fn child_overrides_parent() {
    let network = format!("fcgi-override-{}", std::process::id());
    let (_php, php_ip) = start_php_fpm(&network).await;

    let nginx = NginxContainer::builder()
        .network(&network)
        .start(format!(
            r#"
events {{
    worker_connections 1024;
}}
http {{
    server {{
        listen 80;

        fastcgi_param SCRIPT_FILENAME /var/www/html/test.php;
        fastcgi_param REQUEST_METHOD $request_method;
        fastcgi_param CUSTOM_PARENT "parent_value";

        location / {{
            # Repeats SCRIPT_FILENAME and REQUEST_METHOD so PHP works,
            # but CUSTOM_PARENT is lost
            fastcgi_param SCRIPT_FILENAME /var/www/html/test.php;
            fastcgi_param REQUEST_METHOD $request_method;
            fastcgi_param CUSTOM_CHILD "child_value";
            fastcgi_pass {php_ip}:9000;
        }}
    }}
}}
"#
        ))
        .await;

    let resp = reqwest::get(nginx.url("/test.php")).await.unwrap();
    let body = resp.text().await.unwrap();

    // CUSTOM_PARENT is lost because child has its own fastcgi_param directives
    assert_eq!(
        get_value(&body, "CUSTOM_PARENT"),
        "",
        "Expected CUSTOM_PARENT to be lost when child block has its own fastcgi_param"
    );
    // CUSTOM_CHILD is set by the child block
    assert_eq!(
        get_value(&body, "CUSTOM_CHILD"),
        "child_value",
        "Expected CUSTOM_CHILD to be set by child block"
    );
}

/// When all parent params are repeated in the child block, they are preserved.
#[tokio::test]
#[ignore]
async fn repeated_in_child_preserves_all() {
    let network = format!("fcgi-repeat-{}", std::process::id());
    let (_php, php_ip) = start_php_fpm(&network).await;

    let nginx = NginxContainer::builder()
        .network(&network)
        .start(format!(
            r#"
events {{
    worker_connections 1024;
}}
http {{
    server {{
        listen 80;

        fastcgi_param SCRIPT_FILENAME /var/www/html/test.php;
        fastcgi_param REQUEST_METHOD $request_method;
        fastcgi_param CUSTOM_PARENT "parent_value";

        location / {{
            # Good: all parent params repeated
            fastcgi_param SCRIPT_FILENAME /var/www/html/test.php;
            fastcgi_param REQUEST_METHOD $request_method;
            fastcgi_param CUSTOM_PARENT "parent_value";
            fastcgi_param CUSTOM_CHILD "child_value";
            fastcgi_pass {php_ip}:9000;
        }}
    }}
}}
"#
        ))
        .await;

    let resp = reqwest::get(nginx.url("/test.php")).await.unwrap();
    let body = resp.text().await.unwrap();

    assert_eq!(
        get_value(&body, "CUSTOM_PARENT"),
        "parent_value",
        "Expected CUSTOM_PARENT to be preserved when explicitly repeated"
    );
    assert_eq!(
        get_value(&body, "CUSTOM_CHILD"),
        "child_value",
        "Expected CUSTOM_CHILD to be set"
    );
}

/// Without fastcgi_param in the child block, parent params are inherited.
#[tokio::test]
#[ignore]
async fn no_child_override_inherits_parent() {
    let network = format!("fcgi-inherit-{}", std::process::id());
    let (_php, php_ip) = start_php_fpm(&network).await;

    let nginx = NginxContainer::builder()
        .network(&network)
        .start(format!(
            r#"
events {{
    worker_connections 1024;
}}
http {{
    server {{
        listen 80;

        fastcgi_param SCRIPT_FILENAME /var/www/html/test.php;
        fastcgi_param REQUEST_METHOD $request_method;
        fastcgi_param CUSTOM_PARENT "parent_value";

        location / {{
            # No fastcgi_param here - parent params are inherited
            fastcgi_pass {php_ip}:9000;
        }}
    }}
}}
"#
        ))
        .await;

    let resp = reqwest::get(nginx.url("/test.php")).await.unwrap();
    let body = resp.text().await.unwrap();

    assert_eq!(
        get_value(&body, "CUSTOM_PARENT"),
        "parent_value",
        "Expected CUSTOM_PARENT to be inherited from parent when no child override"
    );
}
