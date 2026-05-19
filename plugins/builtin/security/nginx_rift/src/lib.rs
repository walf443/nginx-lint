//! nginx-rift plugin (CVE-2026-42945)
//!
//! Detects the vulnerable nginx configuration pattern behind CVE-2026-42945
//! (marketed as "NGINX Rift"): a `rewrite` directive whose replacement
//! string contains `?`, followed in the same block by a `set` or `rewrite`
//! directive that references an unnamed PCRE capture variable
//! (`$1`..`$9`).
//!
//! On affected nginx versions (0.6.27 .. 1.30.0) the leaked `is_args` flag
//! causes a heap buffer overrun in the worker process during request
//! handling, which can lead to a worker crash or unauthenticated RCE on
//! systems without ASLR.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

#[derive(Default)]
pub struct NginxRiftPlugin;

impl Plugin for NginxRiftPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "nginx-rift",
            "security",
            "Detects the CVE-2026-42945 vulnerable pattern: a rewrite with '?' in \
             the replacement followed by a `set` or `rewrite` using unnamed \
             captures ($1..$9) in the same scope",
        )
        .with_severity("warning")
        .with_why(
            "CVE-2026-42945 (\"NGINX Rift\") is a heap buffer overflow in \
             ngx_http_rewrite_module that affects nginx 0.6.27 through 1.30.0 \
             (fixed in 1.30.1 and 1.31.0). When a `rewrite` replacement \
             contains `?`, the worker sets an internal `is_args` flag on the \
             main script engine and never clears it. A subsequent `set` or \
             `rewrite` directive that references an unnamed PCRE capture \
             (`$1`..`$9`) then uses a length-calculation pass that \
             ignores the flag while the copy pass honors it, so the worker \
             writes more bytes (each escape-prone byte expands by 2, e.g. \
             `+` -> `%2B`) than were allocated. This may cause worker \
             crashes or — on systems without ASLR — unauthenticated remote \
             code execution. Empirically, the captured value is also \
             corrupted on the wire (`/api/foo+bar` returns a truncated \
             `captured=foo%2Bb`). The reliable fixes are: (1) upgrade nginx \
             to 1.30.1 / 1.31.0 or later; (2) drop `?` from the rewrite \
             replacement; (3) switch to named captures (`(?<name>...)`), \
             which are resolved through a separate code path that does not \
             share the rewrite engine's `is_args` state. Note that simply \
             reordering `set` to run before `rewrite` does NOT remove the \
             underlying bug. The configuration is syntactically valid and \
             nginx will load it; the danger is runtime-only on vulnerable \
             builds. This rule is enabled by default; disable it in \
             configuration once your entire fleet is on nginx >= 1.30.1 / \
             1.31.0.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nvd.nist.gov/vuln/detail/CVE-2026-42945".to_string(),
            "https://depthfirst.com/research/nginx-rift-achieving-nginx-rce-via-an-18-year-old-vulnerability".to_string(),
            "https://github.com/walf443/nginx-lint/blob/main/plugins/builtin/security/nginx_rift/tests/container_test.rs".to_string(),
        ])
        .with_min_version("0.6.27")
        .with_max_version("1.30.0")
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();
        check_items(&config.items, &mut errors, &err);
        errors
    }
}

fn check_items(items: &[ConfigItem], errors: &mut Vec<LintError>, err: &ErrorBuilder) {
    let directives: Vec<&Directive> = items
        .iter()
        .filter_map(|item| match item {
            ConfigItem::Directive(d) => Some(d.as_ref()),
            _ => None,
        })
        .collect();

    for (i, dir) in directives.iter().enumerate() {
        if is_rewrite_with_question_mark(dir) {
            let rest = &directives[i + 1..];
            let triggers = rest
                .iter()
                .any(|next| is_capture_consumer(next) && uses_unnamed_capture(next));
            if triggers {
                let mut error = err.warning_at(
                    "CVE-2026-42945: `rewrite` replacement contains `?` and is \
                     followed by a directive that references an unnamed capture \
                     ($1..$9) in the same scope — this triggers a heap buffer \
                     overflow on nginx <1.30.1/<1.31.0. Upgrade nginx, switch to \
                     named captures, or remove `?` from the replacement.",
                    *dir,
                );
                for fix in build_named_capture_fixes(dir, rest) {
                    error = error.with_fix(fix);
                }
                errors.push(error);
            }
        }

        if let Some(block) = &dir.block {
            check_items(&block.items, errors, err);
        }
    }
}

/// Build autofix entries that rewrite unnamed captures to named ones (`cap1`,
/// `cap2`, ..., `capN`) across the vulnerable rewrite and the subsequent
/// capture-consuming directives in the same scope.
///
/// The fix preserves the surrounding token (including quotes) and only edits
/// inside each argument's source span. If any sub-fix cannot be safely
/// generated (e.g. a `set $foo $5` referencing a capture index that doesn't
/// exist), that one sub-fix is skipped — the rest are still emitted, so the
/// user gets a partial autofix and the rule keeps flagging the residual.
fn build_named_capture_fixes(v_rewrite: &Directive, rest: &[&Directive]) -> Vec<Fix> {
    let mut fixes = Vec::new();

    let Some(v_regex_arg) = v_rewrite.args.first() else {
        return fixes;
    };
    if regex_has_named_capture(&v_regex_arg.raw) {
        // Mixed named/unnamed captures: our cap-numbering can't safely
        // map `$N` references — bail on the entire chain. Warning still
        // fires; user fixes manually.
        return fixes;
    }
    let (new_regex_raw, v_cap_count) = build_renamed_regex(&v_regex_arg.raw);
    if v_cap_count == 0 {
        return fixes;
    }
    push_arg_fix(&mut fixes, v_regex_arg, new_regex_raw);

    // The vulnerable rewrite's own replacement may also use $N.
    for arg in v_rewrite.args.iter().skip(1) {
        if let Some(new_raw) = rename_positional_refs_in_raw(&arg.raw, v_cap_count) {
            push_arg_fix(&mut fixes, arg, new_raw);
        }
    }

    // The "active capture context" is the number of unnamed captures
    // available to a subsequent `set` directive. It starts at the vulnerable
    // rewrite's count and is replaced whenever a later `rewrite` runs.
    let mut active_caps = v_cap_count;

    for consumer in rest {
        if consumer.is("set") {
            for arg in consumer.args.iter().skip(1) {
                if let Some(new_raw) = rename_positional_refs_in_raw(&arg.raw, active_caps) {
                    push_arg_fix(&mut fixes, arg, new_raw);
                }
            }
        } else if consumer.is("rewrite") {
            let Some(c_regex_arg) = consumer.args.first() else {
                continue;
            };
            if regex_has_named_capture(&c_regex_arg.raw) {
                // Can't safely rename this rewrite's regex or its
                // replacement's `$N`. Also force `active_caps = 0` so
                // subsequent `set` consumers don't rename their `$N`
                // either — those `$N` refer to this unfixable
                // rewrite's captures, and renaming them would produce
                // undefined `$capN` references at nginx-load time.
                active_caps = 0;
                continue;
            }
            let (new_c_regex_raw, c_cap_count) = build_renamed_regex(&c_regex_arg.raw);
            if c_cap_count > 0 {
                push_arg_fix(&mut fixes, c_regex_arg, new_c_regex_raw);
            }
            for arg in consumer.args.iter().skip(1) {
                if let Some(new_raw) = rename_positional_refs_in_raw(&arg.raw, c_cap_count) {
                    push_arg_fix(&mut fixes, arg, new_raw);
                }
            }
            active_caps = c_cap_count;
        }
    }

    fixes
}

fn push_arg_fix(fixes: &mut Vec<Fix>, arg: &Argument, new_raw: String) {
    if new_raw == arg.raw {
        return;
    }
    fixes.push(Fix::replace_range(
        arg.span.start.offset,
        arg.span.end.offset,
        &new_raw,
    ));
}

/// Rewrite a regex source string so that each unnamed capture group `(...)`
/// becomes a named group `(?<capN>...)`, numbered from 1 in source order.
///
/// Returns the rewritten source plus the number of captures introduced.
///
/// Non-capturing groups `(?:...)`, lookarounds `(?=...)`/`(?!...)`/`(?<=...)`,
/// already-named groups `(?<name>...)`/`(?P<name>...)`, escaped `\(`, and `(`
/// inside character classes `[...]` are all left untouched.
///
/// Callers should bail out (via [`regex_has_named_capture`]) before calling
/// this on a regex that already contains a named capture: the cap-numbering
/// here is the index among *unnamed* groups, which doesn't match PCRE's
/// positional numbering once named groups are mixed in. Renaming `$1` based
/// on this index would silently change which group is referenced.
fn build_renamed_regex(raw: &str) -> (String, usize) {
    let positions = find_unnamed_capture_positions(raw);
    if positions.is_empty() {
        return (raw.to_string(), 0);
    }

    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len() + positions.len() * 8);
    let mut cursor = 0;
    for (idx, &pos) in positions.iter().enumerate() {
        // Copy bytes up to and including the `(`. Slicing on an ASCII
        // `(` boundary is always valid UTF-8 since `raw: &str` is, so
        // the from_utf8 conversion is infallible here.
        out.push_str(std::str::from_utf8(&bytes[cursor..=pos]).expect("ASCII boundary"));
        out.push_str(&format!("?<cap{}>", idx + 1));
        cursor = pos + 1;
    }
    out.push_str(std::str::from_utf8(&bytes[cursor..]).expect("ASCII boundary"));
    (out, positions.len())
}

/// Detect whether a regex source string contains any named capture group
/// (`(?<name>...)` or `(?P<name>...)`). Lookbehinds `(?<=...)` / `(?<!...)`
/// are NOT named captures and don't count.
///
/// Used to bail out of autofix on regexes that mix named and unnamed
/// captures: our cap-numbering (index among unnamed) diverges from PCRE's
/// positional numbering once a named group is present, so a `$1` could end
/// up renamed to point at a different capture.
fn regex_has_named_capture(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut i = 0;
    let mut in_char_class = false;

    while i < bytes.len() {
        let b = bytes[i];

        if b == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }

        if in_char_class {
            if b == b']' {
                in_char_class = false;
            }
            i += 1;
            continue;
        }

        if b == b'[' {
            in_char_class = true;
            i += 1;
            continue;
        }

        if b == b'(' && i + 3 < bytes.len() && bytes[i + 1] == b'?' {
            // `(?<name>...)` — named iff the char after `<` is a name-start
            // byte. `(?<=...)` and `(?<!...)` are lookbehinds; not captures.
            if bytes[i + 2] == b'<' {
                let after_lt = bytes[i + 3];
                if after_lt != b'=' && after_lt != b'!' {
                    return true;
                }
            }
            // `(?P<name>...)` — Python-style named capture.
            // (Only `bytes[i + 3]` is read; the outer `i + 3 < bytes.len()`
            // guard is already sufficient.)
            if bytes[i + 2] == b'P' && bytes[i + 3] == b'<' {
                return true;
            }
        }

        i += 1;
    }

    false
}

/// Find byte offsets of `(` characters that open an unnamed PCRE capture group.
///
/// Skips: escapes (`\(`), character classes (`[...]`), the `(?...)` family —
/// non-capturing `(?:...)`, named `(?<name>...)` / `(?P<name>...)`,
/// lookarounds `(?=...)` / `(?!...)` / `(?<=...)` / `(?<!...)`, atomic
/// `(?>...)`, comments `(?#...)`, and inline modifiers `(?i)` — and the
/// `(*VERB)` family — PCRE control verbs like `(*PRUNE)`, `(*SKIP)`,
/// `(*FAIL)`, `(*MARK:name)`, etc.
fn find_unnamed_capture_positions(regex: &str) -> Vec<usize> {
    let bytes = regex.as_bytes();
    let mut positions = Vec::new();
    let mut i = 0;
    let mut in_char_class = false;

    while i < bytes.len() {
        let b = bytes[i];

        if b == b'\\' && i + 1 < bytes.len() {
            // Escaped byte — skip both bytes.
            i += 2;
            continue;
        }

        if in_char_class {
            if b == b']' {
                in_char_class = false;
            }
            i += 1;
            continue;
        }

        if b == b'[' {
            in_char_class = true;
            i += 1;
            continue;
        }

        if b == b'(' {
            let next = bytes.get(i + 1).copied();
            // `(?...)` constructs and `(*VERB)` control verbs are never
            // unnamed captures.
            if next != Some(b'?') && next != Some(b'*') {
                positions.push(i);
            }
        }

        i += 1;
    }

    positions
}

/// Rewrite `$1`..`$9` and `${1}`..`${9}` references inside a raw argument
/// source to `$cap1`..`$capN` (using `${capN}` brace form when followed by
/// a name-continuation byte, to keep the variable boundary unambiguous).
///
/// Returns `Some(new_raw)` when every reference can be rewritten safely; the
/// caller still needs to compare against the original to detect "nothing
/// changed". Returns `None` when any `$N` references a position greater than
/// `max_captures` — i.e. the rewrite would create an undefined variable
/// reference, so it's safer to leave that argument alone.
fn rename_positional_refs_in_raw(raw: &str, max_captures: usize) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if b != b'$' || i + 1 >= bytes.len() {
            out.push(b as char);
            i += 1;
            continue;
        }

        let after = bytes[i + 1];
        // `${N}` form
        if after == b'{'
            && i + 3 < bytes.len()
            && matches!(bytes[i + 2], b'1'..=b'9')
            && bytes[i + 3] == b'}'
        {
            let n = (bytes[i + 2] - b'0') as usize;
            if n > max_captures {
                return None;
            }
            out.push_str(&format!("${{cap{}}}", n));
            i += 4;
            continue;
        }
        // `$N` form
        if matches!(after, b'1'..=b'9') {
            let n = (after - b'0') as usize;
            if n > max_captures {
                return None;
            }
            // If the next byte continues a variable name, switch to brace
            // form to avoid `$cap1abc` collapsing into a single var name.
            let follow = bytes.get(i + 2).copied();
            let needs_braces = matches!(
                follow,
                Some(b'_') | Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'0'..=b'9')
            );
            if needs_braces {
                out.push_str(&format!("${{cap{}}}", n));
            } else {
                out.push_str(&format!("$cap{}", n));
            }
            i += 2;
            continue;
        }

        out.push('$');
        i += 1;
    }

    Some(out)
}

fn is_rewrite_with_question_mark(dir: &Directive) -> bool {
    if !dir.is("rewrite") || dir.args.len() < 2 {
        return false;
    }
    // The replacement string is logically one nginx token, but our parser
    // splits it on variable boundaries (e.g. `/backend/$1?foo` →
    // `/backend/`, `$1`, `?foo`). Scan every arg after the regex.
    dir.args.iter().skip(1).any(|a| a.raw.contains('?'))
}

/// Directives that, on vulnerable nginx, hit the buggy script-engine path
/// when evaluating an unnamed PCRE capture (`$1`..`$9`) — i.e. the
/// directives whose use of `$N` after a leaking `rewrite … ?…` actually
/// triggers the buffer overrun.
///
/// `set` and `rewrite` (in its replacement string) are both verified
/// observable on nginx 1.30.0: with a preceding rewrite-with-`?` in the
/// same scope, `+` in the captured value comes back as `%2B` (and for
/// `set`, the value is also truncated mid-escape).
///
/// `return` is intentionally NOT included: empirically `return 200 "$1"`
/// after a `rewrite … ?…` on nginx 1.30.0 returns `$1` clean (no
/// mis-escape, no truncation), because `return`'s complex-value
/// evaluation goes through a different code path.
///
/// `if` is intentionally NOT included: the only way `if`'s argument list
/// can contain `$N` is the form `if ($1 …)`, which nginx itself rejects
/// at config-load time with `unknown "1" variable` — so the rule would
/// only ever fire on configs nginx refuses to start. The legitimate
/// `if ($var ~ "regex")` form has its own captures via the if-regex and
/// does not reference the outer rewrite's `$N` in its argument list.
fn is_capture_consumer(dir: &Directive) -> bool {
    dir.is("set") || dir.is("rewrite")
}

fn uses_unnamed_capture(dir: &Directive) -> bool {
    // Variable arguments (`$1`) keep the `$` in `raw` but strip it in `as_str()`.
    // Quoted / literal arguments embed `$1` inside `raw` and `as_str()` alike.
    // Scanning `raw` covers both cases.
    dir.args
        .iter()
        .any(|arg| contains_unnamed_capture(&arg.raw))
}

/// Detect references to positional captures `$1`..`$9`, including the
/// brace-disambiguated form `${1}`..`${9}` used when the capture is
/// followed by characters that would otherwise be parsed as part of the
/// variable name (e.g. `${1}_suffix`).
fn contains_unnamed_capture(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            // `$1`..`$9`
            if matches!(next, b'1'..=b'9') {
                return true;
            }
            // `${1}`..`${9}`
            if next == b'{' && i + 2 < bytes.len() && matches!(bytes[i + 2], b'1'..=b'9') {
                return true;
            }
        }
        i += 1;
    }
    false
}

nginx_lint_plugin::export_component_plugin!(NginxRiftPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::{PluginTestRunner, TestCase};

    #[test]
    fn test_detects_rewrite_then_set_with_capture() {
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_has_errors(
            r#"
http {
    server {
        location ~ ^/api/(.*)$ {
            rewrite ^/api/(.*)$ /internal?migrated=true;
            set $original_endpoint $1;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_detects_rewrite_then_rewrite_with_capture() {
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_has_errors(
            r#"
http {
    server {
        location ~ ^/foo/(.*)$ {
            rewrite ^/foo/(.*)$ /bar?x=1;
            rewrite ^/bar/(.*)$ /baz/$1 last;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_error_on_if_with_positional_capture_form() {
        // `if ($1 …)` is nginx-invalid at runtime (`unknown "1" variable`),
        // so flagging it would only produce false positives on configs
        // nginx itself refuses to load. Verify the rule does NOT fire.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_no_errors(
            r#"
http {
    server {
        location ~ ^/foo/(.*)$ {
            rewrite ^/foo/(.*)$ /bar?x=1;
            if ($1) {
                return 200;
            }
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_detects_brace_form_of_unnamed_capture() {
        // ${1} is the brace-disambiguated form of $1 (used when followed
        // by characters that would otherwise be parsed into the variable
        // name). The bug-triggering semantics are identical, so it must
        // also be flagged.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_has_errors(
            r#"
http {
    server {
        location ~ ^/api/(.*)$ {
            rewrite ^/api/(.*)$ /internal?migrated=true;
            set $combined "${1}_suffix";
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_error_when_no_question_mark() {
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_no_errors(
            r#"
http {
    server {
        location ~ ^/api/(.*)$ {
            rewrite ^/api/(.*)$ /internal/migrated;
            set $original_endpoint $1;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_error_when_set_precedes_rewrite() {
        // The rule narrowly targets the rewrite-then-set order, which is the
        // case that produces the dangerous buffer overrun (length-pass sees
        // is_args=0, copy-pass sees is_args=1). When `set` runs textually
        // before `rewrite`, the captured value is materialised while
        // is_args is still 0, so no buffer-size mismatch occurs at the
        // critical site. (The captured value may still be arg-escaped at
        // *read* time on vulnerable nginx — that's a separate mis-escape,
        // not the RCE-class overrun, and is intentionally out of scope.)
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_no_errors(
            r#"
http {
    server {
        location ~ ^/api/(.*)$ {
            set $original_endpoint $1;
            rewrite ^/api/(.*)$ /internal?migrated=true;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_error_when_no_unnamed_capture_after() {
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            rewrite ^/api/(.*)$ /internal?migrated=true;
            set $foo "static";
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_error_when_only_named_capture() {
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_no_errors(
            r#"
http {
    server {
        location ~ ^/api/(?<rest>.*)$ {
            rewrite ^/api/(?<rest>.*)$ /internal?migrated=true;
            set $original_endpoint $rest;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_no_error_when_subsequent_directive_in_different_block() {
        // Subsequent directive must be in the SAME block to trigger.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_no_errors(
            r#"
http {
    server {
        location ~ ^/api/(.*)$ {
            rewrite ^/api/(.*)$ /internal?migrated=true;
        }
        location /other {
            set $foo $1;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_error_location_points_to_rewrite() {
        TestCase::new(
            r#"
http {
    server {
        location ~ ^/api/(.*)$ {
            rewrite ^/api/(.*)$ /internal?migrated=true;
            set $original_endpoint $1;
        }
    }
}
"#,
        )
        .expect_error_count(1)
        .expect_error_on_line(5)
        .expect_message_contains("CVE-2026-42945")
        .run(&NginxRiftPlugin);
    }

    #[test]
    fn test_only_one_error_per_vulnerable_rewrite() {
        // Even with multiple subsequent capture users, we report once per rewrite.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_errors(
            r#"
http {
    server {
        location ~ ^/api/(.*)$ {
            rewrite ^/api/(.*)$ /internal?migrated=true;
            set $a $1;
            set $b $2;
            rewrite ^/x/(.*)$ /y/$1;
        }
    }
}
"#,
            1,
        );
    }

    #[test]
    fn test_capture_inside_quoted_string() {
        // $1 embedded in a quoted argument is still a capture reference.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_has_errors(
            r#"
http {
    server {
        location ~ ^/api/(.*)$ {
            rewrite ^/api/(.*)$ /internal?migrated=true;
            set $combined "prefix-$1-suffix";
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_capture_inside_single_quoted_string() {
        // nginx expands `$N` inside SINGLE-quoted strings as well as
        // double-quoted (verified empirically on nginx 1.30.0: a config
        // with `set $x 'prefix-$1-suffix'` after a vulnerable rewrite
        // returns `prefix-foo%2Bbar-suff` — same mis-escape + truncation
        // signature as the double-quoted form). The lint rule must
        // therefore flag both quote styles uniformly.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_has_errors(
            r#"
http {
    server {
        location ~ ^/api/(.*)$ {
            rewrite ^/api/(.*)$ /internal?migrated=true;
            set $combined 'prefix-$1-suffix';
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }

    #[test]
    fn test_autofix_basic_rewrite_then_set() {
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_fix_produces(
            r#"http {
  server {
    location ~ ^/api/(.*)$ {
      rewrite ^/api/(.*)$ /internal?migrated=true;
      set $original_endpoint $1;
    }
  }
}
"#,
            r#"http {
  server {
    location ~ ^/api/(.*)$ {
      rewrite ^/api/(?<cap1>.*)$ /internal?migrated=true;
      set $original_endpoint $cap1;
    }
  }
}
"#,
        );
    }

    #[test]
    fn test_autofix_rewrite_then_rewrite_with_capture() {
        // Second rewrite has its own regex; its $1 in the replacement
        // refers to its OWN capture, which we rename to cap1 locally.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_fix_produces(
            r#"http {
  server {
    location ~ ^/foo/(.*)$ {
      rewrite ^/foo/(.*)$ /bar?x=1;
      rewrite ^/bar/(.*)$ /baz/$1 last;
    }
  }
}
"#,
            r#"http {
  server {
    location ~ ^/foo/(.*)$ {
      rewrite ^/foo/(?<cap1>.*)$ /bar?x=1;
      rewrite ^/bar/(?<cap1>.*)$ /baz/$cap1 last;
    }
  }
}
"#,
        );
    }

    #[test]
    fn test_autofix_brace_form_of_unnamed_capture() {
        // ${1}_suffix must use the brace form on the renamed side too
        // (`${cap1}_suffix`) — and our rewriter also auto-promotes plain
        // `$1` to `${cap1}` when followed by a name-continuation byte.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_fix_produces(
            r#"http {
  server {
    location ~ ^/api/(.*)$ {
      rewrite ^/api/(.*)$ /internal?migrated=true;
      set $combined "${1}_suffix";
    }
  }
}
"#,
            r#"http {
  server {
    location ~ ^/api/(.*)$ {
      rewrite ^/api/(?<cap1>.*)$ /internal?migrated=true;
      set $combined "${cap1}_suffix";
    }
  }
}
"#,
        );
    }

    #[test]
    fn test_autofix_capture_inside_quoted_string() {
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_fix_produces(
            r#"http {
  server {
    location ~ ^/api/(.*)$ {
      rewrite ^/api/(.*)$ /internal?migrated=true;
      set $combined "prefix-$1-suffix";
    }
  }
}
"#,
            r#"http {
  server {
    location ~ ^/api/(.*)$ {
      rewrite ^/api/(?<cap1>.*)$ /internal?migrated=true;
      set $combined "prefix-$cap1-suffix";
    }
  }
}
"#,
        );
    }

    #[test]
    fn test_autofix_partial_when_consumer_exceeds_captures() {
        // The vulnerable rewrite has only 1 capture; `set $b $2` references
        // a non-existent positional capture. We MUST NOT rename `$2` to
        // `$cap2` (would create an undefined variable reference at
        // nginx-load time). The other fixes still apply.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_fix_produces(
            r#"http {
  server {
    location / {
      rewrite ^/api/(.*)$ /internal?migrated=true;
      set $a $1;
      set $b $2;
    }
  }
}
"#,
            r#"http {
  server {
    location / {
      rewrite ^/api/(?<cap1>.*)$ /internal?migrated=true;
      set $a $cap1;
      set $b $2;
    }
  }
}
"#,
        );
    }

    #[test]
    fn test_autofix_does_not_touch_other_blocks() {
        // A `set $foo $1` in a different scope must not be renamed —
        // its $1 binds to that scope's own (non-vulnerable) context.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_fix_produces(
            r#"http {
  server {
    location ~ ^/api/(.*)$ {
      rewrite ^/api/(.*)$ /internal?migrated=true;
      set $a $1;
    }
    location ~ ^/other/(.*)$ {
      set $b $1;
    }
  }
}
"#,
            r#"http {
  server {
    location ~ ^/api/(.*)$ {
      rewrite ^/api/(?<cap1>.*)$ /internal?migrated=true;
      set $a $cap1;
    }
    location ~ ^/other/(.*)$ {
      set $b $1;
    }
  }
}
"#,
        );
    }

    #[test]
    fn test_autofix_skips_non_capturing_groups_in_regex() {
        // `(?:...)` is non-capturing — keep it untouched. Real captures
        // remain numbered in source order (cap1, cap2).
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_fix_produces(
            r#"http {
  server {
    location / {
      rewrite ^/(?:api)/(.*)/(.*)$ /backend/$1/$2?migrated=true;
      set $a $1;
      set $b $2;
    }
  }
}
"#,
            r#"http {
  server {
    location / {
      rewrite ^/(?:api)/(?<cap1>.*)/(?<cap2>.*)$ /backend/$cap1/$cap2?migrated=true;
      set $a $cap1;
      set $b $cap2;
    }
  }
}
"#,
        );
    }

    #[test]
    fn test_find_unnamed_captures_skips_constructs() {
        // Direct check on the regex parser helper.
        assert_eq!(find_unnamed_capture_positions("^/api/(.*)$"), vec![6]);
        assert_eq!(find_unnamed_capture_positions("^/(?:api)/(.*)$"), vec![10]);
        assert_eq!(
            find_unnamed_capture_positions("^/(?<name>.*)/(.*)$"),
            vec![14]
        );
        assert_eq!(
            find_unnamed_capture_positions("^/(?P<name>.*)/(.*)$"),
            vec![15]
        );
        assert_eq!(
            find_unnamed_capture_positions(r"\(literal\)"),
            Vec::<usize>::new()
        );
        assert_eq!(find_unnamed_capture_positions("[()]"), Vec::<usize>::new());
        assert_eq!(
            find_unnamed_capture_positions("(a)(b)(?:c)(d)"),
            vec![0, 3, 11]
        );
        // PCRE control verbs like (*PRUNE) / (*FAIL) / (*MARK:tag) start
        // with `(*` and must not be treated as unnamed captures —
        // otherwise we'd emit `(?<cap1>*PRUNE)` which is invalid PCRE.
        assert_eq!(find_unnamed_capture_positions("(*PRUNE)(.*)"), vec![8]);
        assert_eq!(find_unnamed_capture_positions("(*MARK:tag)(.*)"), vec![11]);
    }

    #[test]
    fn test_regex_has_named_capture_handles_truncated_p_form() {
        // Regression: the old `i + 4 < bytes.len()` guard required a
        // byte we never read, so a regex ending exactly with `(?P<`
        // would slip through without bail-out. (nginx itself rejects
        // such malformed regex, but the helper should still answer
        // correctly.)
        assert!(regex_has_named_capture("(?P<"));
        // And the normal case still returns true.
        assert!(regex_has_named_capture("(?P<x>y)"));
    }

    #[test]
    fn test_autofix_multiple_vulnerable_rewrites_in_one_scope() {
        // Three vulnerable rewrites in a chain: each is detected
        // independently (so multiple errors are emitted), and the fix
        // ranges for the middle/last rewrites overlap between errors.
        // `apply_fixes_to_content` deduplicates by range, so the final
        // applied content is still consistent.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_fix_produces(
            r#"http {
  server {
    location / {
      rewrite ^/a/(.*)$ /b?x=$1;
      rewrite ^/b/(.*)$ /c?y=$1;
      rewrite ^/c/(.*)$ /d?z=$1;
    }
  }
}
"#,
            r#"http {
  server {
    location / {
      rewrite ^/a/(?<cap1>.*)$ /b?x=$cap1;
      rewrite ^/b/(?<cap1>.*)$ /c?y=$cap1;
      rewrite ^/c/(?<cap1>.*)$ /d?z=$cap1;
    }
  }
}
"#,
        );
    }

    #[test]
    fn test_autofix_output_is_rule_clean() {
        // Round-trip safety: applying the autofix to a complex
        // vulnerable pattern must produce content that no longer
        // triggers the rule. We verify by (1) confirming the autofix
        // output matches the expected good form, then (2) confirming
        // the good form itself produces zero rule errors.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        let bad = r#"http {
  server {
    location ~ ^/api/(.*)$ {
      rewrite ^/api/(.*)$ /internal?migrated=true;
      rewrite ^/x/(.*)$ /y/$1 last;
      set $original_endpoint $1;
      set $combined "prefix-$1-suffix";
    }
  }
}
"#;
        let good = r#"http {
  server {
    location ~ ^/api/(.*)$ {
      rewrite ^/api/(?<cap1>.*)$ /internal?migrated=true;
      rewrite ^/x/(?<cap1>.*)$ /y/$cap1 last;
      set $original_endpoint $cap1;
      set $combined "prefix-$cap1-suffix";
    }
  }
}
"#;
        runner.assert_fix_produces(bad, good);
        runner.assert_no_errors(good);
    }

    #[test]
    fn test_autofix_in_nested_block() {
        // Vulnerable pattern lives inside `if {}` nested in `location {}`.
        // The scope-walker must recurse into both block levels.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_fix_produces(
            r#"http {
  server {
    location / {
      if ($request_method = POST) {
        rewrite ^/api/(.*)$ /internal?migrated=true;
        set $original_endpoint $1;
      }
    }
  }
}
"#,
            r#"http {
  server {
    location / {
      if ($request_method = POST) {
        rewrite ^/api/(?<cap1>.*)$ /internal?migrated=true;
        set $original_endpoint $cap1;
      }
    }
  }
}
"#,
        );
    }

    #[test]
    fn test_autofix_with_named_capture_outside_vulnerable_scope() {
        // Mixed named/unnamed captures only trip our bail-out when they
        // sit in the rewrite directives we're rewriting. Named captures
        // in the location header (which we never touch) or named
        // variable references in `set` values (`$user`, not `$N`)
        // should be left untouched while the vulnerable chain itself
        // still gets autofixed.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_fix_produces(
            r#"http {
  server {
    location ~ ^/u/(?<user>\w+)/api/(.*)$ {
      rewrite ^/u/.+/api/(.*)$ /backend?migrated=true;
      set $endpoint $1;
      set $username $user;
    }
  }
}
"#,
            r#"http {
  server {
    location ~ ^/u/(?<user>\w+)/api/(.*)$ {
      rewrite ^/u/.+/api/(?<cap1>.*)$ /backend?migrated=true;
      set $endpoint $cap1;
      set $username $user;
    }
  }
}
"#,
        );
    }

    #[test]
    fn test_autofix_bails_when_v_rewrite_regex_has_named_capture() {
        // `(?<user>...)` in the vulnerable rewrite's regex makes
        // cap-numbering ambiguous: `$1` refers to `user` in PCRE, but
        // our cap-numbering would rename `$1` to `$cap1` and assign
        // `cap1` to the *next* unnamed group — a different capture.
        // To avoid silent semantic remap, we bail on the whole chain:
        // warning fires, no fix attached.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        let bad = r#"http {
  server {
    location / {
      rewrite ^/u/(?<user>\w+)/(.*)$ /api?user=$user;
      set $endpoint $1;
    }
  }
}
"#;
        let errors = runner.check_string(bad).expect("check");
        let spec_name = NginxRiftPlugin.spec().name;
        let rule_errors: Vec<_> = errors.iter().filter(|e| e.rule == spec_name).collect();
        assert_eq!(
            rule_errors.len(),
            1,
            "warning should still fire on the vulnerable pattern"
        );
        assert!(
            rule_errors[0].fixes.is_empty(),
            "no fix should be attached when v_rewrite regex mixes named/unnamed"
        );
    }

    #[test]
    fn test_autofix_handles_nested_unnamed_captures() {
        // Nested unnamed captures get numbered outer-first in source
        // order, which happens to match PCRE's positional numbering
        // (outer = group 1, inner = group 2). No special handling is
        // needed — `$1` -> `$cap1` and `$2` -> `$cap2` stay
        // semantically equivalent.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        runner.assert_fix_produces(
            r#"http {
  server {
    location / {
      rewrite ^/x/((.*))$ /target?z=1;
      set $outer $1;
      set $inner $2;
    }
  }
}
"#,
            r#"http {
  server {
    location / {
      rewrite ^/x/(?<cap1>(?<cap2>.*))$ /target?z=1;
      set $outer $cap1;
      set $inner $cap2;
    }
  }
}
"#,
        );
    }

    #[test]
    fn test_autofix_bails_on_nested_capture_when_any_layer_is_named() {
        // Once *any* layer of nesting is a named capture, the bail-out
        // fires — the same uniform conservatism we apply to flat
        // mixed regexes. Tested for both directions of nesting
        // (named-wraps-unnamed and unnamed-wraps-named) to make the
        // boundary explicit for readers.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        let rule_name = NginxRiftPlugin.spec().name;

        for bad in [
            // Named outer, unnamed inner.
            r#"http {
  server {
    location / {
      rewrite ^/x/(?<outer>(.*))$ /target?z=$outer;
      set $endpoint $1;
    }
  }
}
"#,
            // Unnamed outer, named inner.
            r#"http {
  server {
    location / {
      rewrite ^/x/((?<inner>.*))$ /target?z=$inner;
      set $endpoint $1;
    }
  }
}
"#,
        ] {
            let errors = runner.check_string(bad).expect("check");
            let rule_errors: Vec<_> = errors.iter().filter(|e| e.rule == rule_name).collect();
            assert_eq!(
                rule_errors.len(),
                1,
                "warning must still fire on nested mixed-capture; config was:\n{}",
                bad
            );
            assert!(
                rule_errors[0].fixes.is_empty(),
                "nested regex with any named layer must bail; got fixes {:?} for:\n{}",
                rule_errors[0].fixes,
                bad
            );
        }
    }

    #[test]
    fn test_autofix_bails_on_mixed_named_and_unnamed_in_same_regex() {
        // The cornerstone case for the bail-out: a single regex that
        // contains BOTH a named capture AND an unnamed one. Users
        // hitting this in the wild will want to know what the autofix
        // does. Answer: nothing — the warning fires, no fix is
        // attached, and they fix it manually.
        //
        // Both orderings are tested:
        //
        //  - named first (`(?<user>\w+)/(.*)`): a downstream `$1` in
        //    PCRE refers to the *named* `user` group, but our
        //    cap-numbering would assign `cap1` to the *unnamed* `(.*)`
        //    (positional group 2). Renaming `$1` -> `$cap1` would
        //    silently point the consumer at a different capture.
        //
        //  - unnamed first (`(.*)/(?<rest>\w+)`): here `$1` does
        //    actually refer to the unnamed `(.*)` (positional group
        //    1), so renaming would *coincidentally* be correct. We
        //    still bail — the helper has no way to distinguish the
        //    safe ordering from the unsafe one without a full PCRE
        //    parser, and it's better to be uniformly conservative
        //    than to ship a partial heuristic that handles one
        //    ordering and not the other.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        let rule_name = NginxRiftPlugin.spec().name;

        for bad in [
            // Named first, then unnamed.
            r#"http {
  server {
    location / {
      rewrite ^/u/(?<user>\w+)/(.*)$ /api?user=$user;
      set $endpoint $1;
    }
  }
}
"#,
            // Unnamed first, then named.
            r#"http {
  server {
    location / {
      rewrite ^/(.*)/(?<rest>\w+)$ /api?path=$rest;
      set $endpoint $1;
    }
  }
}
"#,
        ] {
            let errors = runner.check_string(bad).expect("check");
            let rule_errors: Vec<_> = errors.iter().filter(|e| e.rule == rule_name).collect();
            assert_eq!(
                rule_errors.len(),
                1,
                "warning must still fire on mixed-capture regex; config was:\n{}",
                bad
            );
            assert!(
                rule_errors[0].fixes.is_empty(),
                "mixed named + unnamed in same regex must bail regardless of order; \
                 got fixes {:?} for config:\n{}",
                rule_errors[0].fixes,
                bad
            );
        }
    }

    #[test]
    fn test_autofix_bails_when_v_rewrite_regex_has_python_style_named_capture() {
        // Same bail-out as the `(?<name>...)` form, but via the
        // `(?P<name>...)` Python-style syntax. The helper unit test
        // (`test_regex_has_named_capture_helper`) confirms the detector
        // recognizes both forms; this is the end-to-end check that the
        // detector actually wires through to the fix builder and
        // suppresses fix emission.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        let bad = r#"http {
  server {
    location / {
      rewrite ^/u/(?P<user>\w+)/(.*)$ /api?user=$user;
      set $endpoint $1;
    }
  }
}
"#;
        let errors = runner.check_string(bad).expect("check");
        let spec_name = NginxRiftPlugin.spec().name;
        let rule_errors: Vec<_> = errors.iter().filter(|e| e.rule == spec_name).collect();
        assert_eq!(rule_errors.len(), 1, "warning should still fire");
        assert!(
            rule_errors[0].fixes.is_empty(),
            "(?P<name>...) in v_rewrite must also bail out, got fixes: {:?}",
            rule_errors[0].fixes
        );
    }

    #[test]
    fn test_autofix_no_fixes_attached_to_unfixable_consumer_rewrite() {
        // The complement to `test_autofix_skips_consumer_rewrite_with_named_capture`:
        // that test verifies the *resulting content* via string compare,
        // which means a bug that emits a wrong-but-harmless fix could
        // slip through. Here we look at the fix list directly and
        // confirm we only emit fixes for the v_rewrite + the early
        // `set` — nothing targets the unfixable consumer's regex,
        // its replacement, or the downstream `set`.
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        let bad = r#"http {
  server {
    location / {
      rewrite ^/api/(.*)$ /internal?migrated=true;
      set $a $1;
      rewrite ^/u/(?<user>\w+)/(.*)$ /next/$1 last;
      set $b $1;
    }
  }
}
"#;
        let errors = runner.check_string(bad).expect("check");
        let spec_name = NginxRiftPlugin.spec().name;
        let rule_errors: Vec<_> = errors.iter().filter(|e| e.rule == spec_name).collect();
        assert_eq!(rule_errors.len(), 1, "single warning expected");

        // Expect exactly 2 fixes: v_rewrite's regex + the first `set`'s
        // value. The consumer rewrite (regex + replacement) and the
        // trailing `set` must contribute 0 fixes.
        let fix_texts: Vec<&str> = rule_errors[0]
            .fixes
            .iter()
            .map(|f| f.new_text.as_str())
            .collect();
        assert_eq!(
            rule_errors[0].fixes.len(),
            2,
            "expected exactly 2 fixes (v_rewrite regex + first set value), got: {:?}",
            fix_texts
        );
        // Sanity: every emitted fix is a `cap1`-renaming. None should
        // contain `cap` for the unfixable consumer (which has a `user`
        // named group, not a `cap` group).
        assert!(
            rule_errors[0]
                .fixes
                .iter()
                .all(|f| f.new_text.contains("cap1")),
            "every fix should be a cap1-renaming, got: {:?}",
            fix_texts
        );
    }

    #[test]
    fn test_autofix_skips_consumer_rewrite_with_named_capture() {
        // The vulnerable rewrite itself is autofixable, but a later
        // consumer rewrite uses a named capture, so we (a) skip
        // renaming that consumer's regex/replacement, and (b) force
        // `active_caps = 0` so a downstream `set $foo $1` is NOT
        // renamed (its `$1` references the unfixable rewrite's
        // captures, which we have no name for).
        let runner = PluginTestRunner::new(NginxRiftPlugin);
        let bad = r#"http {
  server {
    location / {
      rewrite ^/api/(.*)$ /internal?migrated=true;
      set $a $1;
      rewrite ^/u/(?<user>\w+)/(.*)$ /next/$1 last;
      set $b $1;
    }
  }
}
"#;
        let good = r#"http {
  server {
    location / {
      rewrite ^/api/(?<cap1>.*)$ /internal?migrated=true;
      set $a $cap1;
      rewrite ^/u/(?<user>\w+)/(.*)$ /next/$1 last;
      set $b $1;
    }
  }
}
"#;
        runner.assert_fix_produces(bad, good);
    }

    #[test]
    fn test_regex_has_named_capture_helper() {
        // Plain named captures.
        assert!(regex_has_named_capture("(?<name>.*)"));
        assert!(regex_has_named_capture("^/api/(?<rest>.*)$"));
        assert!(regex_has_named_capture("(?P<name>.*)"));
        assert!(regex_has_named_capture("(?<a>x)(.)"));
        // Lookbehinds (NOT named captures).
        assert!(!regex_has_named_capture("(?<=foo)bar"));
        assert!(!regex_has_named_capture("(?<!foo)bar"));
        // Unnamed-only.
        assert!(!regex_has_named_capture("(.*)(.*)"));
        assert!(!regex_has_named_capture("^/api/(.*)$"));
        // Non-capturing groups and escapes.
        assert!(!regex_has_named_capture("(?:foo)(bar)"));
        assert!(!regex_has_named_capture(r"\(?<not_a_capture>\)"));
        // Inside character class — bracket eats the `(?<`.
        assert!(!regex_has_named_capture("[(?<x>]"));
    }

    #[test]
    fn test_rename_positional_refs_bail_when_exceeding_captures() {
        // $5 with max_captures=1 must return None.
        assert!(rename_positional_refs_in_raw("$5", 1).is_none());
        assert!(rename_positional_refs_in_raw("${5}", 1).is_none());
        assert!(rename_positional_refs_in_raw("prefix-$5-suffix", 1).is_none());

        // No-op cases.
        assert_eq!(
            rename_positional_refs_in_raw("static", 1).as_deref(),
            Some("static")
        );
        assert_eq!(
            rename_positional_refs_in_raw("$foo", 1).as_deref(),
            Some("$foo")
        );
    }

    #[test]
    fn test_spec_declares_affected_version_range() {
        // The CVE affects nginx 0.6.27 through 1.30.0 (fixed in 1.30.1 / 1.31.0).
        // The version range on the spec drives nginx-lint's automatic
        // version-based rule filter, so a target_nginx_version of 1.30.1+
        // skips this rule by default.
        let spec = NginxRiftPlugin.spec();
        assert_eq!(spec.min_nginx_version.as_deref(), Some("0.6.27"));
        assert_eq!(spec.max_nginx_version.as_deref(), Some("1.30.0"));
    }
}
