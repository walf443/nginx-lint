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
            for next in directives.iter().skip(i + 1) {
                if is_capture_consumer(next) && uses_unnamed_capture(next) {
                    errors.push(err.warning_at(
                        "CVE-2026-42945: `rewrite` replacement contains `?` and is \
                         followed by a directive that references an unnamed capture \
                         ($1..$9) in the same scope — this triggers a heap buffer \
                         overflow on nginx <1.30.1/<1.31.0. Upgrade nginx, switch to \
                         named captures, or remove `?` from the replacement.",
                        *dir,
                    ));
                    break;
                }
            }
        }

        if let Some(block) = &dir.block {
            check_items(&block.items, errors, err);
        }
    }
}

fn is_rewrite_with_question_mark(dir: &Directive) -> bool {
    if !dir.is("rewrite") || dir.args.len() < 2 {
        return false;
    }
    dir.args[1].as_str().contains('?')
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
