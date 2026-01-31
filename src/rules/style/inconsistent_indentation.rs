use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::fs;
use std::path::Path;

/// Check for inconsistent indentation
pub struct InconsistentIndentation {
    /// Expected spaces per indent level (default: 4)
    pub indent_size: usize,
}

impl Default for InconsistentIndentation {
    fn default() -> Self {
        Self { indent_size: 4 }
    }
}

impl LintRule for InconsistentIndentation {
    fn name(&self) -> &'static str {
        "inconsistent-indentation"
    }

    fn description(&self) -> &'static str {
        "Detects inconsistent indentation in nginx configuration"
    }

    fn check(&self, _config: &Config, path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return errors,
        };

        let mut expected_depth: i32 = 0;
        let mut detected_indent_size: Option<usize> = None;

        for (line_num, line) in content.lines().enumerate() {
            let line_number = line_num + 1;
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Calculate current indentation
            let leading_spaces = line.len() - line.trim_start().len();

            // Detect if line uses tabs
            if line.starts_with('\t') {
                errors.push(
                    LintError::new(
                        self.name(),
                        "Use spaces instead of tabs for indentation",
                        Severity::Warning,
                    )
                    .with_location(line_number, 1),
                );
                continue;
            }

            // Adjust expected depth before checking if line starts with }
            let closes_block = trimmed.starts_with('}');
            if closes_block {
                expected_depth -= 1;
            }

            // Detect indent size from first indented line
            if detected_indent_size.is_none() && leading_spaces > 0 && expected_depth > 0 {
                detected_indent_size = Some(leading_spaces / expected_depth as usize);
            }

            let indent_size = detected_indent_size.unwrap_or(self.indent_size);
            let expected_spaces = (expected_depth.max(0) as usize) * indent_size;

            // Check indentation
            if leading_spaces != expected_spaces {
                errors.push(
                    LintError::new(
                        self.name(),
                        &format!(
                            "Expected {} spaces of indentation, found {}",
                            expected_spaces, leading_spaces
                        ),
                        Severity::Warning,
                    )
                    .with_location(line_number, 1),
                );
            }

            // Adjust expected depth after checking if line ends with {
            if trimmed.ends_with('{') {
                expected_depth += 1;
            }
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::Config;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn check_content(content: &str) -> Vec<LintError> {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        let path = file.path().to_path_buf();

        let rule = InconsistentIndentation::default();
        let config = Config::new();
        rule.check(&config, &path)
    }

    #[test]
    fn test_correct_indentation() {
        let content = r#"http {
    server {
        listen 80;
    }
}
"#;
        let errors = check_content(content);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_wrong_indentation() {
        // Mixed indentation: first level is 4 spaces, but inner content uses 2
        let content = r#"http {
    server {
  listen 80;
    }
}
"#;
        let errors = check_content(content);
        assert!(!errors.is_empty(), "Expected indentation errors");
    }

    #[test]
    fn test_tab_indentation() {
        let content = "http {\n\tserver {\n\t}\n}\n";
        let errors = check_content(content);
        let tab_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("tabs"))
            .collect();
        assert!(!tab_errors.is_empty(), "Expected tab warning");
    }
}
