//! WIT component model guest bindings
//!
//! This module provides the bridge between the existing Plugin trait
//! and the WIT-generated Guest trait for component model plugins.

// Generate guest-side bindings from the WIT file
wit_bindgen::generate!({
    path: "../../wit/nginx-lint-plugin.wit",
    world: "plugin",
});

/// Convert SDK PluginSpec to WIT PluginSpec
pub fn convert_spec(sdk_spec: super::PluginSpec) -> nginx_lint::plugin::types::PluginSpec {
    nginx_lint::plugin::types::PluginSpec {
        name: sdk_spec.name,
        category: sdk_spec.category,
        description: sdk_spec.description,
        api_version: sdk_spec.api_version,
        severity: sdk_spec.severity,
        why: sdk_spec.why,
        bad_example: sdk_spec.bad_example,
        good_example: sdk_spec.good_example,
        references: sdk_spec.references,
    }
}

/// Convert SDK Severity to WIT Severity
pub fn convert_severity(severity: super::Severity) -> nginx_lint::plugin::types::Severity {
    match severity {
        super::Severity::Error => nginx_lint::plugin::types::Severity::Error,
        super::Severity::Warning => nginx_lint::plugin::types::Severity::Warning,
    }
}

/// Convert SDK Fix to WIT Fix
pub fn convert_fix(fix: super::Fix) -> nginx_lint::plugin::types::Fix {
    nginx_lint::plugin::types::Fix {
        line: fix.line as u32,
        old_text: fix.old_text,
        new_text: fix.new_text,
        delete_line: fix.delete_line,
        insert_after: fix.insert_after,
        start_offset: fix.start_offset.map(|v| v as u32),
        end_offset: fix.end_offset.map(|v| v as u32),
    }
}

/// Convert SDK LintError to WIT LintError
pub fn convert_lint_error(error: super::LintError) -> nginx_lint::plugin::types::LintError {
    nginx_lint::plugin::types::LintError {
        rule: error.rule,
        category: error.category,
        message: error.message,
        severity: convert_severity(error.severity),
        line: error.line.map(|v| v as u32),
        column: error.column.map(|v| v as u32),
        fixes: error.fixes.into_iter().map(convert_fix).collect(),
    }
}
