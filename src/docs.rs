//! Rule documentation for nginx-lint
//!
//! This module provides detailed documentation for each lint rule,
//! explaining why the rule exists and what the recommended configuration is.

/// Documentation for a lint rule
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
}

/// Get documentation for a rule by name
pub fn get_rule_doc(name: &str) -> Option<&'static RuleDoc> {
    all_rule_docs().iter().find(|doc| doc.name == name).copied()
}

/// Get all rule documentation
pub fn all_rule_docs() -> &'static [&'static RuleDoc] {
    use crate::rules::{
        best_practices::{gzip_not_enabled, missing_error_log},
        security::{autoindex_enabled, deprecated_ssl_protocol, server_tokens_enabled, weak_ssl_ciphers},
        style::{indent, space_before_semicolon, trailing_whitespace},
        syntax::{duplicate_directive, missing_semicolon, unclosed_quote, unmatched_braces},
    };

    static DOCS: &[&RuleDoc] = &[
        // Security
        &server_tokens_enabled::DOC,
        &autoindex_enabled::DOC,
        &deprecated_ssl_protocol::DOC,
        &weak_ssl_ciphers::DOC,
        // Syntax
        &unmatched_braces::DOC,
        &unclosed_quote::DOC,
        &missing_semicolon::DOC,
        &duplicate_directive::DOC,
        // Style
        &indent::DOC,
        &trailing_whitespace::DOC,
        &space_before_semicolon::DOC,
        // Best Practices
        &gzip_not_enabled::DOC,
        &missing_error_log::DOC,
    ];

    DOCS
}

/// Get all rule names
pub fn all_rule_names() -> Vec<&'static str> {
    all_rule_docs().iter().map(|doc| doc.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_rule_doc() {
        let doc = get_rule_doc("server-tokens-enabled");
        assert!(doc.is_some());
        let doc = doc.unwrap();
        assert_eq!(doc.name, "server-tokens-enabled");
        assert_eq!(doc.category, "security");
    }

    #[test]
    fn test_get_rule_doc_not_found() {
        let doc = get_rule_doc("nonexistent-rule");
        assert!(doc.is_none());
    }

    #[test]
    fn test_all_rule_names() {
        let names = all_rule_names();
        assert!(names.contains(&"server-tokens-enabled"));
        assert!(names.contains(&"indent"));
        assert!(names.contains(&"gzip-not-enabled"));
    }
}
