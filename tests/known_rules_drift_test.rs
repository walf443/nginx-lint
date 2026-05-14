//! Drift-detection tests for `nginx-lint config validate`.
//!
//! Background (https://github.com/walf443/nginx-lint/issues/172):
//! `LintConfig::KNOWN_RULES` (in `nginx-lint-common`) is the whitelist used
//! by `nginx-lint config validate`. It must include every builtin plugin
//! name, otherwise users get a spurious `unknown rule` error for a config
//! that lints correctly.
//!
//! Because `nginx-lint-common` is a downstream crate, it cannot import
//! `BUILTIN_PLUGIN_NAMES` directly. The drift test therefore lives here,
//! in the top-level crate, where both sources of truth are visible.

#![cfg(any(feature = "plugins", feature = "native-builtin-plugins"))]

use nginx_lint::LintConfig;
use nginx_lint::plugin::BUILTIN_PLUGIN_NAMES;

#[test]
fn known_rules_includes_all_builtin_plugins() {
    let missing: Vec<&str> = BUILTIN_PLUGIN_NAMES
        .iter()
        .copied()
        .filter(|name| !LintConfig::KNOWN_RULES.contains(name))
        .collect();

    assert!(
        missing.is_empty(),
        "BUILTIN_PLUGIN_NAMES contains rules that are NOT in \
         `LintConfig::KNOWN_RULES`: {missing:?}. \
         `nginx-lint config validate` would reject user configs that \
         reference these rules, even though they lint correctly. \
         Add them to `KNOWN_RULES` in crates/nginx-lint-common/src/config.rs.",
    );
}

#[test]
fn known_rules_does_not_reference_removed_builtin_plugins() {
    // The reverse direction: if a rule name listed in `KNOWN_RULES` looks
    // like a builtin plugin (i.e. is not one of the hand-maintained native
    // rule names) but is no longer present in `BUILTIN_PLUGIN_NAMES`, it
    // was probably renamed or deleted and the whitelist has gone stale.
    const NATIVE_RULES: &[&str] = &[
        "unmatched-braces",
        "unclosed-quote",
        "missing-semicolon",
        "indent",
        "include-path-exists",
    ];

    let stale: Vec<&&str> = LintConfig::KNOWN_RULES
        .iter()
        .filter(|name| !NATIVE_RULES.contains(name) && !BUILTIN_PLUGIN_NAMES.contains(name))
        .collect();

    assert!(
        stale.is_empty(),
        "KNOWN_RULES contains entries that are neither native rules nor \
         current builtin plugins: {stale:?}. Either add them back to \
         BUILTIN_PLUGIN_NAMES (if they still exist) or remove them from \
         KNOWN_RULES.",
    );
}
