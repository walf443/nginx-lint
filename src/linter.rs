use nginx_config::ast::Main;
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
    fn check(&self, config: &Main, path: &Path) -> Vec<LintError>;
}

pub struct Linter {
    rules: Vec<Box<dyn LintRule>>,
}

impl Linter {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn with_default_rules() -> Self {
        use crate::rules::{best_practices, security, syntax};

        let mut linter = Self::new();

        // Syntax rules
        linter.add_rule(Box::new(syntax::DuplicateDirective));

        // Security rules
        linter.add_rule(Box::new(security::DeprecatedSslProtocol));
        linter.add_rule(Box::new(security::ServerTokensEnabled));
        linter.add_rule(Box::new(security::AutoindexEnabled));

        // Best practices
        linter.add_rule(Box::new(best_practices::GzipNotEnabled));
        linter.add_rule(Box::new(best_practices::MissingErrorLog));

        linter
    }

    pub fn add_rule(&mut self, rule: Box<dyn LintRule>) {
        self.rules.push(rule);
    }

    pub fn lint(&self, config: &Main, path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();
        for rule in &self.rules {
            errors.extend(rule.check(config, path));
        }
        errors
    }
}

impl Default for Linter {
    fn default() -> Self {
        Self::with_default_rules()
    }
}
