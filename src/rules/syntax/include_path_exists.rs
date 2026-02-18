use crate::docs::RuleDoc;
use crate::include::{apply_path_mapping, resolve_include_pattern};
use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use nginx_lint_common::config::PathMapping;
use std::path::Path;

/// Rule documentation
pub static DOC: RuleDoc = RuleDoc {
    name: "include-path-exists",
    category: "syntax",
    description: "Detects include directives that reference non-existent files",
    severity: "error",
    why: r#"When an include directive references a file that does not exist,
nginx will fail to start. Glob patterns that match no files are
accepted by nginx but may indicate a misconfiguration."#,
    bad_example: include_str!("include_path_exists/bad.conf"),
    good_example: include_str!("include_path_exists/good.conf"),
    references: &["https://nginx.org/en/docs/ngx_core_module.html#include"],
};

/// Check that files referenced by include directives exist
pub struct IncludePathExists {
    path_mappings: Vec<PathMapping>,
}

impl Default for IncludePathExists {
    fn default() -> Self {
        Self::new()
    }
}

impl IncludePathExists {
    pub fn new() -> Self {
        Self {
            path_mappings: Vec::new(),
        }
    }

    pub fn with_path_mappings(path_mappings: Vec<PathMapping>) -> Self {
        Self { path_mappings }
    }
}

/// Returns true if the pattern contains glob wildcard characters
fn is_glob_pattern(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[')
}

impl LintRule for IncludePathExists {
    fn name(&self) -> &'static str {
        "include-path-exists"
    }

    fn category(&self) -> &'static str {
        "syntax"
    }

    fn description(&self) -> &'static str {
        "Detects include directives that reference non-existent files"
    }

    fn check(&self, config: &Config, path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();
        let parent_dir = path.parent().unwrap_or(Path::new("."));

        for directive in config.all_directives() {
            if !directive.is("include") {
                continue;
            }

            let pattern = match directive.first_arg() {
                Some(p) => p,
                None => continue,
            };

            // Apply path mappings (chained)
            let mapped_pattern = self
                .path_mappings
                .iter()
                .fold(pattern.to_string(), |p, mapping| {
                    apply_path_mapping(&p, mapping)
                });

            // Skip absolute paths (environment-dependent)
            if Path::new(&mapped_pattern).is_absolute() {
                continue;
            }

            // Resolve the pattern and check if any files match
            let resolved = resolve_include_pattern(&mapped_pattern, parent_dir, &[]);

            if resolved.is_empty() {
                let line = directive.span.start.line;
                let column = directive.span.start.column;

                if is_glob_pattern(&mapped_pattern) {
                    errors.push(
                        LintError::new(
                            self.name(),
                            self.category(),
                            &format!(
                                "Include pattern '{}' does not match any files",
                                mapped_pattern
                            ),
                            Severity::Warning,
                        )
                        .with_location(line, column),
                    );
                } else {
                    errors.push(
                        LintError::new(
                            self.name(),
                            self.category(),
                            &format!("Included file '{}' does not exist", mapped_pattern),
                            Severity::Error,
                        )
                        .with_location(line, column),
                    );
                }
            }
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_file(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        path
    }

    fn parse_and_check(
        dir: &std::path::Path,
        config_name: &str,
        content: &str,
        path_mappings: Vec<PathMapping>,
    ) -> Vec<LintError> {
        let config_path = create_test_file(dir, config_name, content);
        let config = crate::parser::parse_string(content).unwrap();
        let rule = IncludePathExists::with_path_mappings(path_mappings);
        rule.check(&config, &config_path)
    }

    #[test]
    fn test_relative_include_exists() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();
        create_test_file(dir, "conf.d/default.conf", "server {}");

        let errors = parse_and_check(dir, "nginx.conf", "include conf.d/default.conf;", vec![]);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_relative_include_not_found() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        let errors = parse_and_check(
            dir,
            "nginx.conf",
            "include conf.d/nonexistent.conf;",
            vec![],
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, Severity::Error);
        assert!(errors[0].message.contains("does not exist"));
    }

    #[test]
    fn test_absolute_path_skipped() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        let errors = parse_and_check(
            dir,
            "nginx.conf",
            "include /etc/nginx/conf.d/*.conf;",
            vec![],
        );
        assert!(
            errors.is_empty(),
            "Absolute paths should be skipped, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_glob_pattern_matches() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();
        create_test_file(dir, "conf.d/a.conf", "server {}");
        create_test_file(dir, "conf.d/b.conf", "server {}");

        let errors = parse_and_check(dir, "nginx.conf", "include conf.d/*.conf;", vec![]);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_glob_pattern_no_match() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        let errors = parse_and_check(dir, "nginx.conf", "include conf.d/*.conf;", vec![]);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, Severity::Warning);
        assert!(errors[0].message.contains("does not match any files"));
    }

    #[test]
    fn test_path_mapping_absolute_to_relative() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();
        create_test_file(dir, "conf.d/app.conf", "server {}");

        let mappings = vec![PathMapping {
            from: "/etc/nginx".to_string(),
            to: "".to_string(),
        }];

        let errors = parse_and_check(
            dir,
            "nginx.conf",
            "include /etc/nginx/conf.d/app.conf;",
            mappings,
        );
        assert!(
            errors.is_empty(),
            "Path mapping should convert absolute to relative, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_path_mapping_absolute_to_relative_not_found() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        let mappings = vec![PathMapping {
            from: "/etc/nginx".to_string(),
            to: "".to_string(),
        }];

        let errors = parse_and_check(
            dir,
            "nginx.conf",
            "include /etc/nginx/conf.d/missing.conf;",
            mappings,
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, Severity::Error);
        assert!(errors[0].message.contains("does not exist"));
    }

    #[test]
    fn test_path_mapping_still_absolute_skipped() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        // Mapping replaces one absolute prefix with another → still absolute → skip
        let mappings = vec![PathMapping {
            from: "sites-enabled".to_string(),
            to: "sites-available".to_string(),
        }];

        let errors = parse_and_check(
            dir,
            "nginx.conf",
            "include /etc/nginx/sites-enabled/app.conf;",
            mappings,
        );
        assert!(
            errors.is_empty(),
            "Still-absolute paths after mapping should be skipped, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_multiple_includes() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();
        create_test_file(dir, "mime.types", "types {}");

        let content = r#"
include mime.types;
include missing.conf;
"#;
        let errors = parse_and_check(dir, "nginx.conf", content, vec![]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("missing.conf"));
    }

    #[test]
    fn test_include_in_block() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        let content = r#"
http {
    include conf.d/nonexistent.conf;
}
"#;
        let errors = parse_and_check(dir, "nginx.conf", content, vec![]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("does not exist"));
    }

    #[test]
    fn test_no_arg_include_skipped() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        // Malformed include with no argument — should not panic
        let config_path = create_test_file(dir, "nginx.conf", "include;");
        let config = crate::parser::parse_string("include;").unwrap();
        let rule = IncludePathExists::new();
        let errors = rule.check(&config, &config_path);
        assert!(errors.is_empty());
    }
}
