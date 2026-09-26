//! The documentation a native rule declares about itself, as a `static`
//! (`pub static DOC: RuleDoc`). [`crate::docs`] collects these and adds the
//! plugins' documentation to them.

/// Documentation for a lint rule (static version for native rules)
pub struct RuleDoc {
    /// Rule name (e.g., "server-tokens-enabled")
    pub name: &'static str,
    /// Category (e.g., "security")
    pub category: &'static str,
    /// Short description
    pub description: &'static str,
    /// Severity level
    pub severity: &'static str,
    /// Why this rule exists
    pub why: &'static str,
    /// Example of bad configuration
    pub bad_example: &'static str,
    /// Example of good configuration
    pub good_example: &'static str,
    /// References (URLs, documentation links)
    pub references: &'static [&'static str],
    /// Minimum nginx version this rule applies to (inclusive), if declared.
    pub min_nginx_version: Option<&'static str>,
    /// Maximum nginx version this rule applies to (inclusive), if declared.
    pub max_nginx_version: Option<&'static str>,
}

impl RuleDoc {
    /// Field defaults for use with Rust's struct-update syntax:
    ///
    /// ```ignore
    /// pub static DOC: RuleDoc = RuleDoc {
    ///     name: "my-rule",
    ///     // ...required fields...
    ///     ..RuleDoc::DEFAULTS
    /// };
    /// ```
    ///
    /// Currently only the optional `min_nginx_version` / `max_nginx_version`
    /// fields have meaningful defaults; the rest are empty placeholders that
    /// you should override. Future additive fields with sensible defaults
    /// can be added here so existing DOC literals automatically pick them
    /// up via `..RuleDoc::DEFAULTS` without each call site needing edits.
    pub const DEFAULTS: RuleDoc = RuleDoc {
        name: "",
        category: "",
        description: "",
        severity: "",
        why: "",
        bad_example: "",
        good_example: "",
        references: &[],
        min_nginx_version: None,
        max_nginx_version: None,
    };
}
