//! Container-based integration tests for the deprecated-ssl-protocol rule.
//!
//! Verifies that modern TLS protocols (TLSv1.2, TLSv1.3) work while
//! deprecated protocols (TLSv1.0, TLSv1.1) are rejected.
//!
//! Each test starts nginx inside a Docker container with a self-signed
//! certificate and uses `openssl s_client` to verify protocol behavior.
//!
//! Run with:
//!   cargo test -p deprecated-ssl-protocol-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p deprecated-ssl-protocol-plugin --test container_test -- --ignored

use nginx_lint_plugin::container_testing::NginxContainer;

/// Build an nginx SSL config with the given ssl_protocols directive.
fn ssl_config(ssl_protocols: &str) -> String {
    format!(
        r#"
events {{ worker_connections 1024; }}
http {{
    server {{
        listen 443 ssl;
        ssl_certificate /tmp/cert.pem;
        ssl_certificate_key /tmp/key.pem;
        ssl_protocols {ssl_protocols};
        location / {{ return 200 "ssl-ok"; }}
    }}
}}
"#,
        ssl_protocols = ssl_protocols,
    )
}

/// With only TLSv1.2 and TLSv1.3 enabled, TLSv1.2 connection succeeds.
#[tokio::test]
#[ignore]
async fn modern_protocols_tlsv1_2_succeeds() {
    let nginx = NginxContainer::start_ssl(ssl_config("TLSv1.2 TLSv1.3")).await;
    let output = nginx
        .exec_shell(
            "echo | openssl s_client -connect 127.0.0.1:443 -tls1_2 2>&1 | grep 'Protocol  :'",
        )
        .await;
    assert!(
        output.stdout.contains("TLSv1.2"),
        "Expected TLSv1.2 connection to succeed, got: {}",
        output.output()
    );
}

/// With only TLSv1.2 and TLSv1.3 enabled, TLSv1.0 connection is rejected.
#[tokio::test]
#[ignore]
async fn modern_protocols_tlsv1_0_rejected() {
    let nginx = NginxContainer::start_ssl(ssl_config("TLSv1.2 TLSv1.3")).await;
    let output = nginx
        .exec_shell("echo | openssl s_client -connect 127.0.0.1:443 -tls1 2>&1")
        .await;
    let combined = output.output();
    assert!(
        combined.contains("alert protocol version")
            || combined.contains("alert internal error")
            || combined.contains("no protocols available"),
        "Expected TLSv1.0 to be rejected, got: {combined}"
    );
}

/// With only TLSv1.2 and TLSv1.3 enabled, TLSv1.1 connection is rejected.
#[tokio::test]
#[ignore]
async fn modern_protocols_tlsv1_1_rejected() {
    let nginx = NginxContainer::start_ssl(ssl_config("TLSv1.2 TLSv1.3")).await;
    let output = nginx
        .exec_shell("echo | openssl s_client -connect 127.0.0.1:443 -tls1_1 2>&1")
        .await;
    let combined = output.output();
    assert!(
        combined.contains("alert protocol version")
            || combined.contains("alert internal error")
            || combined.contains("no protocols available"),
        "Expected TLSv1.1 to be rejected, got: {combined}"
    );
}

/// Even when nginx is configured to allow TLSv1.0, OpenSSL 3.x rejects it
/// at the library level. This demonstrates that deprecated protocols are
/// effectively broken even when configured.
#[tokio::test]
#[ignore]
async fn deprecated_tlsv1_0_fails_even_when_configured() {
    let nginx = NginxContainer::start_ssl(ssl_config("TLSv1 TLSv1.1 TLSv1.2")).await;
    let output = nginx
        .exec_shell("echo | openssl s_client -connect 127.0.0.1:443 -tls1 2>&1")
        .await;
    let combined = output.output();
    assert!(
        combined.contains("alert protocol version")
            || combined.contains("alert internal error")
            || combined.contains("no protocols available"),
        "Expected TLSv1.0 to fail even when configured, got: {combined}"
    );
}

/// nginx -t accepts deprecated protocols without any warning,
/// which is why the lint rule is valuable.
#[test]
#[ignore]
fn nginx_accepts_deprecated_protocols_without_warning() {
    let result = nginx_lint_plugin::container_testing::nginx_config_test(
        r#"
events { worker_connections 1024; }
http {
    server {
        listen 80;
        ssl_protocols SSLv3 TLSv1 TLSv1.1;
        location / { return 200 "ok"; }
    }
}
"#,
    );
    // nginx accepts deprecated protocols without any warning
    result.assert_success_without_warnings();
}
