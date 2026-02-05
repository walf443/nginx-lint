//! Common types and parser for nginx-lint
//!
//! This crate provides the core functionality shared between nginx-lint-cli
//! and nginx-lint-plugin:
//! - nginx configuration parser
//! - AST types
//! - LintRule trait and LintError types
//! - Configuration management
//! - Ignore comment support

pub mod config;
pub mod docs;
pub mod ignore;
pub mod linter;
pub mod parser;

// Re-export commonly used types
pub use config::{Color, ColorConfig, ColorMode, LintConfig, ValidationError};
pub use docs::{RuleDoc, RuleDocOwned};
pub use ignore::{filter_errors, parse_context_comment, FilterResult, IgnoreTracker, IgnoreWarning};
pub use linter::{Fix, LintError, LintRule, Linter, Severity};
pub use parser::{parse_config, parse_string};
