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
    parse_string,
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

/// Apply fixes to content string
/// Returns (modified content, number of fixes applied)
#[cfg(feature = "cli")]
pub fn apply_fixes_to_content(content: &str, fixes: &[&Fix]) -> (String, usize) {
    // Separate range-based and line-based fixes
    let (range_fixes, line_fixes): (Vec<&&Fix>, Vec<&&Fix>) = fixes
        .iter()
        .partition(|f| f.start_offset.is_some() && f.end_offset.is_some());

    let mut fix_count = 0;
    let mut result = content.to_string();

    // Apply range-based fixes first (sort by start_offset descending to avoid shifts)
    if !range_fixes.is_empty() {
        let mut sorted_range_fixes = range_fixes;
        sorted_range_fixes.sort_by(|a, b| b.start_offset.unwrap().cmp(&a.start_offset.unwrap()));

        // Check for overlapping ranges and skip overlapping fixes
        let mut applied_ranges: Vec<(usize, usize)> = Vec::new();

        for fix in sorted_range_fixes {
            let start = fix.start_offset.unwrap();
            let end = fix.end_offset.unwrap();

            // Check if this range overlaps with any already applied range
            let overlaps = applied_ranges.iter().any(|(s, e)| {
                // Ranges overlap if one starts before the other ends
                start < *e && end > *s
            });

            if overlaps {
                continue; // Skip overlapping fix
            }

            if start <= result.len() && end <= result.len() && start <= end {
                result.replace_range(start..end, &fix.new_text);
                applied_ranges.push((start, start + fix.new_text.len()));
                fix_count += 1;
            }
        }
    }

    // Apply line-based fixes
    if !line_fixes.is_empty() {
        let mut lines: Vec<String> = result.lines().map(|s| s.to_string()).collect();

        // Sort by line number descending, with special handling for insert_after
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
            if fix.line == 0 {
                continue;
            }

            if fix.insert_after {
                let insert_idx = fix.line.min(lines.len());
                lines.insert(insert_idx, fix.new_text.clone());
                fix_count += 1;
                continue;
            }

            if fix.line > lines.len() {
                continue;
            }

            let line_idx = fix.line - 1;

            if fix.delete_line {
                lines.remove(line_idx);
                fix_count += 1;
            } else if let Some(ref old_text) = fix.old_text {
                if lines[line_idx].contains(old_text.as_str()) {
                    lines[line_idx] = lines[line_idx].replace(old_text.as_str(), &fix.new_text);
                    fix_count += 1;
                }
            } else {
                lines[line_idx] = fix.new_text.clone();
                fix_count += 1;
            }
        }

        result = lines.join("\n");
    }

    // Ensure trailing newline
    if !result.ends_with('\n') {
        result.push('\n');
    }

    (result, fix_count)
}
