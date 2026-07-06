mod errorformat;
mod github_actions;
mod json;

use crate::LintError;
use crate::config::ColorConfig;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default)]
pub enum OutputFormat {
    #[default]
    ErrorFormat,
    Json,
    GithubActions,
}

pub struct Reporter {
    format: OutputFormat,
    colors: ColorConfig,
}

impl Reporter {
    pub fn new(format: OutputFormat) -> Self {
        Self {
            format,
            colors: ColorConfig::default(),
        }
    }

    pub fn with_colors(format: OutputFormat, colors: ColorConfig) -> Self {
        Self { format, colors }
    }

    pub fn report(&self, errors: &[LintError], path: &Path, ignored_count: usize) {
        self.report_to(&mut std::io::stdout().lock(), errors, path, ignored_count);
    }

    /// Report to stderr instead of stdout. Used in `--fix` stdin mode, where
    /// stdout carries the fixed content.
    pub fn report_to_stderr(&self, errors: &[LintError], path: &Path, ignored_count: usize) {
        self.report_to(&mut std::io::stderr().lock(), errors, path, ignored_count);
    }

    fn report_to(
        &self,
        writer: &mut dyn std::io::Write,
        errors: &[LintError],
        path: &Path,
        ignored_count: usize,
    ) {
        match self.format {
            OutputFormat::ErrorFormat => {
                errorformat::report(writer, errors, path, &self.colors, ignored_count)
            }
            OutputFormat::Json => json::report(writer, errors, path, ignored_count),
            OutputFormat::GithubActions => github_actions::report(writer, errors, path),
        }
    }
}
