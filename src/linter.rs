use crate::config::LintConfig;
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

/// Represents a fix that can be applied to resolve a lint error
#[derive(Debug, Clone, Serialize)]
pub struct Fix {
    /// Line number where the fix should be applied (1-indexed)
    pub line: usize,
    /// The original text to replace (if None and new_text is empty, delete the line)
    pub old_text: Option<String>,
    /// The new text to insert (empty string with old_text=None means delete)
    pub new_text: String,
    /// Whether to delete the entire line
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub delete_line: bool,
    /// Whether to insert new_text as a new line after the specified line
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub insert_after: bool,
}

impl Fix {
    /// Create a fix that replaces text on a specific line
    pub fn replace(line: usize, old_text: &str, new_text: &str) -> Self {
        Self {
            line,
            old_text: Some(old_text.to_string()),
            new_text: new_text.to_string(),
            delete_line: false,
            insert_after: false,
        }
    }

    /// Create a fix that replaces an entire line
    pub fn replace_line(line: usize, new_text: &str) -> Self {
        Self {
            line,
            old_text: None,
            new_text: new_text.to_string(),
            delete_line: false,
            insert_after: false,
        }
    }

    /// Create a fix that deletes an entire line
    pub fn delete(line: usize) -> Self {
        Self {
            line,
            old_text: None,
            new_text: String::new(),
            delete_line: true,
            insert_after: false,
        }
    }

    /// Create a fix that inserts a new line after the specified line
    pub fn insert_after(line: usize, new_text: &str) -> Self {
        Self {
            line,
            old_text: None,
            new_text: new_text.to_string(),
            delete_line: false,
            insert_after: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LintError {
    pub rule: String,
    pub category: String,
    pub message: String,
    pub severity: Severity,
    pub line: Option<usize>,
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

impl LintError {
    pub fn new(rule: &str, category: &str, message: &str, severity: Severity) -> Self {
        Self {
            rule: rule.to_string(),
            category: category.to_string(),
            message: message.to_string(),
            severity,
            line: None,
            column: None,
            fix: None,
        }
    }

    pub fn with_location(mut self, line: usize, column: usize) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }
}

pub trait LintRule: Send + Sync {
    fn name(&self) -> &'static str;
    fn category(&self) -> &'static str;
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
        Self::with_config(None)
    }

    pub fn with_config(config: Option<&LintConfig>) -> Self {
        use crate::rules::{
            AutoindexEnabled, DeprecatedSslProtocol, DuplicateDirective, GzipNotEnabled,
            InconsistentIndentation, MissingErrorLog, MissingSemicolon, ServerTokensEnabled,
            UnclosedQuote, UnmatchedBraces, WeakSslCiphers,
        };

        let mut linter = Self::new();

        let is_enabled = |name: &str| config.is_none_or(|c| c.is_rule_enabled(name));

        // Syntax rules
        if is_enabled("duplicate-directive") {
            linter.add_rule(Box::new(DuplicateDirective));
        }
        if is_enabled("unmatched-braces") {
            linter.add_rule(Box::new(UnmatchedBraces));
        }
        if is_enabled("unclosed-quote") {
            linter.add_rule(Box::new(UnclosedQuote));
        }
        if is_enabled("missing-semicolon") {
            linter.add_rule(Box::new(MissingSemicolon));
        }

        // Security rules
        if is_enabled("deprecated-ssl-protocol") {
            let mut rule = DeprecatedSslProtocol::default();
            if let Some(allowed) = config
                .and_then(|c| c.get_rule_config("deprecated-ssl-protocol"))
                .and_then(|cfg| cfg.allowed_protocols.clone())
            {
                rule.allowed_protocols = allowed;
            }
            linter.add_rule(Box::new(rule));
        }
        if is_enabled("server-tokens-enabled") {
            linter.add_rule(Box::new(ServerTokensEnabled));
        }
        if is_enabled("autoindex-enabled") {
            linter.add_rule(Box::new(AutoindexEnabled));
        }
        if is_enabled("weak-ssl-ciphers") {
            let mut rule = WeakSslCiphers::default();
            if let Some(cfg) = config.and_then(|c| c.get_rule_config("weak-ssl-ciphers")) {
                if let Some(weak_ciphers) = cfg.weak_ciphers.clone() {
                    rule.weak_ciphers = weak_ciphers;
                }
                if let Some(required_exclusions) = cfg.required_exclusions.clone() {
                    rule.required_exclusions = required_exclusions;
                }
            }
            linter.add_rule(Box::new(rule));
        }

        // Style rules
        if is_enabled("inconsistent-indentation") {
            let mut rule = InconsistentIndentation::default();
            if let Some(indent_size) = config
                .and_then(|c| c.get_rule_config("inconsistent-indentation"))
                .and_then(|cfg| cfg.indent_size)
            {
                rule.indent_size = indent_size;
            }
            linter.add_rule(Box::new(rule));
        }

        // Best practices
        if is_enabled("gzip-not-enabled") {
            linter.add_rule(Box::new(GzipNotEnabled));
        }
        if is_enabled("missing-error-log") {
            linter.add_rule(Box::new(MissingErrorLog));
        }

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
