mod errorformat;
mod github_actions;
mod json;

use nginx_lint_common::config::ColorConfig;
use nginx_lint_common::linter::LintError;
use std::path::Path;

/// `text` on one line, with `\r` and `\n` written as the two characters `\r`
/// and `\n`.
///
/// Reports are read line by line, and in a GitHub Actions job the runner
/// takes every output line starting with `::` as a workflow command, whatever
/// printed it. Messages and paths come from the linted tree (the parser
/// decodes `\n` in quoted strings, and git allows newlines in file names), so
/// a newline in them could start such a line: a forged annotation, or a
/// `::stop-commands::` that hides the real ones.
pub fn one_line(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains(['\r', '\n']) {
        text.replace('\r', "\\r").replace('\n', "\\n").into()
    } else {
        text.into()
    }
}

/// [`one_line`] for a path.
pub fn one_line_path(path: &Path) -> String {
    one_line(&path.display().to_string()).into_owned()
}

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

    /// Report to stdout. Returns any write error so the caller can react
    /// (e.g. a broken pipe, or a full disk truncating JSON output).
    pub fn report(
        &self,
        errors: &[LintError],
        path: &Path,
        ignored_count: usize,
    ) -> std::io::Result<()> {
        self.report_to(&mut std::io::stdout().lock(), errors, path, ignored_count)
    }

    /// Report to stderr instead of stdout. Used in `--fix` stdin mode, where
    /// stdout carries the fixed content.
    pub fn report_to_stderr(
        &self,
        errors: &[LintError],
        path: &Path,
        ignored_count: usize,
    ) -> std::io::Result<()> {
        self.report_to(&mut std::io::stderr().lock(), errors, path, ignored_count)
    }

    fn report_to(
        &self,
        writer: &mut dyn std::io::Write,
        errors: &[LintError],
        path: &Path,
        ignored_count: usize,
    ) -> std::io::Result<()> {
        match self.format {
            OutputFormat::ErrorFormat => {
                errorformat::report(writer, errors, path, &self.colors, ignored_count)
            }
            OutputFormat::Json => json::report(writer, errors, path, ignored_count),
            OutputFormat::GithubActions => github_actions::report(writer, errors, path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_escapes_line_breaks() {
        assert_eq!(
            one_line("a\n::stop-commands::zz\r\nb"),
            r"a\n::stop-commands::zz\r\nb"
        );
        assert!(matches!(
            one_line("plain"),
            std::borrow::Cow::Borrowed("plain")
        ));
        assert_eq!(
            one_line_path(Path::new("conf.d/x\n::y.conf")),
            r"conf.d/x\n::y.conf"
        );
    }
}
