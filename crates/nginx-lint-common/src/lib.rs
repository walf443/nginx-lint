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

// Re-export parser crate
pub use nginx_lint_parser as parser;

// Re-export commonly used types
pub use config::{Color, ColorConfig, ColorMode, LintConfig, ValidationError};
pub use docs::{RuleDoc, RuleDocOwned};
pub use ignore::{
    FilterResult, IgnoreTracker, IgnoreWarning, filter_errors, parse_context_comment,
};
pub use linter::{Fix, LintError, LintRule, Linter, RULE_CATEGORIES, Severity};
pub use nginx_lint_parser::{parse_config, parse_string};
