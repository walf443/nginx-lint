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

/// Compute the byte offset of the start of each line (1-indexed).
///
/// Returns a vector where `line_starts[0]` is always `0` (start of line 1),
/// `line_starts[1]` is the byte offset of line 2, etc.
/// An extra entry at the end equals `content.len()` for convenience.
#[cfg(any(feature = "cli", feature = "wasm"))]
fn compute_line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts.push(content.len());
    starts
}

/// Convert a line-based [`Fix`] into an offset-based one using precomputed line starts.
///
/// Returns `None` if the fix references an out-of-range line or the old_text is not found.
#[cfg(any(feature = "cli", feature = "wasm"))]
fn normalize_line_fix(fix: &Fix, content: &str, line_starts: &[usize]) -> Option<Fix> {
    if fix.line == 0 {
        return None;
    }

    let num_lines = line_starts.len() - 1; // last entry is content.len()

    if fix.delete_line {
        if fix.line > num_lines {
            return None;
        }
        let start = line_starts[fix.line - 1];
        let end = if fix.line < num_lines {
            line_starts[fix.line] // includes the trailing \n
        } else {
            // Last line: also remove the preceding \n if there is one
            let end = line_starts[fix.line]; // == content.len()
            if start > 0 && content.as_bytes().get(start - 1) == Some(&b'\n') {
                return Some(Fix::replace_range(start - 1, end, ""));
            }
            end
        };
        return Some(Fix::replace_range(start, end, ""));
    }

    if fix.insert_after {
        if fix.line > num_lines {
            return None;
        }
        // Insert point: right after the \n at end of the target line
        let insert_offset = if fix.line < num_lines {
            line_starts[fix.line]
        } else {
            content.len()
        };
        let new_text = if insert_offset == content.len() && !content.ends_with('\n') {
            format!("\n{}", fix.new_text)
        } else {
            format!("{}\n", fix.new_text)
        };
        return Some(Fix::replace_range(insert_offset, insert_offset, &new_text));
    }

    if fix.line > num_lines {
        return None;
    }

    let line_start = line_starts[fix.line - 1];
    let line_end_with_newline = line_starts[fix.line];
    // Line content without trailing newline
    let line_end = if line_end_with_newline > line_start
        && content.as_bytes().get(line_end_with_newline - 1) == Some(&b'\n')
    {
        line_end_with_newline - 1
    } else {
        line_end_with_newline
    };

    if let Some(ref old_text) = fix.old_text {
        // Replace first occurrence of old_text within the line
        let line_content = &content[line_start..line_end];
        if let Some(pos) = line_content.find(old_text.as_str()) {
            let start = line_start + pos;
            let end = start + old_text.len();
            return Some(Fix::replace_range(start, end, &fix.new_text));
        }
        return None;
    }

    // Replace entire line content (not including newline)
    Some(Fix::replace_range(line_start, line_end, &fix.new_text))
}

/// Apply fixes to content string
/// Returns (modified content, number of fixes applied)
#[cfg(any(feature = "cli", feature = "wasm"))]
pub fn apply_fixes_to_content(content: &str, fixes: &[&Fix]) -> (String, usize) {
    let line_starts = compute_line_starts(content);

    // Normalize all fixes to range-based
    let mut range_fixes: Vec<Fix> = Vec::with_capacity(fixes.len());
    for fix in fixes {
        if fix.is_range_based() {
            range_fixes.push((*fix).clone());
        } else if let Some(normalized) = normalize_line_fix(fix, content, &line_starts) {
            range_fixes.push(normalized);
        }
    }

    // Sort by start_offset descending to avoid index shifts.
    // For same-offset insertions (start == end), sort by indent ascending so that
    // the more-indented text is processed last and ends up first in the file.
    range_fixes.sort_by(|a, b| {
        let a_start = a.start_offset.unwrap();
        let b_start = b.start_offset.unwrap();
        match b_start.cmp(&a_start) {
            std::cmp::Ordering::Equal => {
                let a_is_insert = a.end_offset.unwrap() == a_start;
                let b_is_insert = b.end_offset.unwrap() == b_start;
                if a_is_insert && b_is_insert {
                    // For insertions at the same point: ascending indent order
                    // so more-indented text is processed last (appears first in output)
                    let a_indent = a.new_text.len() - a.new_text.trim_start().len();
                    let b_indent = b.new_text.len() - b.new_text.trim_start().len();
                    a_indent.cmp(&b_indent)
                } else {
                    std::cmp::Ordering::Equal
                }
            }
            other => other,
        }
    });

    let mut fix_count = 0;
    let mut result = content.to_string();
    let mut applied_ranges: Vec<(usize, usize)> = Vec::new();

    for fix in &range_fixes {
        let start = fix.start_offset.unwrap();
        let end = fix.end_offset.unwrap();

        // Check if this range overlaps with any already applied range
        let overlaps = applied_ranges.iter().any(|(s, e)| start < *e && end > *s);
        if overlaps {
            continue;
        }

        if start <= result.len() && end <= result.len() && start <= end {
            result.replace_range(start..end, &fix.new_text);
            applied_ranges.push((start, start + fix.new_text.len()));
            fix_count += 1;
        }
    }

    // Ensure trailing newline
    if !result.ends_with('\n') {
        result.push('\n');
    }

    (result, fix_count)
}
