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

/// A single lint error for JavaScript
#[derive(serde::Serialize)]
struct JsLintError {
    rule: String,
    category: String,
    message: String,
    severity: String,
    line: Option<usize>,
    column: Option<usize>,
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

/// Lint options passed from JavaScript
#[derive(serde::Deserialize, Default)]
struct LintOptions {
    #[serde(default)]
    indent_size: Option<usize>,
}

/// Lint an nginx configuration string with custom settings
///
/// # Arguments
/// * `content` - The nginx configuration content to lint
/// * `options_json` - JSON string with options (e.g., `{"indent_size": 4}`)
///
/// # Returns
/// A `WasmLintResult` containing the lint errors as JSON
#[wasm_bindgen]
pub fn lint_with_config(content: &str, options_json: &str) -> Result<WasmLintResult, JsValue> {
    use crate::config::{LintConfig, RuleConfig};
    use crate::rules::InconsistentIndentation;
    use std::collections::HashMap;

    // Parse options
    let options: LintOptions = if options_json.is_empty() {
        LintOptions::default()
    } else {
        serde_json::from_str(options_json).map_err(|e| JsValue::from_str(&e.to_string()))?
    };

    // Parse the configuration
    let config = parse_string(content).map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Build LintConfig from options
    let lint_config = if options.indent_size.is_some() {
        let mut rules = HashMap::new();
        if let Some(indent_size) = options.indent_size {
            rules.insert(
                "inconsistent-indentation".to_string(),
                RuleConfig {
                    enabled: true,
                    indent_size: Some(indent_size),
                    ..Default::default()
                },
            );
        }
        Some(LintConfig {
            rules,
            ..Default::default()
        })
    } else {
        None
    };

    // Create linter with config
    let linter = Linter::with_config(lint_config.as_ref());

    // Lint the configuration (use a dummy path since we're linting a string)
    // Note: Some rules that read from files won't work, so we handle them separately
    let mut errors = linter.lint(&config, std::path::Path::new("nginx.conf"));

    // Run indentation check directly on content (since file-based check won't work in WASM)
    let indent_size = options.indent_size.unwrap_or(2);
    let indent_rule = InconsistentIndentation { indent_size };
    let indent_errors = indent_rule.check_content(content);
    errors.extend(indent_errors);

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
