//! Rule documentation for nginx-lint
//!
//! This module provides detailed documentation for each lint rule,
//! explaining why the rule exists and what the recommended configuration is.

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

/// Get documentation for a rule by name
pub fn get_rule_doc(name: &str) -> Option<&'static RuleDoc> {
    all_rule_docs().iter().find(|doc| doc.name == name).copied()
}

/// Get all rule documentation (native rules only)
/// Rule documentation for include-path-exists (cli-only rule, but docs are always available)
static INCLUDE_PATH_EXISTS_DOC: RuleDoc = RuleDoc {
    name: "include-path-exists",
    category: "syntax",
    description: "Detects include directives that reference non-existent files",
    severity: "error",
    why: r#"When an include directive references a file that does not exist,
nginx will fail to start. Glob patterns that match no files are
accepted by nginx but may indicate a misconfiguration."#,
    bad_example: include_str!("rules/syntax/include_path_exists/bad.conf"),
    good_example: include_str!("rules/syntax/include_path_exists/good.conf"),
    references: &["https://nginx.org/en/docs/ngx_core_module.html#include"],
};

pub fn all_rule_docs() -> &'static [&'static RuleDoc] {
    use crate::rules::{
        style::indent,
        syntax::{invalid_directive_context, missing_semicolon, unclosed_quote, unmatched_braces},
    };

    static DOCS: &[&RuleDoc] = &[
        // Syntax
        &unmatched_braces::DOC,
        &unclosed_quote::DOC,
        &missing_semicolon::DOC,
        &invalid_directive_context::DOC,
        &INCLUDE_PATH_EXISTS_DOC,
        // Style
        &indent::DOC,
    ];

    DOCS
}

/// Get all rule names
pub fn all_rule_names() -> Vec<&'static str> {
    all_rule_docs().iter().map(|doc| doc.name).collect()
}

/// Get all rule documentation including plugins (owned version)
#[cfg(any(feature = "wasm-builtin-plugins", feature = "native-builtin-plugins"))]
pub fn all_rule_docs_with_plugins() -> Vec<RuleDocOwned> {
    let mut docs: Vec<RuleDocOwned> = all_rule_docs().iter().map(|d| (*d).into()).collect();
    docs.extend(get_builtin_plugin_docs());
    docs
}

/// Get documentation for a rule by name, including plugins
#[cfg(any(feature = "wasm-builtin-plugins", feature = "native-builtin-plugins"))]
pub fn get_rule_doc_with_plugins(name: &str) -> Option<RuleDocOwned> {
    // First check native rules
    if let Some(doc) = get_rule_doc(name) {
        return Some(doc.into());
    }
    // Then check builtin plugins
    get_builtin_plugin_docs()
        .into_iter()
        .find(|d| d.name == name)
}

/// Get documentation from native plugins
#[cfg(feature = "native-builtin-plugins")]
fn get_builtin_plugin_docs() -> Vec<RuleDocOwned> {
    use crate::linter::LintRule;
    use crate::plugin::native_builtin::load_native_builtin_plugins;

    let plugins: Vec<Box<dyn LintRule>> = load_native_builtin_plugins();
    plugins
        .into_iter()
        .map(|rule| RuleDocOwned {
            name: rule.name().to_string(),
            category: rule.category().to_string(),
            description: rule.description().to_string(),
            severity: rule.severity().unwrap_or("warning").to_string(),
            why: rule.why().unwrap_or("").to_string(),
            bad_example: rule.bad_example().unwrap_or("").to_string(),
            good_example: rule.good_example().unwrap_or("").to_string(),
            references: rule.references().unwrap_or_default(),
            is_plugin: true,
        })
        .collect()
}

/// Get documentation from WASM builtin plugins
#[cfg(all(
    feature = "wasm-builtin-plugins",
    not(feature = "native-builtin-plugins")
))]
fn get_builtin_plugin_docs() -> Vec<RuleDocOwned> {
    use crate::linter::LintRule;
    use crate::plugin::builtin::load_builtin_plugins;

    let mut docs = Vec::new();

    if let Ok(plugins) = load_builtin_plugins() {
        for plugin in plugins {
            docs.push(RuleDocOwned {
                name: plugin.name().to_string(),
                category: plugin.category().to_string(),
                description: plugin.description().to_string(),
                severity: plugin.severity().unwrap_or("warning").to_string(),
                why: plugin.why().unwrap_or("").to_string(),
                bad_example: plugin.bad_example().unwrap_or("").to_string(),
                good_example: plugin.good_example().unwrap_or("").to_string(),
                references: plugin.references().unwrap_or_default(),
                is_plugin: true,
            });
        }
    }

    docs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_rule_doc() {
        let doc = get_rule_doc("indent");
        assert!(doc.is_some());
        let doc = doc.unwrap();
        assert_eq!(doc.name, "indent");
        assert_eq!(doc.category, "style");
    }

    #[test]
    fn test_get_rule_doc_not_found() {
        let doc = get_rule_doc("nonexistent-rule");
        assert!(doc.is_none());
    }

    #[test]
    fn test_all_rule_names() {
        let names = all_rule_names();
        assert!(names.contains(&"indent"));
        assert!(names.contains(&"unmatched-braces"));
        assert!(names.contains(&"invalid-directive-context"));
    }
}

/// Tests that verify example files produce expected lint results
#[cfg(test)]
mod example_tests {
    use super::*;
    use crate::linter::{Fix, Linter};
    use nginx_lint_common::parse_string;
    use std::path::Path;

    /// Apply a single line-based fix to content and return the result
    fn apply_line_fix(content: &str, fix: &Fix) -> String {
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

        if fix.line == 0 || fix.line > lines.len() + 1 {
            return lines.join("\n");
        }

        if fix.insert_after {
            let insert_idx = fix.line.min(lines.len());
            lines.insert(insert_idx, fix.new_text.clone());
        } else if fix.delete_line {
            if fix.line <= lines.len() {
                lines.remove(fix.line - 1);
            }
        } else if let Some(old_text) = &fix.old_text {
            if fix.line <= lines.len() && lines[fix.line - 1].contains(old_text) {
                lines[fix.line - 1] = lines[fix.line - 1].replace(old_text, &fix.new_text);
            }
        } else if fix.line <= lines.len() {
            lines[fix.line - 1] = fix.new_text.clone();
        }

        lines.join("\n")
    }

    /// Apply all fixes to content (supports both range-based and line-based fixes)
    fn apply_fixes(content: &str, errors: &[crate::linter::LintError]) -> String {
        let fixes: Vec<&Fix> = errors.iter().flat_map(|e| e.fixes.iter()).collect();

        // Separate range-based and line-based fixes
        let (range_fixes, line_fixes): (Vec<&&Fix>, Vec<&&Fix>) = fixes
            .iter()
            .partition(|f| f.start_offset.is_some() && f.end_offset.is_some());

        let mut result = content.to_string();

        // Apply range-based fixes first (sort by start_offset descending)
        if !range_fixes.is_empty() {
            let mut sorted_range_fixes = range_fixes;
            sorted_range_fixes
                .sort_by(|a, b| b.start_offset.unwrap().cmp(&a.start_offset.unwrap()));

            let mut applied_ranges: Vec<(usize, usize)> = Vec::new();

            for fix in sorted_range_fixes {
                let start = fix.start_offset.unwrap();
                let end = fix.end_offset.unwrap();

                let overlaps = applied_ranges.iter().any(|(s, e)| start < *e && end > *s);
                if overlaps {
                    continue;
                }

                if start <= result.len() && end <= result.len() && start <= end {
                    result.replace_range(start..end, &fix.new_text);
                    applied_ranges.push((start, start + fix.new_text.len()));
                }
            }
        }

        // Apply line-based fixes
        if !line_fixes.is_empty() {
            let mut sorted_line_fixes = line_fixes;
            sorted_line_fixes.sort_by(|a, b| match b.line.cmp(&a.line) {
                std::cmp::Ordering::Equal if a.insert_after && b.insert_after => {
                    let a_indent = a.new_text.len() - a.new_text.trim_start().len();
                    let b_indent = b.new_text.len() - b.new_text.trim_start().len();
                    a_indent.cmp(&b_indent)
                }
                other => other,
            });

            for fix in sorted_line_fixes {
                result = apply_line_fix(&result, fix);
            }
        }

        result
    }

    /// Test that each rule's bad_example produces at least one error from that rule
    #[test]
    fn test_bad_examples_produce_errors() {
        let linter = Linter::with_default_rules();
        let dummy_path = Path::new("test.conf");

        for doc in all_rule_docs() {
            // Skip rules that check for file-level issues that can't be tested via parse_string
            // (e.g., missing-error-log only checks for presence/absence)
            if doc.name == "missing-error-log" {
                continue;
            }

            // Skip style rules - they check content directly and are tested in test_style_bad_examples
            if doc.category == "style" {
                continue;
            }

            // Skip syntax rules that check content directly - tested in test_syntax_bad_examples
            if doc.name == "unclosed-quote"
                || doc.name == "missing-semicolon"
                || doc.name == "unmatched-braces"
            {
                continue;
            }

            // Skip rules that require real filesystem access
            if doc.name == "include-path-exists" {
                continue;
            }

            // Parse bad example and check for errors
            let config = match parse_string(doc.bad_example) {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                        "Rule '{}': bad_example failed to parse: {}\nExample:\n{}",
                        doc.name, e, doc.bad_example
                    );
                }
            };

            let errors = linter.lint(&config, dummy_path);
            let rule_errors: Vec<_> = errors.iter().filter(|e| e.rule == doc.name).collect();

            assert!(
                !rule_errors.is_empty(),
                "Rule '{}': bad_example should produce at least one error\nExample:\n{}",
                doc.name,
                doc.bad_example
            );
        }
    }

    /// Test that each rule's good_example produces no errors from that rule
    #[test]
    fn test_good_examples_produce_no_errors() {
        let linter = Linter::with_default_rules();
        let dummy_path = Path::new("test.conf");

        for doc in all_rule_docs() {
            // Skip style rules - they check content directly and are tested in test_style_good_examples
            if doc.category == "style" {
                continue;
            }

            // Skip syntax rules that check content directly - tested in test_syntax_good_examples
            if doc.name == "unclosed-quote"
                || doc.name == "missing-semicolon"
                || doc.name == "unmatched-braces"
            {
                continue;
            }

            // Skip rules that require real filesystem access
            if doc.name == "include-path-exists" {
                continue;
            }

            // Parse good example
            let config = match parse_string(doc.good_example) {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                        "Rule '{}': good_example failed to parse: {}\nExample:\n{}",
                        doc.name, e, doc.good_example
                    );
                }
            };

            let errors = linter.lint(&config, dummy_path);
            let rule_errors: Vec<_> = errors.iter().filter(|e| e.rule == doc.name).collect();

            assert!(
                rule_errors.is_empty(),
                "Rule '{}': good_example should not produce any errors, but got {:?}\nExample:\n{}",
                doc.name,
                rule_errors,
                doc.good_example
            );
        }
    }

    /// Test bad examples for style rules that work on content directly
    #[test]
    fn test_style_bad_examples() {
        use crate::rules::style::indent::Indent;

        // Test indent
        {
            let doc = get_rule_doc("indent").unwrap();
            let rule = Indent::default();
            let errors = rule.check_content(doc.bad_example);
            assert!(
                !errors.is_empty(),
                "indent bad_example should produce errors:\n{}",
                doc.bad_example
            );
        }
    }

    /// Test good examples for style rules that work on content directly
    #[test]
    fn test_style_good_examples() {
        use crate::rules::style::indent::Indent;

        // Test indent
        {
            let doc = get_rule_doc("indent").unwrap();
            let rule = Indent::default();
            let errors = rule.check_content(doc.good_example);
            assert!(
                errors.is_empty(),
                "indent good_example should not produce errors, but got {:?}:\n{}",
                errors,
                doc.good_example
            );
        }
    }

    /// Test bad examples for syntax rules that check content directly
    #[test]
    fn test_syntax_bad_examples() {
        use crate::rules::syntax::{
            missing_semicolon::MissingSemicolon, unclosed_quote::UnclosedQuote,
            unmatched_braces::UnmatchedBraces,
        };

        // Test unmatched-braces
        {
            let doc = get_rule_doc("unmatched-braces").unwrap();
            let rule = UnmatchedBraces;
            let errors = rule.check_content(doc.bad_example);
            assert!(
                !errors.is_empty(),
                "unmatched-braces bad_example should produce errors:\n{}",
                doc.bad_example
            );
        }

        // Test unclosed-quote
        {
            let doc = get_rule_doc("unclosed-quote").unwrap();
            let rule = UnclosedQuote;
            let errors = rule.check_content(doc.bad_example);
            assert!(
                !errors.is_empty(),
                "unclosed-quote bad_example should produce errors:\n{}",
                doc.bad_example
            );
        }

        // Test missing-semicolon
        {
            let doc = get_rule_doc("missing-semicolon").unwrap();
            let rule = MissingSemicolon;
            let errors = rule.check_content(doc.bad_example);
            assert!(
                !errors.is_empty(),
                "missing-semicolon bad_example should produce errors:\n{}",
                doc.bad_example
            );
        }
    }

    /// Test good examples for syntax rules that check content directly
    #[test]
    fn test_syntax_good_examples() {
        use crate::rules::syntax::{
            missing_semicolon::MissingSemicolon, unclosed_quote::UnclosedQuote,
            unmatched_braces::UnmatchedBraces,
        };

        // Test unmatched-braces
        {
            let doc = get_rule_doc("unmatched-braces").unwrap();
            let rule = UnmatchedBraces;
            let errors = rule.check_content(doc.good_example);
            assert!(
                errors.is_empty(),
                "unmatched-braces good_example should not produce errors, but got {:?}:\n{}",
                errors,
                doc.good_example
            );
        }

        // Test unclosed-quote
        {
            let doc = get_rule_doc("unclosed-quote").unwrap();
            let rule = UnclosedQuote;
            let errors = rule.check_content(doc.good_example);
            assert!(
                errors.is_empty(),
                "unclosed-quote good_example should not produce errors, but got {:?}:\n{}",
                errors,
                doc.good_example
            );
        }

        // Test missing-semicolon
        {
            let doc = get_rule_doc("missing-semicolon").unwrap();
            let rule = MissingSemicolon;
            let errors = rule.check_content(doc.good_example);
            assert!(
                errors.is_empty(),
                "missing-semicolon good_example should not produce errors, but got {:?}:\n{}",
                errors,
                doc.good_example
            );
        }
    }

    /// Test that applying fixes to style bad examples produces the good example
    #[test]
    fn test_style_fixes_produce_good_examples() {
        use crate::rules::style::indent::Indent;

        // Test indent fix
        {
            let doc = get_rule_doc("indent").unwrap();
            let rule = Indent::default();
            let errors = rule.check_content(doc.bad_example);
            if !errors.is_empty() && errors.iter().all(|e| !e.fixes.is_empty()) {
                let fixed = apply_fixes(doc.bad_example, &errors);
                let expected = doc.good_example.trim_end();
                let actual = fixed.trim_end();
                assert_eq!(
                    actual, expected,
                    "indent: applying fixes to bad_example should produce good_example\n\
                     Bad example:\n{}\n\
                     Fixed:\n{}\n\
                     Expected:\n{}",
                    doc.bad_example, fixed, doc.good_example
                );
            }
        }
    }

    /// Test that applying fixes to syntax bad examples produces the good example
    #[test]
    fn test_syntax_fixes_produce_good_examples() {
        use crate::rules::syntax::{
            missing_semicolon::MissingSemicolon, unclosed_quote::UnclosedQuote,
            unmatched_braces::UnmatchedBraces,
        };

        // Test unmatched-braces fix
        {
            let doc = get_rule_doc("unmatched-braces").unwrap();
            let rule = UnmatchedBraces;
            let errors = rule.check_content(doc.bad_example);
            if !errors.is_empty() && errors.iter().all(|e| !e.fixes.is_empty()) {
                let fixed = apply_fixes(doc.bad_example, &errors);
                let expected = doc.good_example.trim_end();
                let actual = fixed.trim_end();
                assert_eq!(
                    actual, expected,
                    "unmatched-braces: applying fixes to bad_example should produce good_example\n\
                     Bad example:\n{}\n\
                     Fixed:\n{}\n\
                     Expected:\n{}",
                    doc.bad_example, fixed, doc.good_example
                );
            }
        }

        // Test unclosed-quote fix
        // Note: unclosed-quote may not have automatic fixes
        {
            let doc = get_rule_doc("unclosed-quote").unwrap();
            let rule = UnclosedQuote;
            let errors = rule.check_content(doc.bad_example);
            if !errors.is_empty() && errors.iter().all(|e| !e.fixes.is_empty()) {
                let fixed = apply_fixes(doc.bad_example, &errors);
                let expected = doc.good_example.trim_end();
                let actual = fixed.trim_end();
                assert_eq!(
                    actual, expected,
                    "unclosed-quote: applying fixes to bad_example should produce good_example\n\
                     Bad example:\n{}\n\
                     Fixed:\n{}\n\
                     Expected:\n{}",
                    doc.bad_example, fixed, doc.good_example
                );
            }
        }

        // Test missing-semicolon fix
        {
            let doc = get_rule_doc("missing-semicolon").unwrap();
            let rule = MissingSemicolon;
            let errors = rule.check_content(doc.bad_example);
            if !errors.is_empty() && errors.iter().all(|e| !e.fixes.is_empty()) {
                let fixed = apply_fixes(doc.bad_example, &errors);
                let expected = doc.good_example.trim_end();
                let actual = fixed.trim_end();
                assert_eq!(
                    actual, expected,
                    "missing-semicolon: applying fixes to bad_example should produce good_example\n\
                     Bad example:\n{}\n\
                     Fixed:\n{}\n\
                     Expected:\n{}",
                    doc.bad_example, fixed, doc.good_example
                );
            }
        }
    }
}
