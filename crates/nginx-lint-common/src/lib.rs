//! Core types shared between the nginx-lint CLI and WASM plugins.
//!
//! This crate provides the foundational types used throughout the nginx-lint
//! ecosystem: lint rule definitions, error reporting, configuration management,
//! and ignore comment support.
//!
//! # Modules
//!
//! - [`linter`] — Core lint types: [`LintRule`] trait, [`LintError`], [`Severity`], [`Fix`]
//! - [`config`] — Configuration loaded from `.nginx-lint.toml` ([`LintConfig`], [`ValidationError`])
//! - [`ignore`] — `# nginx-lint-ignore` comment parsing and error filtering
//! - [`docs`] — Rule documentation extraction ([`RuleDoc`])
//!
//! # Quick reference
//!
//! | Type | Purpose |
//! |------|---------|
//! | [`LintRule`] | Trait that every lint rule (native or WASM) implements |
//! | [`LintError`] | A single lint diagnostic with location, severity, and optional fixes |
//! | [`Severity`] | `Error` or `Warning` |
//! | [`Fix`] | An auto-fix action (replace, delete, insert) |
//! | [`LintConfig`] | Settings loaded from `.nginx-lint.toml` |
//! | [`Linter`] | Container that holds rules and runs them against a parsed config |
//!
//! # Re-exports
//!
//! The [`parser`] module re-exports the entire [`nginx_lint_parser`] crate,
//! giving access to [`parse_config`], [`parse_string`], and the AST types.

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
