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

    pub fn report(&self, errors: &[LintError], path: &Path, ignored_count: usize) {
        match self.format {
            OutputFormat::Text => self.report_text(errors, path, ignored_count),
            OutputFormat::Json => self.report_json(errors, path, ignored_count),
        }
    }

    fn report_text(&self, errors: &[LintError], path: &Path, ignored_count: usize) {
        let path_str = path.display();

        // Sort errors by line number, then by column number
        let mut sorted_errors: Vec<_> = errors.iter().collect();
        sorted_errors.sort_by(|a, b| {
            match (a.line, b.line) {
                (Some(line_a), Some(line_b)) => {
                    line_a.cmp(&line_b).then_with(|| {
                        match (a.column, b.column) {
                            (Some(col_a), Some(col_b)) => col_a.cmp(&col_b),
                            (Some(_), None) => std::cmp::Ordering::Less,
                            (None, Some(_)) => std::cmp::Ordering::Greater,
                            (None, None) => std::cmp::Ordering::Equal,
                        }
                    })
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });

        for error in sorted_errors {
            let location = match (error.line, error.column) {
                (Some(line), Some(col)) => format!("{}:{}:{}", path_str, line, col),
                (Some(line), None) => format!("{}:{}", path_str, line),
                _ => format!("{}", path_str),
            };

            let severity_str = match error.severity {
                Severity::Error => apply_color(&format!("[{}]", error.severity), self.colors.error).bold(),
                Severity::Warning => apply_color(&format!("[{}]", error.severity), self.colors.warning).bold(),
            };

            let rule_str = format!("[{}/{}]", error.category, error.rule).dimmed();

            println!("{} {} {} {}", location, severity_str, rule_str, error.message);
        }

        if !errors.is_empty() || ignored_count > 0 {
            println!();
            let error_count = errors.iter().filter(|e| e.severity == Severity::Error).count();
            let warning_count = errors.iter().filter(|e| e.severity == Severity::Warning).count();

            let mut parts = Vec::new();
            if error_count > 0 {
                parts.push(format!("{} error(s)", error_count));
            }
            if warning_count > 0 {
                parts.push(format!("{} warning(s)", warning_count));
            }
            if ignored_count > 0 {
                parts.push(format!("{} ignored", ignored_count));
            }

            if !parts.is_empty() {
                println!("Found {}", parts.join(", "));
            }
        }
    }

    fn report_json(&self, errors: &[LintError], path: &Path, ignored_count: usize) {
        #[derive(serde::Serialize)]
        struct JsonReport {
            file: String,
            errors: Vec<LintError>,
            summary: Summary,
        }

        #[derive(serde::Serialize)]
        struct Summary {
            errors: usize,
            warnings: usize,
            ignored: usize,
        }

        // Sort errors by line number, then by column number
        let mut sorted_errors: Vec<_> = errors.to_vec();
        sorted_errors.sort_by(|a, b| {
            match (a.line, b.line) {
                (Some(line_a), Some(line_b)) => {
                    line_a.cmp(&line_b).then_with(|| {
                        match (a.column, b.column) {
                            (Some(col_a), Some(col_b)) => col_a.cmp(&col_b),
                            (Some(_), None) => std::cmp::Ordering::Less,
                            (None, Some(_)) => std::cmp::Ordering::Greater,
                            (None, None) => std::cmp::Ordering::Equal,
                        }
                    })
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });

        let report = JsonReport {
            file: path.display().to_string(),
            errors: sorted_errors,
            summary: Summary {
                errors: errors.iter().filter(|e| e.severity == Severity::Error).count(),
                warnings: errors.iter().filter(|e| e.severity == Severity::Warning).count(),
                ignored: ignored_count,
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
