// Re-export from nginx-lint-common
pub use nginx_lint_common::config;
pub use nginx_lint_common::ignore;
pub use nginx_lint_common::parser;

// Local modules with CLI-specific functionality
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
pub use nginx_lint_common::{apply_fixes_to_content, compute_line_starts, normalize_line_fix};

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

/// Run pre-parse checks that can detect errors before parsing
/// These checks work on the raw file content and don't require a valid AST
#[cfg(feature = "cli")]
pub fn pre_parse_checks(path: &Path) -> Vec<LintError> {
    pre_parse_checks_with_config(path, None)
}

/// Run pre-parse checks with optional LintConfig
#[cfg(feature = "cli")]
pub fn pre_parse_checks_with_config(
    path: &Path,
    lint_config: Option<&LintConfig>,
) -> Vec<LintError> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    pre_parse_checks_from_content(&content, lint_config)
}

/// Run pre-parse checks on string content with optional LintConfig
#[cfg(feature = "cli")]
pub fn pre_parse_checks_from_content(
    content: &str,
    lint_config: Option<&LintConfig>,
) -> Vec<LintError> {
    use nginx_lint_common::ignore::{IgnoreTracker, filter_errors, warnings_to_errors};
    use rules::{MissingSemicolon, UnclosedQuote, UnmatchedBraces};

    // Build ignore tracker from content without rule name validation
    // (rule name validation is done later in lint_with_content when all plugins are loaded)
    let (mut tracker, warnings) = IgnoreTracker::from_content(content);

    let additional_block_directives: Vec<String> = lint_config
        .map(|c| c.additional_block_directives().to_vec())
        .unwrap_or_default();

    let mut errors = Vec::new();

    // Check for unmatched braces
    let brace_rule = UnmatchedBraces;
    errors.extend(brace_rule.check_content_with_extras(content, &additional_block_directives));

    // Check for unclosed quotes
    let quote_rule = UnclosedQuote;
    errors.extend(quote_rule.check_content(content));

    // Check for missing semicolons
    let semicolon_rule = MissingSemicolon;
    errors.extend(semicolon_rule.check_content_with_extras(content, &additional_block_directives));

    // Filter ignored errors
    let result = filter_errors(errors, &mut tracker);
    let mut errors = result.errors;

    // Add warnings from ignore comments (parse warnings + unused warnings)
    errors.extend(warnings_to_errors(warnings));
    errors.extend(warnings_to_errors(result.unused_warnings));

    errors
}

/// Apply fixes to a file
/// Returns the number of fixes applied
#[cfg(feature = "cli")]
pub fn apply_fixes(path: &Path, errors: &[LintError]) -> std::io::Result<usize> {
    let content = fs::read_to_string(path)?;
    let fixes: Vec<_> = errors.iter().flat_map(|e| e.fixes.iter()).collect();

    let (result, fix_count) = apply_fixes_to_content(&content, &fixes);

    if fix_count > 0 {
        fs::write(path, result)?;
    }

    Ok(fix_count)
}
