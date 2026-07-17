//! Container-based integration tests for the map-unnamed-capture rule
//! (nginx CVE-2026-42533, "stale regex captures").
//!
//! These tests prove the rule targets a *real* nginx bug by exercising the
//! flagged pattern against a vulnerable nginx and observing the bug's
//! signature — a deterministic worker SIGSEGV — directly from the master's
//! error log.
//!
//! # The bug
//!
//! nginx keeps regex captures in a shared `r->captures` array sized by
//! `r->ncaptures`. When a *capturing* `map` regex is (re-)evaluated inside a
//! cloned subrequest and does NOT match, `ngx_http_regex_exec()` reallocates
//! `r->captures` but — before the fix — leaves `r->ncaptures` at its previous
//! value. A later regex that reads an unnamed positional capture (`$1`..`$9`)
//! then indexes into the freshly-allocated, uninitialized array, reading
//! garbage offsets → out-of-bounds read → worker crash (potential RCE on
//! affected builds). Fixed upstream by commit `0cca8e055a2d` which adds
//! `r->ncaptures = 0;` at the realloc site.
//!
//! # The exact trigger (all three are required)
//!
//! 1. A `map` with a **capturing** regex entry that **mismatches** at runtime
//!    (`~mismatch(.*) 1;`). A capture group — named or unnamed — is what makes
//!    nginx reallocate `r->captures`.
//! 2. The map is **`volatile`**, so it is re-evaluated inside the subrequest
//!    (a non-volatile map is evaluated and cached in the main request and
//!    never re-runs in the subrequest, so it never reallocates there).
//! 3. A **cloned** subrequest: among stock modules only `slice`
//!    (`NGX_HTTP_SUBREQUEST_CLONE`) shares/reallocates the capture array, and
//!    an **unnamed** `$1` is read while that stale `ncaptures` is live.
//!
//! # Why named captures are the fix (and the rule's remediation)
//!
//! Named captures (`(?<name>...)`) resolve through `r->variables`, a code
//! path that never touches the `if (n < r->ncaptures)` uninitialized read.
//! This matches the vendor mitigation: "do not use unnamed captures; use
//! named captures instead and only use them in the same block with the regex
//! match." The mitigation is config-wide, not map-only: naming *just* the
//! map's capture while a consumer still reads an unnamed `$1` does not
//! reliably remove the crash (it still reallocates the array, and the crash
//! then depends on heap layout — observed still crashing on 1.31.2). The
//! [`safe_config`] here applies the mitigation fully — named capture in the
//! map *and* in the consuming `location` — which is clean on every affected
//! build.
//!
//! # Version window
//!
//! The bug is fixed in **1.30.4** and **1.31.3** (verified by file content
//! per release tag; backports are cherry-picks, so commit-ancestry checks
//! mislead). With the padded request [`hammer`] sends, this self-contained
//! repro crashes deterministically on **1.29.x, 1.30.0–1.30.3, and
//! 1.31.0–1.31.2** (8/8 across runs); it is not observable on 1.27/1.28
//! (pre-trigger). The crash tests gate on that observable window, detected
//! from `nginx -v`.
//!
//! Run with (default image is intentionally a vulnerable one):
//!   NGINX_IMAGE=nginx:1.29 cargo test -p map-unnamed-capture-plugin \
//!       --test container_test -- --ignored

use nginx_lint_plugin::container_testing::{NginxContainer, reqwest};

/// Backend document root file the `slice`/`proxy_pass` upstream serves.
/// 200 bytes so `slice 50;` produces several range subrequests.
const BACKEND_FILE: &str = "/usr/share/nginx/html/big.txt";

/// Vulnerable pattern: a `volatile` map whose regex entry uses an **unnamed**
/// capture and mismatches, feeding a `slice` location that reads `$1`.
/// This is exactly the shape the lint rule flags.
fn vulnerable_config() -> &'static str {
    r#"events {
    worker_connections 1024;
}
error_log /tmp/nginx-err.log info;
http {
    map test $my_map {
        volatile;
        ~mismatch(.*)  1;
        default        "";
    }

    server {
        listen 8081;
        location / {
            root /usr/share/nginx/html;
        }
    }

    server {
        listen 80;

        location = /healthz {
            return 200 "ok";
        }

        location ~(.*) {
            slice 50;
            proxy_set_header Test  "[$my_map|$1]";
            proxy_set_header Range $slice_range;
            proxy_pass http://127.0.0.1:8081;
        }
    }
}
"#
}

/// The vendor mitigation applied fully: named captures in **both** the map
/// (`(.*)` → `(?<x>.*)`) and the consuming `location` (`~(.*)` → `~(?<p>.*)`,
/// read as `$p` instead of `$1`). Named captures resolve through a path that
/// never touches the stale-`ncaptures` read, so the crash disappears on every
/// affected build. (Naming only the map is not enough — see the module docs.)
fn safe_config() -> &'static str {
    r#"events {
    worker_connections 1024;
}
error_log /tmp/nginx-err.log info;
http {
    map test $my_map {
        volatile;
        ~mismatch(?<x>.*)  1;
        default            "";
    }

    server {
        listen 8081;
        location / {
            root /usr/share/nginx/html;
        }
    }

    server {
        listen 80;

        location = /healthz {
            return 200 "ok";
        }

        location ~(?<p>.*) {
            slice 50;
            proxy_set_header Test  "[$my_map|$p]";
            proxy_set_header Range $slice_range;
            proxy_pass http://127.0.0.1:8081;
        }
    }
}
"#
}

/// The same unnamed-capture map and `$1`-reading `slice` location as
/// [`vulnerable_config`], with **only** `volatile;` removed.
///
/// This stays clean, but note carefully *why* — it is a property of the `slice`
/// path, not of non-volatile maps in general. Under `slice` the main request
/// proxies the first slice itself, so it evaluates `$my_map` and caches it
/// before any clone exists; the clones then read the cached value and never
/// run the regex. Change the clone source to `proxy_cache_background_update`
/// and the same non-volatile map *does* crash — see
/// [`non_volatile_background_update_config`]. Do not read this test as
/// "non-volatile is safe".
fn non_volatile_config() -> &'static str {
    r#"events {
    worker_connections 1024;
}
error_log /tmp/nginx-err.log info;
http {
    map test $my_map {
        ~mismatch(.*)  1;
        default        "";
    }

    server {
        listen 8081;
        location / {
            root /usr/share/nginx/html;
        }
    }

    server {
        listen 80;

        location = /healthz {
            return 200 "ok";
        }

        location ~(.*) {
            slice 50;
            proxy_set_header Test  "[$my_map|$1]";
            proxy_set_header Range $slice_range;
            proxy_pass http://127.0.0.1:8081;
        }
    }
}
"#
}

/// A **non-volatile** map with an unnamed capture, reached through the *other*
/// stock `NGX_HTTP_SUBREQUEST_CLONE` caller: `proxy_cache_background_update`
/// (`ngx_http_upstream.c:1157`, gated at `:941` by `proxy_cache_use_stale
/// updating` + `proxy_cache_background_update on`).
///
/// The hypothesis this config tests: `volatile` is not actually required. What
/// the bug needs is for the map's regex to *execute* while `realloc_captures`
/// is set — i.e. inside the clone. `volatile` guarantees that by disabling
/// caching, but a non-volatile map reaches it too if its **first** evaluation
/// happens inside the clone, because `sr->variables = r->variables`
/// (`ngx_http_core_module.c:2508`) makes the variable cache shared, not copied.
///
/// On a stale cache hit nginx creates the background clone during
/// `ngx_http_file_cache_open()` — *before* `create_request` — and then answers
/// the main request straight from cache, so the main request never evaluates
/// `proxy_set_header`. That leaves the background subrequest as the first (and
/// only) evaluator of `$my_map`, with `realloc_captures` already set.
///
/// (This is why the `slice` config above stays clean without `volatile`: there
/// the main request proxies the first slice itself, so it evaluates and caches
/// the map before any clone exists.)
fn non_volatile_background_update_config() -> &'static str {
    r#"events {
    worker_connections 1024;
}
error_log /tmp/nginx-err.log info;
http {
    proxy_cache_path /tmp/cache levels=1:2 keys_zone=z:10m inactive=60m;

    log_format probe '$request_uri test=[$http_test]';

    map test $my_map {
        ~mismatch(.*)  1;
        default        "";
    }

    server {
        listen 8081;
        access_log /tmp/backend.log probe;
        location / {
            root /usr/share/nginx/html;
        }
    }

    server {
        listen 80;

        location = /healthz {
            return 200 "ok";
        }

        location ~(.*) {
            proxy_cache                     z;
            proxy_cache_valid               200 1s;
            proxy_cache_use_stale           updating;
            proxy_cache_background_update   on;
            add_header X-Cache $upstream_cache_status always;
            proxy_set_header Test  "[$my_map|$1]";
            proxy_pass http://127.0.0.1:8081;
        }
    }
}
"#
}

/// Exactly what the rule's autofix produces from [`vulnerable_config`]: the
/// map's `(.*)` becomes `(?:.*)`, and **nothing else changes** — the consuming
/// `location ~(.*)` still reads an unnamed `$1`. (The map's value is `1`, which
/// reads no positional capture, so the autofix applies here.)
///
/// This is the sharpest test of the fix's premise. Naming *only* the map is
/// known NOT to be enough — a named group is still a capturing group, so
/// `ngx_http_regex_exec()` still reallocates and the crash survives on 1.31.2.
/// Going non-capturing instead makes `re->ncaptures` zero, so the realloc never
/// happens for this regex at all, and the crash should disappear even with the
/// unnamed `$1` consumer left untouched.
fn autofixed_config() -> &'static str {
    r#"events {
    worker_connections 1024;
}
error_log /tmp/nginx-err.log info;
http {
    map test $my_map {
        volatile;
        ~mismatch(?:.*)  1;
        default          "";
    }

    server {
        listen 8081;
        location / {
            root /usr/share/nginx/html;
        }
    }

    server {
        listen 80;

        location = /healthz {
            return 200 "ok";
        }

        location ~(.*) {
            slice 50;
            proxy_set_header Test  "[$my_map|$1]";
            proxy_set_header Range $slice_range;
            proxy_pass http://127.0.0.1:8081;
        }
    }
}
"#
}

/// Parse `major.minor.patch` and the engine prefix out of `nginx -v` stderr,
/// e.g. `"nginx version: nginx/1.29.8"` → `("nginx", (1, 29, 8))`.
fn parse_nginx_version(stderr: &str) -> Option<(String, (u32, u32, u32))> {
    let after = stderr.split('/').nth(1)?;
    let ver = after.split_whitespace().next()?;
    let engine = stderr
        .split(':')
        .nth(1)?
        .trim()
        .split('/')
        .next()?
        .to_string();
    let mut it = ver.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().unwrap_or(0);
    Some((engine, (major, minor, patch)))
}

/// Whether the deterministic crash is observable on the running build. True
/// for stock nginx in the confirmed vulnerable window: 1.29.x, 1.30.0–1.30.3,
/// and 1.31.0–1.31.2. Everything else (patched 1.30.4+/1.31.3+, pre-trigger
/// 1.27/1.28, non-nginx engines) is skipped.
async fn crash_observable(nginx: &NginxContainer) -> bool {
    let out = nginx.exec(&["nginx", "-v"]).await;
    let combined = format!("{}{}", out.stdout, out.stderr);
    match parse_nginx_version(&combined) {
        Some((engine, _)) if engine != "nginx" => false,
        Some((_, (1, 29, _))) => true,
        Some((_, (1, 30, patch))) => patch <= 3,
        Some((_, (1, 31, patch))) => patch <= 2,
        _ => false,
    }
}

/// Create the upstream document the `slice` subrequests fetch. 200 'A' bytes.
async fn seed_backend_file(nginx: &NginxContainer) {
    let out = nginx
        .exec_shell(&format!("yes A | head -200 | tr -d '\\n' > {BACKEND_FILE}"))
        .await;
    assert_eq!(out.exit_code, 0, "failed to seed backend file: {out:?}");
}

/// Count `worker process ... exited on signal` lines in the master's error log.
async fn worker_crash_count(nginx: &NginxContainer) -> u32 {
    let out = nginx
        .exec_shell("grep -ac 'exited on signal' /tmp/nginx-err.log")
        .await;
    out.stdout.trim().parse().unwrap_or(0)
}

/// Drive the vulnerable path a handful of times. A long query string loads
/// non-zero bytes into the request pool so the stale-`ncaptures` read lands on
/// garbage offsets — this single request shape crashes every vulnerable build
/// (1.29.x, 1.30.0–1.30.3, 1.31.0–1.31.2) deterministically. The requests that
/// crash the worker fail at the connection level, so errors are expected and
/// ignored — the crash itself is asserted from the error log, not the client.
async fn hammer(nginx: &NginxContainer) {
    let query = "D".repeat(800);
    for _ in 0..8 {
        let _ = reqwest::get(nginx.url(&format!("/big.txt?q={query}"))).await;
    }
}

// ============================================================================
// The flagged config is valid nginx (runs on every version)
// ============================================================================

#[tokio::test]
#[ignore]
async fn vulnerable_config_is_valid_nginx() {
    // The harness only returns once `/healthz` answers 200, so a successful
    // start already proves the flagged pattern is a config nginx accepts —
    // the rule is not flagging something nginx would reject at load time.
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(vulnerable_config())
        .await;

    let resp = reqwest::get(nginx.url("/healthz")).await.unwrap();
    assert_eq!(resp.status(), 200, "vulnerable-pattern config must load");
}

// ============================================================================
// On a vulnerable build, the flagged pattern crashes the worker
// ============================================================================

#[tokio::test]
#[ignore]
async fn unnamed_map_capture_crashes_worker() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(vulnerable_config())
        .await;

    if !crash_observable(&nginx).await {
        eprintln!(
            "Skipping: crash not observable on this nginx build \
             (need 1.29.x / 1.30.0-1.30.3 / 1.31.0-1.31.2)"
        );
        return;
    }

    seed_backend_file(&nginx).await;
    hammer(&nginx).await;

    let crashes = worker_crash_count(&nginx).await;
    assert!(
        crashes > 0,
        "expected the unnamed-capture map + slice + $1 pattern to crash a \
         worker (CVE-2026-42533 stale-captures signature), but saw no \
         `exited on signal` lines"
    );
}

// ============================================================================
// The full mitigation removes the signature on the same vulnerable build
// ============================================================================
//
// Critical: same vulnerable image, no version gate beyond observability.
// Proves the remediation (named captures, applied to the map and its
// consumer) actually neutralises the bug rather than merely compiling.

#[tokio::test]
#[ignore]
async fn named_captures_remove_crash() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(safe_config())
        .await;

    if !crash_observable(&nginx).await {
        eprintln!("Skipping: crash not observable on this nginx build; nothing to neutralise");
        return;
    }

    seed_backend_file(&nginx).await;
    hammer(&nginx).await;

    let crashes = worker_crash_count(&nginx).await;
    assert_eq!(
        crashes, 0,
        "using named captures in the map and its consumer must remove the \
         stale-captures crash on a vulnerable build, but saw {crashes} worker \
         crash(es)"
    );
}

// ============================================================================
// Under `slice` specifically, dropping `volatile` removes the crash
// ============================================================================
//
// Same vulnerable image, same unnamed capture, same `slice` + unnamed `$1`
// consumer — only `volatile;` is gone. This pins down the mechanism (the map
// must actually execute inside the clone), but it is NOT a licence to gate the
// lint rule on `volatile`: the background-update test below crashes with a
// non-volatile map.

#[tokio::test]
#[ignore]
async fn non_volatile_map_does_not_crash() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(non_volatile_config())
        .await;

    if !crash_observable(&nginx).await {
        eprintln!("Skipping: crash not observable on this nginx build; nothing to compare against");
        return;
    }

    seed_backend_file(&nginx).await;
    hammer(&nginx).await;

    let crashes = worker_crash_count(&nginx).await;
    assert_eq!(
        crashes, 0,
        "under `slice` the main request evaluates and caches the map before any \
         clone exists, so a non-volatile map must not crash here, but saw \
         {crashes} crash(es)"
    );
}

// ============================================================================
// The rule's autofix neutralises the bug on its own
// ============================================================================

#[tokio::test]
#[ignore]
async fn autofixed_config_removes_crash() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(autofixed_config())
        .await;

    if !crash_observable(&nginx).await {
        eprintln!("Skipping: crash not observable on this nginx build; nothing to neutralise");
        return;
    }

    seed_backend_file(&nginx).await;
    hammer(&nginx).await;

    let crashes = worker_crash_count(&nginx).await;
    assert_eq!(
        crashes, 0,
        "making the map's group non-capturing must remove the crash even with \
         the unnamed `$1` consumer untouched (this is what the autofix relies \
         on), but saw {crashes} worker crash(es)"
    );
}

#[tokio::test]
#[ignore]
async fn autofixed_config_serves_request() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(autofixed_config())
        .await;

    seed_backend_file(&nginx).await;

    let resp = reqwest::get(nginx.url("/big.txt")).await.unwrap();
    assert!(
        resp.status().is_success(),
        "autofixed config must still serve the proxied file, got {}",
        resp.status()
    );
}

// ============================================================================
// Does the bug need `volatile` at all? (background-update clone path)
// ============================================================================

#[tokio::test]
#[ignore]
async fn non_volatile_map_via_background_update() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(non_volatile_background_update_config())
        .await;

    if !crash_observable(&nginx).await {
        eprintln!("Skipping: crash not observable on this nginx build");
        return;
    }

    seed_backend_file(&nginx).await;

    let query = "D".repeat(800);
    let path = format!("/big.txt?q={query}");

    // Populate the cache, let the entry go stale, then hit it again: the stale
    // hit is what spawns the background clone.
    let mut statuses = Vec::new();
    for _ in 0..8 {
        let _ = reqwest::get(nginx.url(&path)).await;
        nginx.exec_shell("sleep 2").await;
        if let Ok(resp) = reqwest::get(nginx.url(&path)).await {
            let status = resp
                .headers()
                .get("x-cache")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("<none>")
                .to_string();
            statuses.push(status);
        }
    }
    nginx.exec_shell("sleep 1").await;

    // Guard against a vacuous pass: if the second request never reports STALE
    // or UPDATING, the background clone was never created and this config
    // proves nothing about the hypothesis.
    eprintln!("cache statuses on the second request: {statuses:?}");
    assert!(
        statuses.iter().any(|s| s == "STALE" || s == "UPDATING"),
        "background-update path was never exercised (statuses: {statuses:?}) — \
         this test would pass vacuously"
    );

    // What the upstream actually received tells us whether the background
    // clone evaluated the map at all, and what `$1` resolved to there.
    let backend = nginx.exec_shell("cut -c1-60 /tmp/backend.log").await;
    eprintln!("--- backend saw ---\n{}\n---", backend.stdout);

    let crashes = worker_crash_count(&nginx).await;
    assert!(
        crashes > 0,
        "expected a NON-volatile map to crash a worker through the \
         background-update clone path (this is what proves the lint rule must \
         not gate on `volatile`), but saw no `exited on signal` lines"
    );
}

// ============================================================================
// The fixed config still serves normal traffic (runs on every version)
// ============================================================================

#[tokio::test]
#[ignore]
async fn safe_config_serves_request() {
    let nginx = NginxContainer::builder()
        .health_path("/healthz")
        .start(safe_config())
        .await;

    seed_backend_file(&nginx).await;

    let resp = reqwest::get(nginx.url("/big.txt")).await.unwrap();
    assert!(
        resp.status().is_success(),
        "safe config must still serve the proxied file, got {}",
        resp.status()
    );
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn parses_stock_nginx_version() {
        let (engine, v) = parse_nginx_version("nginx version: nginx/1.29.8").unwrap();
        assert_eq!(engine, "nginx");
        assert_eq!(v, (1, 29, 8));
    }

    #[test]
    fn parses_two_component_version() {
        let (_, v) = parse_nginx_version("nginx version: nginx/1.31").unwrap();
        assert_eq!(v, (1, 31, 0));
    }

    #[test]
    fn parses_openresty_engine() {
        let (engine, v) = parse_nginx_version("nginx version: openresty/1.29.2.4").unwrap();
        assert_eq!(engine, "openresty");
        assert_eq!(v, (1, 29, 2));
    }
}
