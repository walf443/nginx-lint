// Re-export from nginx-lint-common
pub use nginx_lint_common::config;
pub use nginx_lint_common::ignore;
pub use nginx_lint_common::parser;

// Local modules with CLI-specific functionality
pub mod cache;
pub mod docs;
pub mod linter;
pub mod rules;

// CLI-only modules (require filesystem access)
#[cfg(feature = "cli")]
pub mod include;
#[cfg(feature = "cli")]
pub mod reporter;

// WASM module
#[cfg(feature = "wasm")]
pub mod wasm;

// Plugin system (for loading custom WASM lint rules or native plugin adapters)
#[cfg(any(feature = "plugins", feature = "native-builtin-plugins"))]
pub mod plugin;

// Re-export commonly used types from nginx-lint-common
pub use nginx_lint_common::{
    Color, ColorConfig, ColorMode, FilterResult, IgnoreTracker, IgnoreWarning, IncludeConfig,
    LintConfig, PathMapping, ValidationError, filter_errors, parse_config, parse_context_comment,
    parse_string, parse_string_with_errors,
};

// Re-export from local modules
pub use docs::{RuleDoc, RuleDocOwned};
#[cfg(feature = "cli")]
pub use linter::RuleProfile;
pub use linter::{Fix, LintError, LintRule, Linter, Severity};
pub use nginx_lint_common::RULE_CATEGORIES;
pub use nginx_lint_common::{
    FixApplyResult, apply_fixes_to_content, apply_fixes_to_content_detailed, compute_line_starts,
    normalize_line_fix,
};

#[cfg(feature = "cli")]
pub use include::{IncludedFile, collect_included_files, collect_included_files_with_context};
#[cfg(feature = "cli")]
pub use reporter::{OutputFormat, Reporter};

#[cfg(feature = "cli")]
use std::fs;
#[cfg(feature = "cli")]
use std::path::Path;

/// Convert parser `SyntaxError`s into `LintError`s.
///
/// Each syntax error is reported as severity `Error` with rule name `"syntax-error"`.
pub fn syntax_errors_to_lint_errors(
    syntax_errors: &[nginx_lint_common::parser::parser::SyntaxError],
    source: &str,
) -> Vec<LintError> {
    let line_index = nginx_lint_common::parser::line_index::LineIndex::new(source);
    syntax_errors
        .iter()
        .map(|e| {
            let pos = line_index.position(e.offset);
            LintError {
                rule: "syntax-error".to_string(),
                category: "syntax".to_string(),
                message: e.message.clone(),
                severity: Severity::Error,
                line: Some(pos.line),
                column: Some(pos.column),
                fixes: Vec::new(),
            }
        })
        .collect()
}

/// Apply fixes to a file
/// Returns the application result, including applied and skipped fix counts
#[cfg(feature = "cli")]
pub fn apply_fixes(path: &Path, errors: &[LintError]) -> std::io::Result<FixApplyResult> {
    let content = fs::read_to_string(path)?;
    let fixes: Vec<_> = errors.iter().flat_map(|e| e.fixes.iter()).collect();

    let result = apply_fixes_to_content_detailed(&content, &fixes);

    if result.applied > 0 {
        fs::write(path, &result.content)?;
    }

    Ok(result)
}
