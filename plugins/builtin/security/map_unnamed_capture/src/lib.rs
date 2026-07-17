//! map-unnamed-capture plugin (nginx CVE-2026-42533)
//!
//! This plugin warns when a regex entry in a `map` block uses an unnamed
//! capture group `(...)`. A capturing `map` regex makes nginx reallocate its
//! shared capture array (`r->captures`); when that regex runs inside a cloned
//! subrequest (`NGX_HTTP_SUBREQUEST_CLONE`) and does not match, nginx — before
//! the fix — leaves the capture count (`r->ncaptures`) stale. A later read of
//! an unnamed positional capture (`$1`..`$9`) then indexes into the
//! uninitialized reallocated array, causing an out-of-bounds read that can
//! crash the worker (potential RCE on affected builds).
//!
//! # Why this is not gated on `volatile`
//!
//! An earlier version of this rule only flagged `volatile` maps, on the theory
//! that a cached map never re-runs inside the subrequest. That is wrong.
//! `volatile` *guarantees* the regex re-runs there, but the real precondition
//! is only that the regex executes while `realloc_captures` is set. Because
//! `ngx_http_subrequest()` shares the variable cache rather than copying it
//! (`sr->variables = r->variables`), a non-volatile map is equally exposed
//! whenever its **first** evaluation happens inside the clone.
//!
//! Two stock callers pass `NGX_HTTP_SUBREQUEST_CLONE`:
//! `ngx_http_slice_filter_module.c` and, via
//! `ngx_http_upstream_cache_background_update()`, `proxy_cache_background_update
//! on` (reached via `proxy_cache_use_stale updating`, or equally via
//! `stale-while-revalidate` from the upstream's own `Cache-Control`). Under
//! `slice` a non-volatile map happens to stay clean, but only because the main
//! request proxies the first slice itself and so evaluates and caches the map
//! before any clone exists — a property of that path, not of non-volatile
//! maps. Under `proxy_cache_background_update` the clone is created when
//! `ngx_http_upstream_cache()` sees `ngx_http_file_cache_open()` return
//! `NGX_HTTP_CACHE_STALE`, which is *before* `create_request`; the main
//! request then answers straight from cache without ever evaluating
//! `proxy_set_header` — leaving the background subrequest as the map's first
//! evaluator, with `realloc_captures` already set.
//!
//! A **non-volatile** map has been observed crashing a worker through that
//! background-update path (nginx 1.29, x86_64). It is not covered by a
//! container test: whether the stale read faults there depends on what the
//! uninitialized memory holds, and the outcome flips on unrelated config
//! details — a test asserting it passes or fails by luck. The rule's scope
//! rests on the structural facts above, which the tested `slice` path
//! demonstrates end-to-end. Gating on `volatile` would miss the more
//! mainstream of the two clone paths.
//!
//! The vendor mitigation is to use named captures instead of unnamed ones:
//! named captures resolve through a separate code path that never touches the
//! stale-`ncaptures` read. Fixed upstream in nginx 1.30.4 / 1.31.3 (commit
//! `0cca8e055a2d`, which resets `r->ncaptures = 0` at the realloc site).
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::helpers::find_unnamed_capture_positions;
use nginx_lint_plugin::prelude::*;

/// Check that map regex entries use named captures instead of `(...)`
#[derive(Default)]
pub struct MapUnnamedCapturePlugin;

impl Plugin for MapUnnamedCapturePlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "map-unnamed-capture",
            "security",
            "Warns when a map regex entry uses an unnamed capture group (CVE-2026-42533)",
        )
        .with_severity("warning")
        .with_why(
            "A capture group in a `map` regex makes nginx reallocate its shared capture array. On \
             builds before nginx 1.30.4 / 1.31.3 (CVE-2026-42533), when that regex runs inside a \
             cloned subrequest (`slice`, or the background subrequest of \
             `proxy_cache_background_update`) and does not match, the capture count is left stale; \
             a later read of an unnamed positional capture (`$1`..`$9`) then reads uninitialized \
             memory, causing an out-of-bounds read that can crash the worker or be leveraged for \
             remote code execution. This applies whether or not the map is `volatile` — the \
             variable cache is shared with subrequests, so a cached map is also exposed when its \
             first evaluation lands inside the clone. The fix is config-wide, not map-only: naming \
             the map's capture alone does NOT remove the crash, because a named group still \
             reallocates the array — every `location`/`if`/`rewrite` on the request path must stop \
             reading unnamed `$1`..`$9` too. So replace `(...)` with a named capture \
             `(?<name>...)` in the map AND reference captures by name (`$name`) in the blocks that \
             consume them; named captures resolve through a separate path that never touches the \
             stale-count read. Better still, use a non-capturing group `(?:...)` when the group is \
             only for grouping (as in `~*(bot|crawler)`): with no capture left the regex stops \
             reallocating at all, which is the one remediation that needs no change outside the \
             map. Scope warning: this rule reports only UNNAMED captures, so a map whose captures \
             are already named is never reported — even though it still reallocates and so still \
             arms the bug for any unnamed `$1`..`$9` read elsewhere on the request path. That is \
             not a rounding error: on a config with a named map capture and an unnamed `$1` \
             consumer, nginx 1.29 fails 100% of requests while this rule reports nothing at all \
             (measured). Silence from this rule therefore does not mean the config is safe; it \
             means no map regex has an unnamed capture. Detecting the rest needs the `$1`..`$9` \
             readers checked too, which this rule does not do.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_map_module.html".to_string(),
            "https://github.com/nginx/nginx/commit/0cca8e055a2d909f1a00c2071665b502ec2fe94c"
                .to_string(),
        ])
        // The vulnerability report says 0.9.6+, which is when `map` learned
        // regex entries — but the bug also needs a cloned subrequest to exist,
        // and `NGX_HTTP_SUBREQUEST_CLONE` arrived with the slice module in
        // 1.9.8 (`CHANGES`: "Changes with nginx 1.9.8 ... Feature: the
        // ngx_http_slice_module"). Nothing in `[0.9.6, 1.9.7]` can reach it.
        .with_min_version("1.9.8")
        // Inclusive upper bound. The fix shipped on two branches — stable
        // 1.30.4 and mainline 1.31.3 — so the affected set is
        // `[1.9.8, 1.30.3]` plus mainline `[1.31.0, 1.31.2]`, which a single
        // min..max interval cannot express exactly. We take `1.31.2` (the last
        // affected mainline release) rather than the stable fix point `1.30.3`:
        // for a security rule a false negative (staying silent on an affected
        // build) is worse than a false positive, and `1.31.2` keeps every
        // affected version in range. The only imprecision is that a
        // `target_nginx_version` on a fixed stable release (1.30.4..=1.31.2)
        // still gets the warning — harmless over-reporting.
        .with_max_version("1.31.2")
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();

        for ctx in config.all_directives_with_context() {
            if !ctx.directive.is("map") {
                continue;
            }

            // `stream` cannot reach this bug: it has no subrequests, and
            // `ngx_stream_regex_exec()` allocates `s->captures` only
            // `if (s->captures == NULL)` — there is no `realloc_captures`
            // equivalent, so the array is never reallocated and the count is
            // never left stale.
            //
            // Tested as `!is_inside("stream")` rather than `is_inside("http")`
            // on purpose: a snippet `include`d into `http` does not have the
            // `http` block in its own parse tree, and missing a real
            // vulnerability is worse than reporting one in a stray fragment.
            if ctx.is_inside("stream") {
                continue;
            }

            let Some(block) = ctx.directive.block.as_ref() else {
                continue;
            };

            // Deliberately NOT gated on `volatile`. `volatile` guarantees the
            // regex re-runs inside the clone, but it is not required: the
            // variable cache is shared with subrequests
            // (`sr->variables = r->variables`), so a cached map is also
            // vulnerable whenever its *first* evaluation lands inside the
            // clone. See the module docs for the evidence.
            for entry in block.directives() {
                let Some(pattern) = map_regex_pattern(&entry.name) else {
                    continue;
                };

                let positions = find_unnamed_capture_positions(pattern);
                if positions.is_empty() {
                    continue;
                }

                let mut error = err.warning_at(
                    "map regex uses an unnamed capture group (CVE-2026-42533); use `(?:...)` if \
                     the group only needs to group, otherwise use a named capture `(?<name>...)` \
                     here and read it by name (`$name`) in the blocks that consume it — naming it \
                     here alone is not enough",
                    entry,
                );

                if let Some(fixes) = non_capturing_fixes(entry, pattern, &positions) {
                    error = error.with_fixes(fixes);
                }

                errors.push(error);
            }
        }

        errors
    }
}

/// Fixes turning every unnamed group in this entry's regex into `(?:...)`, or
/// `None` when rewriting them would change what the entry means.
///
/// This is the one remediation that can be applied mechanically. Naming a
/// capture cannot be: the value has to be rewritten to `$name`, and so does
/// every `location`/`if`/`rewrite` that reads the capture — which block that is
/// depends on which regex ran first at request time, may live in another
/// `include`d file, and needs a variable name that is guaranteed not to
/// collide. None of that is decidable here.
///
/// Going non-capturing is also the *stronger* fix, when it removes the last
/// capture. `ngx_http_regex_exec()` reallocates only `if (re->ncaptures)`, and
/// PCRE counts named groups too — so a named capture still reallocates and only
/// makes its own read safe. Dropping every unnamed group to `(?:...)` stops the
/// realloc outright *if no named group remains*; if one does, the regex still
/// reallocates and this only removes the unnamed reads.
///
/// Refused unless the whole entry is safe to rewrite:
///
/// - the **value** must not read a positional capture — otherwise `(?:...)`
///   deletes the group it names (`~^/old/(.*)$ /new/$1;`);
/// - the **pattern** must not reference a group by number either. A
///   backreference outlives the group: `~^/(a|b)/\1$` rewrites to
///   `~^/(?:a|b)/\1$`, whose `\1` now points at nothing, and nginx refuses to
///   start with `pcre_compile() failed`. Same for `\g{1}`, recursion `(?1)`,
///   and conditionals `(?(1)...)`.
///
/// What survives is pure grouping (`~*(bot|crawler) 1;`), where `(?:...)` is
/// the better idiom regardless of the CVE. Anything else is reported without a
/// fix.
///
/// Caveat: nginx also lets a *later* directive read `$1` left behind by a
/// matching map regex. That aliasing is exactly the footgun this CVE is about
/// and cannot be detected from the map block, so it is not accounted for here.
fn non_capturing_fixes(entry: &Directive, pattern: &str, positions: &[usize]) -> Option<Vec<Fix>> {
    if entry
        .args
        .iter()
        .any(|arg| reads_positional_capture(&arg.raw))
    {
        return None;
    }

    if references_group_by_number(pattern) {
        return None;
    }

    // `positions` index into `pattern`, which is a slice of `entry.name` — the
    // *decoded* key. Rewriting bytes needs source offsets, and `name_span`
    // covers the raw token, so the two only line up once the quoting is
    // accounted for.
    let key_start = entry.name_span.start.offset + key_quote_len(entry)?;
    let pattern_start = key_start + (entry.name.len() - pattern.len());

    Some(
        positions
            .iter()
            .map(|&pos| {
                let paren = pattern_start + pos;
                Fix::replace_range(paren, paren + 1, "(?:")
            })
            .collect(),
    )
}

/// Width of the opening quote on this entry's key, or `None` when offsets into
/// the decoded key cannot be mapped back onto the source.
///
/// The parser hands back `name` decoded but `name_span` covering the raw token,
/// so the gap between them is what the quoting occupies:
///
/// - `0` — a bare token such as `~^/old/(.*)$`; offsets already line up.
/// - `2` — a plain `"..."` / `'...'` with nothing else decoded away, so the
///   opening quote is one byte and the rest of the key maps over one-to-one.
///
/// Any other gap means the parser also unescaped something inside the key, and
/// positions in the decoded string no longer correspond linearly to source
/// bytes — in that case there is no fix, only the warning. Rewriting on a
/// mis-mapped offset would silently corrupt the config: a key like
/// `"~^/a{2,3}/(x|y)$"` (quoted because nginx requires it for `{n,m}`) would
/// get its `/` rewritten instead of its `(`, leaving unbalanced parens that
/// nginx refuses to load.
///
/// The `2` case leans on a quirk of *this* parser: nginx's own
/// `ngx_conf_read_token` decodes `\\`, `\"`, `\'`, `\t`, `\r`, `\n` in
/// unquoted tokens as well, and if nginx-lint's parser ever did the same, two
/// such escapes in a bare key would also give a gap of 2 and shift every fix
/// by a byte. It does not decode them today (checked), so a gap of 2 means
/// quotes and nothing else. If that ever changes, this must become an explicit
/// "is the first byte a quote" test against the source text.
fn key_quote_len(entry: &Directive) -> Option<usize> {
    let raw_len = entry.name_span.end.offset - entry.name_span.start.offset;

    match raw_len.checked_sub(entry.name.len())? {
        0 => Some(0),
        2 => Some(1),
        _ => None,
    }
}

/// Whether the regex refers to one of its own groups by number, which makes
/// dropping that group to `(?:...)` a breaking change rather than a no-op.
///
/// Covers backreferences (`\1`, `\g1`, `\g{1}`, `\g{-1}`), subroutine calls and
/// recursion (`(?1)`, `(?+1)`, `(?R)`), and conditionals (`(?(1)...)`). Like
/// [`reads_positional_capture`] this is deliberately blunt: anything resembling
/// a numeric reference suppresses the autofix rather than risking a rewrite
/// that nginx then refuses to load.
fn references_group_by_number(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        let next = bytes.get(i + 1).copied();
        match b {
            // `\1`, `\g...`. Every byte is inspected, escape pairs included —
            // no skipping. That over-matches (a literal `\\1` trips the second
            // `\`), but over-blocking only costs a fix, so it stays blunt.
            b'\\' => match next {
                Some(c) if c.is_ascii_digit() => return true,
                Some(b'g') | Some(b'k') => return true,
                _ => {}
            },
            // `(?1)`, `(?+1)`, `(?-1)`, `(?R)`, `(?(1)...)`, `(?&name)`
            b'(' if next == Some(b'?') => match bytes.get(i + 2).copied() {
                Some(c) if c.is_ascii_digit() => return true,
                Some(b'+') | Some(b'-') | Some(b'R') | Some(b'(') | Some(b'&') => return true,
                _ => {}
            },
            _ => {}
        }
    }

    false
}

/// Whether the text reads a positional capture — `$1`..`$9`.
///
/// `${1}` is also matched, but only out of bluntness: it is NOT a braced
/// positional reference. `ngx_http_script_compile()` tests the digit before the
/// brace, so `${1}` takes the variable path and becomes a variable *named* "1",
/// which nginx rejects at load with `unknown "1" variable`. Blocking the fix
/// there costs nothing — the config never ran.
///
/// Deliberately blunt: anything that looks like a positional reference counts,
/// so an odd spelling suppresses the autofix rather than risking a rewrite that
/// changes behaviour.
fn reads_positional_capture(raw: &str) -> bool {
    let bytes = raw.as_bytes();

    for (i, _) in bytes.iter().enumerate().filter(|&(_, &b)| b == b'$') {
        match bytes.get(i + 1) {
            Some(b) if b.is_ascii_digit() => return true,
            Some(b'{') => {
                let mut end = i + 2;
                while bytes.get(end).is_some_and(|b| b.is_ascii_digit()) {
                    end += 1;
                }
                if end > i + 2 && bytes.get(end) == Some(&b'}') {
                    return true;
                }
            }
            _ => {}
        }
    }

    false
}

/// Strip the `~` / `~*` match modifier off a map entry key.
///
/// Returns `None` for literal keys (`/old`), for `hostnames` / `volatile` /
/// `default`, and for a bare `~` with no pattern behind it.
fn map_regex_pattern(key: &str) -> Option<&str> {
    let rest = key.strip_prefix('~')?;
    let pattern = rest.strip_prefix('*').unwrap_or(rest);
    (!pattern.is_empty()).then_some(pattern)
}

nginx_lint_plugin::export_component_plugin!(MapUnnamedCapturePlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::PluginTestRunner;

    fn runner() -> PluginTestRunner<MapUnnamedCapturePlugin> {
        PluginTestRunner::new(MapUnnamedCapturePlugin)
    }

    #[test]
    fn test_unnamed_capture_is_reported() {
        runner().assert_has_errors(
            r#"
http {
    map $uri $target {
        default /;
        ~^/old/(.*)$ /new/$1;
    }
}
"#,
        );
    }

    #[test]
    fn test_unnamed_capture_reported_even_when_value_ignores_it() {
        runner().assert_has_errors(
            r#"
http {
    map $uri $is_old {
        default 0;
        ~^/old/(.*)$ 1;
    }
}
"#,
        );
    }

    #[test]
    fn test_named_capture_is_ok() {
        runner().assert_no_errors(
            r#"
http {
    map $uri $target {
        default /;
        ~^/old/(?<rest>.*)$ /new/$rest;
        ~^/x/(?'seg'.*)$ /y/$seg;
        ~^/p/(?P<seg2>.*)$ /q/$seg2;
    }
}
"#,
        );
    }

    #[test]
    fn test_non_capturing_and_lookaround_are_ok() {
        runner().assert_no_errors(
            r#"
http {
    map $uri $flag {
        default 0;
        ~^/a/(?:foo|bar)$ 1;
        ~^/b/(?=foo) 1;
        ~^/c/(?!foo) 1;
        ~^/d/(?<=foo)bar 1;
        ~^/e/(?<!foo)bar 1;
    }
}
"#,
        );
    }

    #[test]
    fn test_literal_and_special_keys_are_ok() {
        runner().assert_no_errors(
            r#"
http {
    map $http_host $name {
        hostnames;
        default 0;
        example.com 1;
        *.example.com 1;
        /path/(not-a-regex) 2;
    }
}
"#,
        );
    }

    #[test]
    fn test_escaped_and_bracketed_parens_are_ok() {
        runner().assert_no_errors(
            r#"
http {
    map $uri $flag {
        default 0;
        ~^/esc/\(literal\)$ 1;
        ~^/cls/[()]$ 2;
    }
}
"#,
        );
    }

    #[test]
    fn test_case_insensitive_map_regex() {
        runner().assert_has_errors(
            r#"
http {
    map $uri $target {
        default /;
        ~*^/OLD/(.*)$ /new/$1;
    }
}
"#,
        );
    }

    #[test]
    fn test_mixed_named_and_unnamed_is_reported() {
        runner().assert_has_errors(
            r#"
http {
    map $uri $target {
        default /;
        ~^/(?<head>[a-z]+)/(.*)$ /$head/$2;
    }
}
"#,
        );
    }

    /// `stream` has no subrequests, and `ngx_stream_regex_exec()` allocates
    /// `s->captures` only when it is null — it never reallocates, so the stale
    /// count this CVE depends on cannot arise. Reporting here would be a pure
    /// false positive.
    #[test]
    fn test_map_in_stream_is_ignored() {
        runner().assert_no_errors(
            r#"
stream {
    map $ssl_preread_server_name $backend {
        default default_backend;
        ~^(.+)\.example\.com$ $1_backend;
    }
}
"#,
        );
    }

    /// A map inside `http` is still reported when a `stream` block sits
    /// alongside it — the skip must key off the map's own context, not the
    /// presence of `stream` anywhere in the file.
    #[test]
    fn test_http_map_is_reported_alongside_a_stream_block() {
        runner().assert_has_errors(
            r#"
stream {
    map $ssl_preread_server_name $backend {
        default default_backend;
        ~^(.+)\.example\.com$ $1_backend;
    }
}
http {
    map $uri $target {
        default /;
        ~^/old/(.*)$ /new/$1;
    }
}
"#,
        );
    }

    /// A bare `map` with no visible `http` parent — the shape of an
    /// `include`d snippet — must still be reported.
    #[test]
    fn test_map_without_visible_http_parent_is_reported() {
        runner().assert_has_errors(
            r#"
map $uri $target {
    default /;
    ~^/old/(.*)$ /new/$1;
}
"#,
        );
    }

    /// `volatile` is not a precondition for the bug, so it must not gate the
    /// rule: a cached map is exploitable too when its first evaluation lands
    /// inside a clone subrequest. See the module docs for why that is not
    /// covered by a container test.
    ///
    /// This alternation also shows why the resulting report is not mere noise:
    /// `(bot|crawler)` never needed to capture, and `(?:bot|crawler)` is the
    /// better idiom regardless of the CVE.
    #[test]
    fn test_non_volatile_map_is_also_reported() {
        runner().assert_has_errors(
            r#"
http {
    map $http_user_agent $is_bot {
        default 0;
        ~*(bot|crawler) 1;
    }
}
"#,
        );
    }

    #[test]
    fn test_volatile_map_is_reported() {
        runner().assert_has_errors(
            r#"
http {
    map $uri $target {
        volatile;
        default /;
        ~^/old/(.*)$ /new/$1;
    }
}
"#,
        );
    }

    #[test]
    fn test_capture_outside_map_is_ignored() {
        runner().assert_no_errors(
            r#"
http {
    server {
        location ~ ^/api/(.*)$ {
            rewrite ^/api/(.*)$ /$1 break;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_reports_each_offending_entry() {
        let errors = runner()
            .check_string(
                r#"
http {
    map $uri $a {
        default /;
        ~^/one/(.*)$ /1/$1;
        ~^/two/(?<r>.*)$ /2/$r;
        ~^/three/(.*)$ /3/$1;
    }
}
"#,
            )
            .unwrap();

        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
    }

    /// Apply every range-based fix to the source, right-to-left so earlier
    /// offsets stay valid.
    fn apply_fixes(source: &str, errors: &[LintError]) -> String {
        let mut fixes: Vec<_> = errors.iter().flat_map(|e| e.fixes.iter()).collect();
        fixes.sort_by_key(|f| std::cmp::Reverse(f.start_offset.unwrap()));

        let mut out = source.to_string();
        for fix in fixes {
            out.replace_range(
                fix.start_offset.unwrap()..fix.end_offset.unwrap(),
                &fix.new_text,
            );
        }
        out
    }

    #[test]
    fn test_grouping_only_entry_is_fixed_to_non_capturing() {
        let source = r#"
http {
    map $http_user_agent $is_bot {
        default 0;
        ~*(bot|crawler) 1;
    }
}
"#;
        let errors = runner().check_string(source).unwrap();
        let fixed = apply_fixes(source, &errors);

        assert!(
            fixed.contains("~*(?:bot|crawler) 1;"),
            "expected the group to become non-capturing, got:\n{fixed}"
        );
        // The fix must fully resolve the finding, not just move it.
        runner().assert_no_errors(&fixed);
    }

    #[test]
    fn test_multiple_groups_in_one_entry_are_all_fixed() {
        let source = r#"
http {
    map $uri $flag {
        default 0;
        ~^/(a|b)/(c|d)$ 1;
    }
}
"#;
        let errors = runner().check_string(source).unwrap();
        let fixed = apply_fixes(source, &errors);

        assert!(
            fixed.contains("~^/(?:a|b)/(?:c|d)$ 1;"),
            "expected both groups to become non-capturing, got:\n{fixed}"
        );
        runner().assert_no_errors(&fixed);
    }

    /// The value reads `$1`, so going non-capturing would break the map.
    /// Report, but offer no fix.
    #[test]
    fn test_entry_whose_value_reads_capture_is_not_fixed() {
        let errors = runner()
            .check_string(
                r#"
http {
    map $uri $target {
        default /;
        ~^/old/(.*)$ /new/$1;
    }
}
"#,
            )
            .unwrap();

        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].fixes.is_empty(),
            "must not autofix an entry whose value reads $1, got: {:?}",
            errors[0].fixes
        );
    }

    /// `${1}` is NOT a braced positional reference — nginx reads it as a
    /// variable named "1" and refuses to load (`unknown "1" variable`),
    /// verified on 1.29. The fix is blocked anyway because
    /// [`reads_positional_capture`] is deliberately blunt, which is free: the
    /// config could never have run. Pinned so nobody "simplifies" the check on
    /// the false premise that `${1}` means `$1`.
    #[test]
    fn test_braced_variable_named_1_blocks_the_fix() {
        let errors = runner()
            .check_string(
                r#"
http {
    map $host $backend {
        default default_backend;
        ~^(.+)\.example\.com$ ${1}_backend;
    }
}
"#,
            )
            .unwrap();

        assert_eq!(errors.len(), 1);
        assert!(errors[0].fixes.is_empty(), "`${{1}}` must block the fix");
    }

    /// A named group in the same entry is left alone — only the unnamed one is
    /// rewritten, and `$name` keeps resolving because PCRE still numbers it.
    ///
    /// Note what this fix does NOT achieve. The surviving named group keeps
    /// `re->ncaptures` non-zero, so the regex still reallocates and still arms
    /// the bug for any unnamed `$1` read elsewhere on the request path. The
    /// rule reports nothing afterwards because its predicate is "unnamed
    /// capture in a map regex", not "this regex is now harmless" — the
    /// `assert_no_errors` below pins the predicate, not safety.
    #[test]
    fn test_named_group_is_preserved_while_unnamed_is_fixed() {
        let source = r#"
http {
    map $uri $target {
        default /;
        ~^/(?<head>[a-z]+)/(x|y)$ /$head/;
    }
}
"#;
        let errors = runner().check_string(source).unwrap();
        let fixed = apply_fixes(source, &errors);

        assert!(
            fixed.contains("~^/(?<head>[a-z]+)/(?:x|y)$ /$head/;"),
            "expected only the unnamed group to change, got:\n{fixed}"
        );
        runner().assert_no_errors(&fixed);
    }

    /// Regression: nginx requires quoting for keys containing `{n,m}`, and the
    /// parser decodes `name` while `name_span` still covers the quotes. Getting
    /// this wrong rewrote the `/` instead of the `(`, emitting a config with
    /// unbalanced parens that nginx refuses to load.
    #[test]
    fn test_quoted_key_is_fixed_at_the_right_offset() {
        let source = r#"
http {
    map $uri $flag {
        default 0;
        "~^/a{2,3}/(x|y)$" 1;
    }
}
"#;
        let errors = runner().check_string(source).unwrap();
        let fixed = apply_fixes(source, &errors);

        assert!(
            fixed.contains(r#""~^/a{2,3}/(?:x|y)$" 1;"#),
            "quoted key must be rewritten at the right offset, got:\n{fixed}"
        );
        runner().assert_no_errors(&fixed);
    }

    #[test]
    fn test_single_quoted_key_is_fixed_at_the_right_offset() {
        let source = "
http {
    map $uri $flag {
        default 0;
        '~^/a{2,3}/(x|y)$' 1;
    }
}
";
        let errors = runner().check_string(source).unwrap();
        let fixed = apply_fixes(source, &errors);

        assert!(
            fixed.contains("'~^/a{2,3}/(?:x|y)$' 1;"),
            "single-quoted key must be rewritten at the right offset, got:\n{fixed}"
        );
        runner().assert_no_errors(&fixed);
    }

    /// An escape inside the key means the decoded `name` is shorter than the
    /// source by an amount that is not just the quotes, so positions no longer
    /// map linearly. Report, but do not risk a corrupting rewrite.
    #[test]
    fn test_key_with_escapes_is_reported_without_a_fix() {
        let errors = runner()
            .check_string(
                r#"
http {
    map $uri $flag {
        default 0;
        "~^/a\"b/(x|y)$" 1;
    }
}
"#,
            )
            .unwrap();

        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].fixes.is_empty(),
            "a key with escapes must not be autofixed, got: {:?}",
            errors[0].fixes
        );
    }

    /// Regression: `(?:...)` deletes the group a backreference points at, and
    /// nginx then refuses to start (`pcre_compile() failed`). Verified against
    /// nginx 1.29: `~^/(a|b)/\1$` loads, `~^/(?:a|b)/\1$` does not.
    #[test]
    fn test_backreference_in_pattern_blocks_the_fix() {
        let errors = runner()
            .check_string(
                r#"
http {
    map $uri $flag {
        default 0;
        ~^/(a|b)/\1$ 1;
    }
}
"#,
            )
            .unwrap();

        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].fixes.is_empty(),
            "a backreference must block the fix, got: {:?}",
            errors[0].fixes
        );
    }

    /// Same class as the backreference: these all name a group by number, so
    /// removing the group breaks them.
    #[test]
    fn test_numeric_group_references_block_the_fix() {
        for pattern in [
            r"~^/(a|b)/\g{1}$",
            r"~^/(a|b)(?1)$",
            r"~^/(a)(?(1)x|y)$",
            r"~^/(a|b)/\g1$",
        ] {
            let errors = runner()
                .check_string(&format!(
                    "http {{\n    map $uri $flag {{\n        default 0;\n        {pattern} 1;\n    }}\n}}\n"
                ))
                .unwrap();

            assert!(
                !errors.is_empty() && errors[0].fixes.is_empty(),
                "{pattern} must be reported without a fix, got: {errors:?}"
            );
        }
    }

    /// The detector must not see regex syntax inside a `\Q...\E` literal span
    /// or a character class whose first member is `]`.
    #[test]
    fn test_literal_parens_are_not_reported() {
        runner().assert_no_errors(
            r#"
http {
    map $uri $flag {
        default 0;
        ~^/q/\Q(a)\E$ 1;
        ~^/c/[]()]$ 2;
        ~^/p/[[:alpha:]()]$ 3;
    }
}
"#,
        );
    }

    /// Known miss, pinned so it is not mistaken for correct behaviour.
    ///
    /// nginx honours a quote only at the start of a token
    /// (`ngx_conf_read_token` checks `last_space`), so `~^/"a"/(x|y)$` is one
    /// key with a literal `"` in it and loads fine. nginx-lint's parser splits
    /// the token at the quote instead, so the plugin never sees the capture and
    /// stays silent. The fix belongs in the parser, not here.
    #[test]
    fn test_key_with_embedded_quote_is_missed() {
        runner().assert_no_errors(
            r#"
http {
    map $uri $flag {
        default 0;
        ~^/"a"/(x|y)$ 1;
    }
}
"#,
        );
    }

    #[test]
    fn test_examples() {
        runner().test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_fixtures() {
        runner().test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
