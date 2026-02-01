pub mod config;
pub mod include;
pub mod linter;
pub mod parser;
pub mod reporter;
pub mod rules;

pub use config::{Color, ColorConfig, ColorMode, LintConfig, ValidationError};
pub use include::{collect_included_files, IncludedFile};
pub use linter::{Fix, LintError, Linter, Severity};
pub use parser::{parse_config, parse_string};
pub use reporter::{OutputFormat, Reporter};

use std::fs;
use std::path::Path;

/// Run pre-parse checks that can detect errors before parsing
/// These checks work on the raw file content and don't require a valid AST
pub fn pre_parse_checks(path: &Path) -> Vec<LintError> {
    use linter::LintRule;
    use parser::ast::Config;
    use rules::{MissingSemicolon, UnclosedQuote, UnmatchedBraces};

    // Create a dummy config for the check (the rule reads from file directly)
    let dummy_config = Config::new();

    let mut errors = Vec::new();

    // Check for unmatched braces
    let brace_rule = UnmatchedBraces;
    errors.extend(brace_rule.check(&dummy_config, path));

    // Check for unclosed quotes
    let quote_rule = UnclosedQuote;
    errors.extend(quote_rule.check(&dummy_config, path));

    // Check for missing semicolons
    let semicolon_rule = MissingSemicolon;
    errors.extend(semicolon_rule.check(&dummy_config, path));

    errors
}

/// Apply fixes to a file
/// Returns the number of fixes applied
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
