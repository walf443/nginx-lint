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
        match self.format {
            OutputFormat::ErrorFormat => {
                errorformat::report(errors, path, &self.colors, ignored_count)
            }
            OutputFormat::Json => json::report(errors, path, ignored_count),
            OutputFormat::GithubActions => github_actions::report(errors, path),
        }
    }
}
