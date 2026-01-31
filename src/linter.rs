use crate::parser::ast::Config;
use rayon::prelude::*;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "ERROR"),
            Severity::Warning => write!(f, "WARNING"),
            Severity::Info => write!(f, "INFO"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LintError {
    pub rule: String,
    pub message: String,
    pub severity: Severity,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl LintError {
    pub fn new(rule: &str, message: &str, severity: Severity) -> Self {
        Self {
            rule: rule.to_string(),
            message: message.to_string(),
            severity,
            line: None,
            column: None,
        }
    }

    pub fn with_location(mut self, line: usize, column: usize) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }
}

pub trait LintRule: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn check(&self, config: &Config, path: &Path) -> Vec<LintError>;
}

pub struct Linter {
    rules: Vec<Box<dyn LintRule>>,
}

impl Linter {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn with_default_rules() -> Self {
        use crate::rules::{
            AutoindexEnabled, DeprecatedSslProtocol, DuplicateDirective, GzipNotEnabled,
            InconsistentIndentation, MissingErrorLog, MissingSemicolon, ServerTokensEnabled,
            UnclosedQuote, UnmatchedBraces,
        };

        let mut linter = Self::new();

        // Syntax rules
        linter.add_rule(Box::new(DuplicateDirective));
        linter.add_rule(Box::new(UnmatchedBraces));
        linter.add_rule(Box::new(UnclosedQuote));
        linter.add_rule(Box::new(MissingSemicolon));

        // Security rules
        linter.add_rule(Box::new(DeprecatedSslProtocol));
        linter.add_rule(Box::new(ServerTokensEnabled));
        linter.add_rule(Box::new(AutoindexEnabled));

        // Style rules
        linter.add_rule(Box::new(InconsistentIndentation::default()));

        // Best practices
        linter.add_rule(Box::new(GzipNotEnabled));
        linter.add_rule(Box::new(MissingErrorLog));

        linter
    }

    pub fn add_rule(&mut self, rule: Box<dyn LintRule>) {
        self.rules.push(rule);
    }

    /// Run all lint rules in parallel and collect errors
    ///
    /// Results are collected in rule order (deterministic output)
    pub fn lint(&self, config: &Config, path: &Path) -> Vec<LintError> {
        self.rules
            .par_iter()
            .map(|rule| rule.check(config, path))
            .collect::<Vec<_>>()
            .into_iter()
            .flatten()
            .collect()
    }
}

impl Default for Linter {
    fn default() -> Self {
        Self::with_default_rules()
    }
}
