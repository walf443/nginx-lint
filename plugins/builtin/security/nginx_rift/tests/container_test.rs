//! Container-based integration tests for the nginx-rift rule
//! (CVE-2026-42945, "NGINX Rift").
//!
//! These tests prove the rule is targeting a *real* nginx bug by exercising
//! the flagged directive sequence against a vulnerable nginx and observing
//! the bug's signature directly.
//!
//! # The bug's observable signature
//!
//! With the vulnerable pattern (`rewrite ... ?... ;` followed by a `set $x $1;`
//! that consumes the location-regex capture), nginx computes the buffer size
//! for the captured value on a freshly zeroed sub-engine (is_args=0, so each
//! byte is counted as 1 byte) but copies the value on the main engine (where
//! the rewrite has leaked is_args=1, so escape-prone characters expand). The
//! buffer is therefore undersized.
//!
//! On a request whose capture contains `+`, the visible symptoms on
//! vulnerable nginx are:
//!
//! - the captured value is wrongly arg-escaped (`+` becomes `%2B`), and
//! - the value is *truncated* mid-escape, because the worker writes more
//!   bytes than the buffer holds and the output is cut off at the allocated
//!   length.
//!
//! For example, `/api/foo+bar` (capture `foo+bar`, raw_size = 7) on
//! nginx 1.30.0 returns `captured=foo%2Bb` — the worker tried to write
//! `foo%2Bbar` (9 bytes) into a 7-byte buffer.
//!
//! On a patched build (>= 1.30.1, 1.31.0) the `e->is_args = 0` reset in
//! `ngx_http_script_regex_end_code` keeps args-escaping off for the
//! capture, so the same request returns `captured=foo+bar` cleanly.
//!
//! # Why we skip on patched versions
//!
//! On patched nginx the signature is *absent* by design, so there is
//! nothing to assert on. We therefore skip the bug-observation tests on
//! `nginx_version_at_least(1, 31)`. We do NOT skip on potentially-
//! unpatched tags (`nginx:1.30`, `openresty:noble`, etc.) — those are
//! exactly the builds we want to exercise. Confirmed observed on:
//!
//! - `nginx:1.30.0` (CVE patched in 1.30.1) — truncated/escaped
//! - `openresty/openresty:noble` (openresty/1.29.2.3) — mis-escaped
//! - `nginx:1.31`, `freenginx:1.31` — clean (tests skipped)
//!
//! # Why we don't assert on a worker crash
//!
//! The CVSS-4.0 = 9.2 RCE scenario rests on the buffer overrun corrupting
//! adjacent heap allocations. Whether that manifests as a SIGSEGV depends
//! on heap layout and is not a reliable CI signal — even on 1.30.0 the
//! request returns 200 OK with corrupted output rather than crashing
//! deterministically. The truncated/mis-escaped output is the deterministic
//! precondition; if that is present, the heap is being overrun.
//!
//! Run with:
//!   cargo test -p nginx-rift-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_IMAGE=nginx:1.30.0 cargo test -p nginx-rift-plugin \
//!       --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{
    NginxContainer, nginx_image_tag, nginx_version_at_least, reqwest,
};

/// On patched builds the bug's signature is absent by design, so the
/// observation tests have nothing to assert on. Skip them there. We do
/// NOT skip on potentially-unpatched tags — that is what we want to test.
///
/// Considered "patched":
/// - nginx >= 1.31 (major.minor check)
/// - the floating `1.30` tag — Docker Hub now resolves it to the patched
///   `1.30.1`, even though `nginx_version_at_least` only sees (1, 30)
/// - any explicit `1.30.x` where x >= 1 (1.30.1 onward shipped the fix)
///
/// To run the observation tests against the genuine unpatched build, use
/// the explicit pinned tag, e.g. `NGINX_IMAGE=nginx:1.30.0`.
fn skip_on_patched_version() -> bool {
    if nginx_version_at_least(1, 31) {
        return true;
    }
    let tag = nginx_image_tag();
    // Bare `1.30` is the floating Docker tag — now patched (1.30.1+).
    if tag == "1.30" {
        return true;
    }
    // Explicit 1.30.x with x >= 1 is patched.
    if let Some(rest) = tag.strip_prefix("1.30.")
        && let Ok(patch) = rest.parse::<u32>()
    {
        return patch >= 1;
    }
    false
}

/// Vulnerable pattern: rewrite with `?` in the replacement, followed by a
/// `set` that consumes the unnamed capture in the same location block.
/// This is the exact directive sequence the lint rule flags.
fn vulnerable_config() -> &'static str {
    r#"events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location /healthz {
            return 200 'OK';
        }

        location ~ ^/api/(.*)$ {
            rewrite ^/api/(.*)$ /internal?migrated=true;
            set $original_endpoint $1;
            return 200 "captured=$original_endpoint\n";
        }
    }
}
"#
}

/// Second vulnerable shape: rewrite-then-rewrite. The first rewrite leaks
/// `is_args = 1`; the second rewrite then references `$1` in its
/// replacement, where the leaked flag causes args-escaping (`+` → `%2B`)
/// to be applied to the captured value as it is written into the
/// rewritten URI. This is a separate observable manifestation of the
/// same `is_args` leak the rule flags.
fn vulnerable_rewrite_rewrite_config() -> &'static str {
    r#"events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location /healthz {
            return 200 'OK';
        }

        location ~ ^/foo/(.*)$ {
            rewrite ^/foo/(.*)$ /bar/$1?x=1;
            rewrite ^/bar/(.*)$ /baz/$1;
        }
        location ~ ^/baz/(.*)$ {
            return 200 "final=$1\n";
        }
    }
}
"#
}

/// Recommended fix #1: drop the `?` from the rewrite replacement so the
/// `is_args` flag is never set in the first place. The captured value is
/// then routed through `set` without going through the args-escaping path.
fn safe_no_question_mark_config() -> &'static str {
    r#"events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location /healthz {
            return 200 'OK';
        }

        location ~ ^/api/(.*)$ {
            rewrite ^/api/(.*)$ /internal/migrated;
            set $original_endpoint $1;
            return 200 "captured=$original_endpoint\n";
        }
    }
}
"#
}

/// Recommended fix #2: switch to named captures. Named captures are
/// resolved through a separate code path that doesn't share the rewrite
/// engine's `is_args` state.
fn safe_named_capture_config() -> &'static str {
    r#"events {
    worker_connections 1024;
}
http {
    server {
        listen 80;

        location /healthz {
            return 200 'OK';
        }

        location ~ ^/api/(?<rest>.*)$ {
            rewrite ^/api/(?<rest>.*)$ /internal?migrated=true;
            set $original_endpoint $rest;
            return 200 "captured=$original_endpoint\n";
        }
    }
}
"#
}

// ============================================================================
// The vulnerable pattern is syntactically valid nginx config
// ============================================================================
//
// Runs on every version. The rule must not be flagging a pattern that
// nginx rejects at config-load time.

#[tokio::test]
#[ignore]
async fn vulnerable_config_loads_and_serves_normal_request() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(vulnerable_config())
        .await;

    // No escape-prone characters: identical output on vulnerable and
    // patched builds, so this is a version-independent smoke test.
    let resp = reqwest::get(nginx.url("/api/users")).await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "vulnerable-pattern config must load and route normal requests"
    );
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("captured=users"),
        "expected capture to round-trip through `set $original_endpoint $1`, got: {body}"
    );
}

// ============================================================================
// On a vulnerable build, the bug's signature is observable
// ============================================================================

#[tokio::test]
#[ignore]
async fn vulnerable_pattern_corrupts_capture_with_plus() {
    if skip_on_patched_version() {
        eprintln!("Skipping: nginx >= 1.31 has the CVE-2026-42945 fix");
        return;
    }
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(vulnerable_config())
        .await;

    // raw capture = "foo+bar" (7 bytes), arg-escaped = "foo%2Bbar" (9 bytes).
    // The vulnerable worker allocates 7 bytes (length-pass, is_args=0) and
    // writes 9 bytes (copy-pass, is_args=1) — output is mis-escaped *and*
    // truncated.
    let resp = reqwest::get(nginx.url("/api/foo+bar")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("%2B"),
        "expected leaked is_args to wrongly arg-escape `+` to `%2B` in the \
         captured value (CVE-2026-42945 signature), got: {body}"
    );
    assert!(
        !body.contains("captured=foo+bar\n"),
        "capture should NOT round-trip cleanly on a vulnerable build, got: {body}"
    );
    // The buffer-overrun signature: the value is shorter than the
    // expanded escape would require. `foo%2Bbar` is 9 bytes; the
    // vulnerable build only writes 7. Allow some slack across versions
    // by asserting the value is shorter than the fully-escaped form.
    let captured = body
        .strip_prefix("captured=")
        .and_then(|s| s.strip_suffix('\n'))
        .unwrap_or("");
    assert!(
        captured.len() < "foo%2Bbar".len(),
        "expected buffer-overrun truncation: captured value `{captured}` \
         should be shorter than fully-escaped `foo%2Bbar` (9 bytes); \
         got length {}",
        captured.len()
    );
}

#[tokio::test]
#[ignore]
async fn vulnerable_rewrite_then_rewrite_misescapes_capture() {
    // The rule flags `rewrite (with ?) ... rewrite (using $N)` in the
    // same scope as well as the `set` shape. Verify the second-rewrite
    // form also produces the leaked-is_args symptom on a vulnerable
    // build, proving that flagging the rewrite-rewrite shape is not a
    // false positive.
    if skip_on_patched_version() {
        eprintln!("Skipping: nginx >= 1.31 has the CVE-2026-42945 fix");
        return;
    }
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(vulnerable_rewrite_rewrite_config())
        .await;

    // /foo/foo+bar →
    //   1st rewrite: URI=/bar/foo+bar, args=x=1, is_args leaked to 1
    //   2nd rewrite: URI=/baz/<$1> where $1="foo+bar". Leaked is_args
    //                applies args-escaping → final=/baz/foo%2Bbar
    let resp = reqwest::get(nginx.url("/foo/foo+bar")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("%2B"),
        "expected leaked is_args to wrongly arg-escape `+` to `%2B` in the \
         second rewrite's replacement (CVE-2026-42945 signature in \
         rewrite-rewrite shape), got: {body}"
    );
    assert!(
        !body.contains("final=foo+bar"),
        "captured value should NOT pass through clean to the second \
         rewrite's replacement on a vulnerable build, got: {body}"
    );
}

// ============================================================================
// Both recommended fixes remove the bug's signature on the same build
// ============================================================================
//
// Critical: these tests use the *vulnerable* nginx image (no version gate).
// They prove that the rule's suggested remediations actually neutralise the
// bug — not just that they happen to compile.

#[tokio::test]
#[ignore]
async fn safe_no_question_mark_removes_signature() {
    if skip_on_patched_version() {
        eprintln!("Skipping: on patched versions the vulnerable form also lacks the signature");
        return;
    }
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(safe_no_question_mark_config())
        .await;

    let resp = reqwest::get(nginx.url("/api/foo+bar")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("captured=foo+bar"),
        "removing `?` must keep the capture intact, got: {body}"
    );
    assert!(
        !body.contains("%2B"),
        "removing `?` must suppress the arg-escape leak, got: {body}"
    );
}

#[tokio::test]
#[ignore]
async fn safe_named_capture_removes_signature() {
    if skip_on_patched_version() {
        eprintln!("Skipping: on patched versions the vulnerable form also lacks the signature");
        return;
    }
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(safe_named_capture_config())
        .await;

    let resp = reqwest::get(nginx.url("/api/foo+bar")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("captured=foo+bar"),
        "named-capture variant must keep the capture intact, got: {body}"
    );
    assert!(
        !body.contains("%2B"),
        "named-capture variant must suppress the arg-escape leak, got: {body}"
    );
}

// ============================================================================
// Both recommended fixes still produce a working server
// ============================================================================
//
// Runs on every version. Smoke-checks that ordinary traffic continues to
// route correctly through the remediated configs.

#[tokio::test]
#[ignore]
async fn safe_no_question_mark_serves_request() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(safe_no_question_mark_config())
        .await;

    let resp = reqwest::get(nginx.url("/api/users")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("captured=users"),
        "no-question-mark config must still capture, got: {body}"
    );
}

#[tokio::test]
#[ignore]
async fn safe_named_capture_config_serves_request() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(safe_named_capture_config())
        .await;

    let resp = reqwest::get(nginx.url("/api/users")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("captured=users"),
        "named-capture config must still capture, got: {body}"
    );
}
