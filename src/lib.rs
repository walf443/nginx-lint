pub mod linter;
pub mod parser;
pub mod reporter;
pub mod rules;

pub use linter::{LintError, Linter, Severity};
pub use parser::parse_config;
pub use reporter::{OutputFormat, Reporter};
