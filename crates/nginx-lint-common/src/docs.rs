//! Rule documentation types for nginx-lint
//!
//! This module provides type definitions for lint rule documentation.

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
}

/// Documentation for a lint rule (owned version, supports plugins)
#[derive(Debug, Clone)]
pub struct RuleDocOwned {
    /// Rule name (e.g., "server-tokens-enabled")
    pub name: String,
    /// Category (e.g., "security")
    pub category: String,
    /// Short description
    pub description: String,
    /// Severity level
    pub severity: String,
    /// Why this rule exists
    pub why: String,
    /// Example of bad configuration
    pub bad_example: String,
    /// Example of good configuration
    pub good_example: String,
    /// References (URLs, documentation links)
    pub references: Vec<String>,
    /// Whether this is from a plugin
    pub is_plugin: bool,
}

impl From<&RuleDoc> for RuleDocOwned {
    fn from(doc: &RuleDoc) -> Self {
        Self {
            name: doc.name.to_string(),
            category: doc.category.to_string(),
            description: doc.description.to_string(),
            severity: doc.severity.to_string(),
            why: doc.why.to_string(),
            bad_example: doc.bad_example.to_string(),
            good_example: doc.good_example.to_string(),
            references: doc.references.iter().map(|s| s.to_string()).collect(),
            is_plugin: false,
        }
    }
}
