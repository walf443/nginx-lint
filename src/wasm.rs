//! WebAssembly bindings for nginx-lint
//!
//! This module provides JavaScript-callable functions for linting nginx configurations
//! in the browser.

use wasm_bindgen::prelude::*;

use crate::linter::{LintError, Linter, Severity};
use crate::parser::parse_string;

/// Initialize the WASM module (sets up panic hook for better error messages)
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// Lint result returned to JavaScript
#[wasm_bindgen]
#[derive(Clone)]
pub struct WasmLintResult {
    errors_json: String,
    error_count: usize,
    warning_count: usize,
    info_count: usize,
}

#[wasm_bindgen]
impl WasmLintResult {
    /// Get the errors as JSON string
    #[wasm_bindgen(getter)]
    pub fn errors(&self) -> String {
        self.errors_json.clone()
    }

    /// Get the number of errors
    #[wasm_bindgen(getter)]
    pub fn error_count(&self) -> usize {
        self.error_count
    }

    /// Get the number of warnings
    #[wasm_bindgen(getter)]
    pub fn warning_count(&self) -> usize {
        self.warning_count
    }

    /// Get the number of info messages
    #[wasm_bindgen(getter)]
    pub fn info_count(&self) -> usize {
        self.info_count
    }

    /// Check if there are any issues
    #[wasm_bindgen]
    pub fn has_issues(&self) -> bool {
        self.error_count > 0 || self.warning_count > 0 || self.info_count > 0
    }
}

/// A fix for JavaScript
#[derive(serde::Serialize)]
struct JsFix {
    line: usize,
    old_text: Option<String>,
    new_text: String,
    delete_line: bool,
    insert_after: bool,
}

/// A single lint error for JavaScript
#[derive(serde::Serialize)]
struct JsLintError {
    rule: String,
    category: String,
    message: String,
    severity: String,
    line: Option<usize>,
    column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<JsFix>,
}

impl From<&LintError> for JsLintError {
    fn from(error: &LintError) -> Self {
        JsLintError {
            rule: error.rule.clone(),
            category: error.category.clone(),
            message: error.message.clone(),
            severity: match error.severity {
                Severity::Error => "error".to_string(),
                Severity::Warning => "warning".to_string(),
                Severity::Info => "info".to_string(),
            },
            line: error.line,
            column: error.column,
            fix: error.fix.as_ref().map(|f| JsFix {
                line: f.line,
                old_text: f.old_text.clone(),
                new_text: f.new_text.clone(),
                delete_line: f.delete_line,
                insert_after: f.insert_after,
            }),
        }
    }
}

/// Lint an nginx configuration string with default settings
///
/// # Arguments
/// * `content` - The nginx configuration content to lint
///
/// # Returns
/// A `WasmLintResult` containing the lint errors as JSON
#[wasm_bindgen]
pub fn lint(content: &str) -> Result<WasmLintResult, JsValue> {
    lint_with_config(content, "")
}

/// Lint an nginx configuration string with custom settings
///
/// # Arguments
/// * `content` - The nginx configuration content to lint
/// * `config_toml` - TOML string with .nginx-lint.toml configuration (can be empty)
///
/// # Returns
/// A `WasmLintResult` containing the lint errors as JSON
#[wasm_bindgen]
pub fn lint_with_config(content: &str, config_toml: &str) -> Result<WasmLintResult, JsValue> {
    use crate::config::LintConfig;
    use crate::rules::{
        InconsistentIndentation, MissingSemicolon, UnclosedQuote, UnmatchedBraces,
    };

    // Parse TOML configuration
    let lint_config = if config_toml.is_empty() {
        None
    } else {
        Some(LintConfig::from_str(config_toml).map_err(|e| JsValue::from_str(&e))?)
    };

    // Helper to check if a rule is enabled
    let is_enabled = |rule_name: &str| {
        lint_config
            .as_ref()
            .map(|c| c.is_rule_enabled(rule_name))
            .unwrap_or(true)
    };

    // Run pre-parse checks first (syntax checks that don't require a valid AST)
    let mut pre_parse_errors = Vec::new();

    if is_enabled("unmatched-braces") {
        let rule = UnmatchedBraces;
        pre_parse_errors.extend(rule.check_content(content));
    }

    if is_enabled("unclosed-quote") {
        let rule = UnclosedQuote;
        pre_parse_errors.extend(rule.check_content(content));
    }

    if is_enabled("missing-semicolon") {
        let rule = MissingSemicolon;
        pre_parse_errors.extend(rule.check_content(content));
    }

    // If there are pre-parse errors with Severity::Error, return them
    // (don't try to parse, as it will likely fail)
    let has_syntax_errors = pre_parse_errors.iter().any(|e| e.severity == Severity::Error);

    let mut errors = pre_parse_errors;

    // Only parse and run AST-based rules if there are no syntax errors
    if !has_syntax_errors {
        // Parse the nginx configuration
        if let Ok(config) = parse_string(content) {
            // Create linter with config
            let linter = Linter::with_config(lint_config.as_ref());

            // Lint the configuration (use a dummy path since we're linting a string)
            let lint_errors = linter.lint(&config, std::path::Path::new("nginx.conf"));
            errors.extend(lint_errors);
        }
        // If parsing fails, we already have pre-parse errors or no errors
    }

    // Run indentation check directly on content (always, as it doesn't need AST)
    if is_enabled("inconsistent-indentation") {
        let indent_size = lint_config
            .as_ref()
            .and_then(|c| c.get_rule_config("inconsistent-indentation"))
            .and_then(|r| r.indent_size)
            .unwrap_or(2);
        let indent_rule = InconsistentIndentation { indent_size };
        errors.extend(indent_rule.check_content(content));
    }

    // Convert errors to JSON
    let js_errors: Vec<JsLintError> = errors.iter().map(JsLintError::from).collect();
    let errors_json =
        serde_json::to_string(&js_errors).map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Count by severity
    let error_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Error)
        .count();
    let warning_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Warning)
        .count();
    let info_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Info)
        .count();

    Ok(WasmLintResult {
        errors_json,
        error_count,
        warning_count,
        info_count,
    })
}

/// Get the version of nginx-lint
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Get all available rule names
#[wasm_bindgen]
pub fn get_rule_names() -> String {
    let linter = Linter::with_default_rules();
    let names: Vec<&str> = linter.rules().iter().map(|r| r.name()).collect();
    serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string())
}
