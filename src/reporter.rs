use crate::LintError;
use crate::Severity;
use colored::Colorize;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

pub struct Reporter {
    format: OutputFormat,
}

impl Reporter {
    pub fn new(format: OutputFormat) -> Self {
        Self { format }
    }

    pub fn report(&self, errors: &[LintError], path: &Path) {
        match self.format {
            OutputFormat::Text => self.report_text(errors, path),
            OutputFormat::Json => self.report_json(errors, path),
        }
    }

    fn report_text(&self, errors: &[LintError], path: &Path) {
        let path_str = path.display();

        for error in errors {
            let location = match (error.line, error.column) {
                (Some(line), Some(col)) => format!("{}:{}:{}", path_str, line, col),
                (Some(line), None) => format!("{}:{}", path_str, line),
                _ => format!("{}", path_str),
            };

            let severity_str = match error.severity {
                Severity::Error => format!("[{}]", error.severity).red().bold(),
                Severity::Warning => format!("[{}]", error.severity).yellow().bold(),
                Severity::Info => format!("[{}]", error.severity).blue().bold(),
            };

            let rule_str = format!("[{}]", error.rule).dimmed();

            println!("{} {} {} {}", location, severity_str, rule_str, error.message);
        }

        if !errors.is_empty() {
            println!();
            let error_count = errors.iter().filter(|e| e.severity == Severity::Error).count();
            let warning_count = errors.iter().filter(|e| e.severity == Severity::Warning).count();
            let info_count = errors.iter().filter(|e| e.severity == Severity::Info).count();

            let mut parts = Vec::new();
            if error_count > 0 {
                parts.push(format!("{} error(s)", error_count));
            }
            if warning_count > 0 {
                parts.push(format!("{} warning(s)", warning_count));
            }
            if info_count > 0 {
                parts.push(format!("{} info(s)", info_count));
            }

            println!("Found {}", parts.join(", "));
        }
    }

    fn report_json(&self, errors: &[LintError], path: &Path) {
        #[derive(serde::Serialize)]
        struct JsonReport<'a> {
            file: String,
            errors: &'a [LintError],
            summary: Summary,
        }

        #[derive(serde::Serialize)]
        struct Summary {
            errors: usize,
            warnings: usize,
            infos: usize,
        }

        let report = JsonReport {
            file: path.display().to_string(),
            errors,
            summary: Summary {
                errors: errors.iter().filter(|e| e.severity == Severity::Error).count(),
                warnings: errors.iter().filter(|e| e.severity == Severity::Warning).count(),
                infos: errors.iter().filter(|e| e.severity == Severity::Info).count(),
            },
        };

        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    }
}
