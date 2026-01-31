use crate::config::{Color, ColorConfig};
use crate::LintError;
use crate::Severity;
use colored::{ColoredString, Colorize};
use std::path::Path;

#[derive(Debug, Clone, Copy, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
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
                Severity::Error => apply_color(&format!("[{}]", error.severity), self.colors.error).bold(),
                Severity::Warning => apply_color(&format!("[{}]", error.severity), self.colors.warning).bold(),
                Severity::Info => apply_color(&format!("[{}]", error.severity), self.colors.info).bold(),
            };

            let rule_str = format!("[{}/{}]", error.category, error.rule).dimmed();

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

/// Apply a color to a string
fn apply_color(s: &str, color: Color) -> ColoredString {
    match color {
        Color::Black => s.black(),
        Color::Red => s.red(),
        Color::Green => s.green(),
        Color::Yellow => s.yellow(),
        Color::Blue => s.blue(),
        Color::Magenta => s.magenta(),
        Color::Cyan => s.cyan(),
        Color::White => s.white(),
        Color::BrightBlack => s.bright_black(),
        Color::BrightRed => s.bright_red(),
        Color::BrightGreen => s.bright_green(),
        Color::BrightYellow => s.bright_yellow(),
        Color::BrightBlue => s.bright_blue(),
        Color::BrightMagenta => s.bright_magenta(),
        Color::BrightCyan => s.bright_cyan(),
        Color::BrightWhite => s.bright_white(),
    }
}
