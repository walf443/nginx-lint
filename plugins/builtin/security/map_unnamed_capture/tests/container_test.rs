//! Container-based integration tests for the map-unnamed-capture rule
//! (nginx CVE-2026-42533, "stale regex captures").
//!
//! These tests prove the rule targets a *real* nginx bug: they run the flagged
//! pattern against real nginx and show that it breaks on affected builds and
//! works on fixed ones, with the same config either way.
//!
//! # The bug
//!
//! nginx keeps regex captures in a shared `r->captures` array sized by
//! `r->ncaptures`. When a *capturing* `map` regex runs inside a cloned
//! subrequest and does NOT match, `ngx_http_regex_exec()` reallocates
//! `r->captures` but — before the fix — leaves `r->ncaptures` at its previous
//! value and returns early. A later read of an unnamed positional capture
//! (`$1`..`$9`) then indexes into the freshly-allocated, uninitialized array,
//! reading garbage offsets — an out-of-bounds read (potential RCE on affected
//! builds). Fixed upstream by commit `0cca8e055a2d`, which adds
//! `r->ncaptures = 0;` at the realloc site.
//!
//! # What these tests observe, and why not the crash
//!
//! An earlier version asserted a worker SIGSEGV. That was a mistake: whether
//! the out-of-bounds read faults depends on what the uninitialized memory
//! happens to hold, which varies by CPU architecture. The same repro crashes
//! 8/8 on arm64 and never crashes on x86_64 — so a crash assertion passes
//! locally on an Apple-silicon laptop and fails in x86_64 CI.
//!
//! What *is* deterministic on both architectures is that the subrequest breaks.
//! Reading the stale capture either faults (arm64) or makes nginx fail the
//! subrequest internally — `unexpected status code 500 in slice response`
//! (x86_64). Either way the client request fails and the upstream never sees
//! the subrequest. On a fixed build `$1` is empty instead, the subrequest is
//! well-formed, and the upstream receives it.
//!
//! So the signature is taken from two places that agree across architectures:
//!   - the client: every request fails on an affected build, succeeds on a
//!     fixed one;
//!   - the upstream's access log: `[|]` (an empty `$1`, i.e. the fix's
//!     `ncaptures = 0`) appears only on fixed builds.
//!
//! Measured 8/8 across {arm64, x86_64} × {1.29, 1.30.3, 1.30.4, 1.31.3}, and
//! the 1.30.3 → 1.30.4 boundary lands exactly on the upstream fix.
//!
//! This is not a universal oracle, and the asymmetry matters: a *failure* is
//! proof of the bug, but a *clean run* only means the stale read happened to
//! land on zeroed memory. Known builds where that happens despite the bug being
//! present are skipped rather than declared fixed — see [`is_stock_nginx`].
//!
//! # The trigger
//!
//! 1. A `map` with a **capturing** regex entry that **mismatches** at runtime
//!    (`~mismatch(.*) 1;`). The capture group — named or unnamed — is what
//!    makes nginx reallocate `r->captures`.
//! 2. The map regex must actually *run* inside the clone. `volatile` (used
//!    here) guarantees that by disabling caching. It is not required in
//!    general — see the rule's docs — but it is what makes this repro
//!    deterministic.
//! 3. A **cloned** subrequest (`NGX_HTTP_SUBREQUEST_CLONE`) — here `slice` —
//!    with an **unnamed** `$1` read while the stale `ncaptures` is live.
//!
//! # Version window
//!
//! Fixed in **1.30.4** and **1.31.3** (verified by file content per release
//! tag; backports are cherry-picks, so commit-ancestry checks mislead). Not
//! observable on 1.27/1.28. [`is_affected_build`] gates on that window.
//!
//! Run with (default image is intentionally a vulnerable one):
//!   NGINX_IMAGE=nginx:1.29 cargo test -p map-unnamed-capture-plugin \
//!       --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{
    NginxContainer, nginx_html_root, nginx_server_name, reqwest,
};

/// Upstream document the `slice` subrequests fetch. 200 bytes, so `slice 50;`
/// produces one main-request range plus three subrequest ranges.
fn backend_file() -> String {
    format!("{}/big.txt", nginx_html_root())
}

/// Shared prologue: an upstream on :8081 that logs the `Test` header it
/// receives, which is where the capture value becomes observable.
fn config_with(map_entry: &str, location: &str) -> String {
    format!(
        r#"events {{
    worker_connections 1024;
}}
error_log /tmp/nginx-err.log info;
http {{
    log_format capture_probe 'T=[$http_test]';

    map test $my_map {{
        volatile;
        {map_entry}
        default        "";
    }}

    server {{
        listen 8081;
        access_log /tmp/backend.log capture_probe;
        location / {{
            root {root};
        }}
    }}

    server {{
        listen 80;

        location = /healthz {{
            return 200 "ok";
        }}

        {location}
    }}
}}
"#,
        root = nginx_html_root(),
    )
}

/// Vulnerable pattern: a map regex with an **unnamed** capture that mismatches,
/// feeding a `slice` location that reads `$1`. Exactly the shape the rule flags.
fn vulnerable_config() -> String {
    config_with(
        r#"~mismatch(.*)  1;"#,
        r#"location ~(.*) {
            slice 50;
            proxy_set_header Test  "[$my_map|$1]";
            proxy_set_header Range $slice_range;
            proxy_pass http://127.0.0.1:8081;
        }"#,
    )
}

/// The vendor mitigation applied fully: named captures in **both** the map and
/// the consuming `location`, read by name. Naming only the map is NOT enough —
/// a named group is still a capturing group, so the array is still reallocated.
fn safe_config() -> String {
    config_with(
        r#"~mismatch(?<x>.*)  1;"#,
        r#"location ~(?<p>.*) {
            slice 50;
            proxy_set_header Test  "[$my_map|$p]";
            proxy_set_header Range $slice_range;
            proxy_pass http://127.0.0.1:8081;
        }"#,
    )
}

/// Exactly what the rule's autofix produces from [`vulnerable_config`]: the
/// map's `(.*)` becomes `(?:.*)` and **nothing else changes** — the consuming
/// `location ~(.*)` still reads an unnamed `$1`. (The map's value is `1`, which
/// reads no positional capture, so the autofix applies here.)
///
/// This is the sharpest test of the fix's premise: `ngx_http_regex_exec()`
/// reallocates only `if (re->ncaptures)`, so a non-capturing group stops the
/// realloc outright, whereas a named capture would not.
fn autofixed_config() -> String {
    config_with(
        r#"~mismatch(?:.*)  1;"#,
        r#"location ~(.*) {
            slice 50;
            proxy_set_header Test  "[$my_map|$1]";
            proxy_set_header Range $slice_range;
            proxy_pass http://127.0.0.1:8081;
        }"#,
    )
}

/// Minimal config that loads on every engine, used only to get a container up
/// so its build options can be read.
const PROBE_CONFIG: &str = r#"events {
    worker_connections 1024;
}
http {
    server {
        listen 80;
        location = /healthz {
            return 200 "ok";
        }
    }
}
"#;

/// Whether the image was built with `--with-http_slice_module`.
///
/// This repro needs `slice` to create the cloned subrequest, and slice is NOT a
/// nginx build default (`auto/options`: `HTTP_SLICE=NO`) — it is opt-in. The
/// official nginx and openresty images enable it; the freenginx image used in
/// CI does not, and without it `slice 50;` is an unknown directive, so nginx
/// refuses the whole config and never starts. Detect it rather than hardcoding
/// which images qualify, so a rebuilt image is picked up automatically.
///
/// Costs one throwaway container: the real config cannot load on an image
/// lacking the module, so this cannot be asked of the container under test.
///
/// Note this says nothing about the *rule's* scope. The other stock path into
/// this bug, `proxy_cache_background_update`, needs no optional module — only
/// the repro depends on slice.
async fn has_slice_module() -> bool {
    let probe = NginxContainer::builder()
        .health_path("/healthz")
        .start(PROBE_CONFIG)
        .await;

    let out = probe.exec(&["nginx", "-V"]).await;
    format!("{}{}", out.stdout, out.stderr).contains("http_slice_module")
}

/// Parse `major.minor.patch` out of `nginx -v` output.
fn parse_nginx_version(output: &str) -> Option<(u32, u32, u32)> {
    let ver = output.split('/').nth(1)?.split_whitespace().next()?;
    let mut it = ver.split('.');
    Some((
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next().unwrap_or("0").parse().unwrap_or(0),
    ))
}

/// Whether the running engine is stock nginx, the only one this repro can
/// actually judge.
///
/// Forks are skipped, and NOT because their fix status is unknown — as of
/// 2026-07-17 both carry the bug: openresty 1.31.1.1 bundles nginx 1.31.1 with
/// no `r->ncaptures = 0;` at the realloc site (its patch set touches only
/// OpenSSL), and freenginx 1.31.3 is likewise missing it.
///
/// They are skipped because **this repro cannot detect that**. On both, the
/// stale read lands on zeroed memory, so `$1` comes through empty and the
/// subrequest succeeds — outwardly identical to a fixed build. That is the
/// honest limit of the signature: it proves the bug when the stale read hits
/// non-zero memory (as it does on every official nginx image tested), but a
/// clean run is not evidence of a fix. Asserting the fixed behaviour here would
/// vouch for builds that are in fact vulnerable.
fn is_stock_nginx() -> bool {
    nginx_server_name() == "nginx"
}

/// Whether the bug is present on the running build: the confirmed window
/// 1.29.x / 1.30.0-1.30.3 / 1.31.0-1.31.2. Only meaningful for stock nginx.
async fn is_affected_build(nginx: &NginxContainer) -> bool {
    let out = nginx.exec(&["nginx", "-v"]).await;
    match parse_nginx_version(&format!("{}{}", out.stdout, out.stderr)) {
        Some((1, 29, _)) => true,
        Some((1, 30, patch)) => patch <= 3,
        Some((1, 31, patch)) => patch <= 2,
        _ => false,
    }
}

async fn seed_backend_file(nginx: &NginxContainer) {
    let out = nginx
        .exec_shell(&format!(
            "yes A | head -200 | tr -d '\\n' > {}",
            backend_file()
        ))
        .await;
    assert_eq!(out.exit_code, 0, "failed to seed backend file: {out:?}");
}

/// Outcome of driving the vulnerable path: what the client saw, and what the
/// upstream received.
struct Observed {
    client_ok: usize,
    client_failed: usize,
    /// Upstream requests carrying an **empty** `$1` — the fix's `ncaptures = 0`
    /// showing through. Only fixed builds produce these.
    empty_capture_hits: u32,
    /// Upstream requests carrying the location's real `$1` value. The 8 main
    /// requests always do; subrequests only when the captures stayed intact.
    real_capture_hits: u32,
}

/// Drive the flagged path a handful of times and collect both signals.
///
/// The long query string is what pushes the stale read onto non-zero bytes on
/// affected builds; without it the read can land on zeroed memory and look
/// indistinguishable from a fixed build.
async fn drive(nginx: &NginxContainer, capture_value: &str) -> Observed {
    let query = "D".repeat(800);
    let (mut client_ok, mut client_failed) = (0, 0);

    for _ in 0..8 {
        match reqwest::get(nginx.url(&format!("/big.txt?q={query}"))).await {
            Ok(resp) if resp.status().is_success() => client_ok += 1,
            _ => client_failed += 1,
        }
    }

    Observed {
        client_ok,
        client_failed,
        empty_capture_hits: count_upstream_hits(nginx, "T=[[|]]").await,
        real_capture_hits: count_upstream_hits(nginx, &format!("T=[[|{capture_value}]]")).await,
    }
}

async fn count_upstream_hits(nginx: &NginxContainer, needle: &str) -> u32 {
    let out = nginx
        .exec_shell(&format!("grep -acF '{needle}' /tmp/backend.log"))
        .await;
    out.stdout.trim().parse().unwrap_or(0)
}

// ============================================================================
// The flagged config is valid nginx (runs on every version)
// ============================================================================

#[tokio::test]
#[ignore]
async fn vulnerable_config_is_valid_nginx() {
    if !has_slice_module().await {
        eprintln!("SKIP vulnerable_config_is_valid_nginx: image lacks http_slice_module");
        return;
    }

    // The harness only returns once `/healthz` answers 200, so a successful
    // start already proves the flagged pattern is a config nginx accepts — the
    // rule is not flagging something nginx would reject at load time.
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(vulnerable_config().as_str())
        .await;

    let resp = reqwest::get(nginx.url("/healthz")).await.unwrap();
    assert_eq!(resp.status(), 200, "vulnerable-pattern config must load");
}

// ============================================================================
// The flagged pattern breaks on affected builds, and only on affected builds
// ============================================================================

#[tokio::test]
#[ignore]
async fn unnamed_map_capture_breaks_the_slice_subrequest() {
    if !is_stock_nginx() {
        eprintln!(
            "Skipping: fix status not established for {}",
            nginx_server_name()
        );
        return;
    }

    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(vulnerable_config().as_str())
        .await;

    seed_backend_file(&nginx).await;
    let seen = drive(&nginx, "/big.txt").await;

    assert_eq!(
        seen.real_capture_hits, 8,
        "expected all 8 main requests to reach the upstream regardless of \
         build — without them this test would prove nothing about the \
         subrequests"
    );

    if is_affected_build(&nginx).await {
        assert_eq!(
            seen.client_ok, 0,
            "on an affected build every request must fail: reading the stale \
             capture either faults or makes nginx fail the slice subrequest \
             internally"
        );
        assert_eq!(
            seen.empty_capture_hits, 0,
            "an affected build must never deliver an empty `$1` — that is the \
             fixed behaviour (`ncaptures = 0`)"
        );
    } else {
        assert_eq!(
            seen.client_failed, 0,
            "on a fixed build the same config must serve every request"
        );
        assert!(
            seen.empty_capture_hits > 0,
            "a fixed build must reset `ncaptures`, so the slice subrequests \
             must reach the upstream with an empty `$1`"
        );
    }
}

// ============================================================================
// The remediations neutralise it on the same affected build
// ============================================================================
//
// Same image, no version gate beyond observability: these prove the fixes
// actually work rather than merely compiling.

#[tokio::test]
#[ignore]
async fn named_captures_keep_the_subrequest_working() {
    if !has_slice_module().await {
        eprintln!("SKIP named_captures_keep_the_subrequest_working: image lacks http_slice_module");
        return;
    }

    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(safe_config().as_str())
        .await;

    seed_backend_file(&nginx).await;
    let seen = drive(&nginx, "/big.txt").await;

    assert_eq!(
        seen.client_failed, 0,
        "naming the captures in the map AND its consumer must keep every \
         request working on an affected build"
    );
}

#[tokio::test]
#[ignore]
async fn autofix_keeps_the_subrequest_working() {
    if !has_slice_module().await {
        eprintln!("SKIP autofix_keeps_the_subrequest_working: image lacks http_slice_module");
        return;
    }

    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(autofixed_config().as_str())
        .await;

    seed_backend_file(&nginx).await;
    let seen = drive(&nginx, "/big.txt").await;

    // The consumer still reads an unnamed `$1`; only the map's group changed.
    // This works solely because a non-capturing group stops the realloc.
    assert_eq!(
        seen.client_failed, 0,
        "making the map's group non-capturing must fix the request even with \
         the unnamed `$1` consumer left untouched — this is what the autofix \
         relies on"
    );

    // Note the mechanism differs from the upstream fix, and so does what the
    // upstream sees. A fixed nginx still reallocates and resets the count, so
    // `$1` comes through empty; here the array is never reallocated, so `$1`
    // keeps the location's real capture in the subrequests too.
    assert_eq!(
        seen.empty_capture_hits, 0,
        "no realloc means `ncaptures` is never reset, so `$1` must not come \
         through empty"
    );
    assert_eq!(
        seen.real_capture_hits, 32,
        "all 8 requests × 4 slices must reach the upstream carrying the \
         location's real capture"
    );
}
