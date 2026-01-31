pub mod linter;
pub mod parser;
pub mod reporter;
pub mod rules;

pub use linter::{LintError, Linter, Severity};
pub use parser::parse_config;
pub use reporter::{OutputFormat, Reporter};

use std::path::Path;

/// Run pre-parse checks that can detect errors before parsing
/// These checks work on the raw file content and don't require a valid AST
pub fn pre_parse_checks(path: &Path) -> Vec<LintError> {
    use rules::{MissingSemicolon, UnmatchedBraces};
    use linter::LintRule;
    use nginx_config::ast::Main;

    // Create a dummy config for the check (the rule reads from file directly)
    let dummy_config = Main { directives: vec![] };

    let mut errors = Vec::new();

    // Check for unmatched braces
    let brace_rule = UnmatchedBraces;
    errors.extend(brace_rule.check(&dummy_config, path));

    // Check for missing semicolons
    let semicolon_rule = MissingSemicolon;
    errors.extend(semicolon_rule.check(&dummy_config, path));

    errors
}
