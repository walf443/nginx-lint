//! Container-based integration tests for the weak-ssl-ciphers rule.
//!
//! Verifies that strong cipher configurations reject non-PFS ciphers while
//! weak configurations (like `ALL`) allow them, demonstrating why the lint
//! rule is valuable.
//!
//! Run with:
//!   cargo test -p weak-ssl-ciphers-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p weak-ssl-ciphers-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::NginxContainer;

/// Build an nginx SSL config with the given ssl_ciphers directive.
/// Uses TLSv1.2 only to ensure cipher negotiation is predictable.
fn ssl_config(ssl_ciphers: &str) -> Vec<u8> {
    format!(
        r#"
events {{ worker_connections 1024; }}
http {{
    server {{
        listen 443 ssl;
        ssl_certificate /tmp/cert.pem;
        ssl_certificate_key /tmp/key.pem;
        ssl_protocols TLSv1.2;
        ssl_ciphers {ssl_ciphers};
        location / {{ return 200 "ssl-ok"; }}
    }}
}}
"#,
        ssl_ciphers = ssl_ciphers,
    )
    .into_bytes()
}

/// With strong cipher configuration (ECDHE only), a PFS cipher succeeds.
#[tokio::test]
#[ignore]
async fn strong_ciphers_accept_pfs_connection() {
    let ciphers = "ECDHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5";
    let nginx = NginxContainer::start_ssl(&ssl_config(ciphers)).await;
    let output = nginx
        .exec_shell(
            "echo | openssl s_client -connect 127.0.0.1:443 -tls1_2 -cipher ECDHE-RSA-AES128-GCM-SHA256 2>&1 | grep 'Cipher is'",
        )
        .await;
    assert!(
        output.stdout.contains("ECDHE-RSA-AES128-GCM-SHA256"),
        "Expected PFS cipher to be negotiated, got: {}",
        output.output()
    );
}

/// With strong cipher configuration (ECDHE only), a non-PFS cipher is rejected.
///
/// AES128-SHA lacks forward secrecy and should be refused when only
/// ECDHE ciphers are configured.
#[tokio::test]
#[ignore]
async fn strong_ciphers_reject_non_pfs() {
    let ciphers = "ECDHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5";
    let nginx = NginxContainer::start_ssl(&ssl_config(ciphers)).await;
    let output = nginx
        .exec_shell(
            "echo | openssl s_client -connect 127.0.0.1:443 -tls1_2 -cipher AES128-SHA 2>&1",
        )
        .await;
    let combined = output.output();
    assert!(
        combined.contains("handshake failure") || combined.contains("Cipher is (NONE)"),
        "Expected non-PFS cipher to be rejected, got: {combined}"
    );
}

/// With `ssl_ciphers ALL`, non-PFS ciphers like AES128-SHA are allowed.
///
/// This demonstrates the security issue: `ALL` includes ciphers without
/// forward secrecy, which the lint rule flags.
#[tokio::test]
#[ignore]
async fn all_ciphers_allow_non_pfs() {
    let nginx = NginxContainer::start_ssl(&ssl_config("ALL")).await;
    let output = nginx
        .exec_shell(
            "echo | openssl s_client -connect 127.0.0.1:443 -tls1_2 -cipher AES128-SHA 2>&1 | grep 'Cipher is'",
        )
        .await;
    assert!(
        output.stdout.contains("AES128-SHA"),
        "Expected ALL to allow AES128-SHA (non-PFS), got: {}",
        output.output()
    );
}

/// Even with `ssl_ciphers ALL`, RC4 is rejected at the OpenSSL 3.x library level.
///
/// RC4 is completely removed from modern OpenSSL, so even when nginx
/// configures it, the client can't negotiate it.
#[tokio::test]
#[ignore]
async fn rc4_rejected_by_openssl_even_with_all() {
    let nginx = NginxContainer::start_ssl(&ssl_config("ALL")).await;
    let output = nginx
        .exec_shell("echo | openssl s_client -connect 127.0.0.1:443 -tls1_2 -cipher RC4-SHA 2>&1")
        .await;
    let combined = output.output();
    assert!(
        combined.contains("no cipher match") || combined.contains("no ciphers available"),
        "Expected RC4 to be rejected by OpenSSL, got: {combined}"
    );
}

/// nginx -t accepts weak cipher configurations without any warning,
/// which is why the lint rule is valuable.
#[test]
#[ignore]
fn nginx_accepts_weak_ciphers_without_warning() {
    let result = nginx_lint_plugin::container_testing::nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80;
        ssl_ciphers ALL;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    result.assert_success_without_warnings();
}
