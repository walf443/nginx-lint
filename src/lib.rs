pub mod config;
pub mod ignore;
pub mod linter;
pub mod parser;
pub mod rules;

// CLI-only modules (require filesystem access)
#[cfg(feature = "cli")]
pub mod include;
#[cfg(feature = "cli")]
pub mod reporter;

// WASM module
#[cfg(feature = "wasm")]
pub mod wasm;

pub use config::{Color, ColorConfig, ColorMode, LintConfig, ValidationError};
pub use ignore::{filter_errors, FilterResult, IgnoreTracker, IgnoreWarning};
pub use linter::{Fix, LintError, Linter, Severity};
pub use parser::{parse_config, parse_string};

#[cfg(feature = "cli")]
pub use include::{collect_included_files, IncludedFile};
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
pub fn pre_parse_checks_with_config(path: &Path, lint_config: Option<&LintConfig>) -> Vec<LintError> {
    use rules::{MissingSemicolon, UnclosedQuote, UnmatchedBraces};
    use ignore::{filter_errors, warnings_to_errors, IgnoreTracker};

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    // Build ignore tracker from content
    let (tracker, warnings) = IgnoreTracker::from_content(&content);

    let additional_block_directives: Vec<String> = lint_config
        .map(|c| c.additional_block_directives().to_vec())
        .unwrap_or_default();

    let mut errors = Vec::new();

    // Check for unmatched braces
    let brace_rule = UnmatchedBraces;
    errors.extend(brace_rule.check_content_with_extras(&content, &additional_block_directives));

    // Check for unclosed quotes
    let quote_rule = UnclosedQuote;
    errors.extend(quote_rule.check_content(&content));

    // Check for missing semicolons
    let semicolon_rule = MissingSemicolon;
    errors.extend(semicolon_rule.check_content_with_extras(&content, &additional_block_directives));

    // Filter ignored errors
    let result = filter_errors(errors, &tracker);
    let mut errors = result.errors;

    // Add warnings from ignore comments
    errors.extend(warnings_to_errors(warnings));

    errors
}

/// Apply fixes to a file
/// Returns the number of fixes applied
#[cfg(feature = "cli")]
pub fn apply_fixes(path: &Path, errors: &[LintError]) -> std::io::Result<usize> {
    let content = fs::read_to_string(path)?;
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

    // Collect fixes and sort by line number (descending to avoid offset issues)
    // For insert_after fixes at the same line, sort by indent ascending
    // (process outer blocks first so inner blocks end up on top)
    let mut fixes: Vec<_> = errors.iter().filter_map(|e| e.fix.as_ref()).collect();
    fixes.sort_by(|a, b| {
        match b.line.cmp(&a.line) {
            std::cmp::Ordering::Equal if a.insert_after && b.insert_after => {
                // For insert_after at same line, sort by indent ascending
                let a_indent = a.new_text.len() - a.new_text.trim_start().len();
                let b_indent = b.new_text.len() - b.new_text.trim_start().len();
                a_indent.cmp(&b_indent)
            }
            other => other,
        }
    });

    let mut fix_count = 0;

    for fix in fixes {
        if fix.line == 0 {
            continue;
        }

        if fix.insert_after {
            // Insert a new line after the specified line
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
            // Delete the entire line
            lines.remove(line_idx);
            fix_count += 1;
        } else if let Some(old_text) = &fix.old_text {
            // Replace specific text on the line
            if lines[line_idx].contains(old_text) {
                lines[line_idx] = lines[line_idx].replace(old_text, &fix.new_text);
                fix_count += 1;
            }
        } else {
            // Replace the entire line
            lines[line_idx] = fix.new_text.clone();
            fix_count += 1;
        }
    }

    if fix_count > 0 {
        fs::write(path, lines.join("\n") + "\n")?;
    }

    Ok(fix_count)
}
