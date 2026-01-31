use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::collections::HashMap;
use std::path::Path;

/// Check for duplicate directives that should only appear once
pub struct DuplicateDirective;

impl LintRule for DuplicateDirective {
    fn name(&self) -> &'static str {
        "duplicate-directive"
    }

    fn category(&self) -> &'static str {
        "syntax"
    }

    fn description(&self) -> &'static str {
        "Detects duplicate directives that should only appear once in a context"
    }

    fn check(&self, config: &Config, _path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        // Directives that should only appear once in main context
        let unique_directives = ["worker_processes", "pid", "error_log"];

        // Check main context
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for directive in config.directives() {
            let name = directive.name.as_str();
            if unique_directives.contains(&name) {
                let count = seen.entry(name).or_insert(0);
                *count += 1;
                if *count > 1 {
                    let message = format!("Duplicate directive '{}' in main context", name);
                    errors.push(
                        LintError::new(self.name(), self.category(), &message, Severity::Warning)
                            .with_location(directive.span.start.line, directive.span.start.column),
                    );
                }
            }
        }

        errors
    }
}
