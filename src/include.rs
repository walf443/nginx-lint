//! Include directive resolution for nginx configuration files
//!
//! This module provides functionality to recursively resolve `include` directives
//! and collect all files that should be linted.

use crate::parser::ast::Config;
use glob::glob;
use nginx_lint_common::config::PathMapping;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Represents a file to be linted with its parsed config (if parseable)
#[derive(Debug)]
pub struct IncludedFile {
    pub path: PathBuf,
    pub config: Option<Config>,
    pub parse_error: Option<String>,
    /// The context (parent directive stack) where this file was included
    /// Empty for root file, e.g., ["http", "server"] for a file included in server block
    pub include_context: Vec<String>,
}

/// Collect all files to lint, including those referenced by `include` directives.
///
/// This function recursively follows include directives and resolves glob patterns.
/// It detects and prevents circular includes.
///
/// # Arguments
/// * `root_path` - The root configuration file to start from
/// * `parse_fn` - A function to parse a config file
/// * `path_mappings` - Path mappings applied (in order) to include patterns before resolving
///
/// # Returns
/// A vector of `IncludedFile` containing all files to lint
pub fn collect_included_files<F>(
    root_path: &Path,
    parse_fn: F,
    path_mappings: &[PathMapping],
) -> Vec<IncludedFile>
where
    F: Fn(&Path) -> Result<Config, String> + Copy,
{
    collect_included_files_with_context(root_path, parse_fn, Vec::new(), path_mappings)
}

/// Collect all files to lint with a specified initial context.
///
/// This is useful for linting standalone files (like sites-available/*.conf)
/// that are normally included from a parent config. By specifying the context,
/// context-aware rules can properly detect issues.
///
/// # Arguments
/// * `root_path` - The root configuration file to start from
/// * `parse_fn` - A function to parse a config file
/// * `initial_context` - The parent context (e.g., ["http", "server"])
/// * `path_mappings` - Path mappings applied (in order) to include patterns before resolving
///
/// # Returns
/// A vector of `IncludedFile` containing all files to lint
pub fn collect_included_files_with_context<F>(
    root_path: &Path,
    parse_fn: F,
    initial_context: Vec<String>,
    path_mappings: &[PathMapping],
) -> Vec<IncludedFile>
where
    F: Fn(&Path) -> Result<Config, String> + Copy,
{
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut result: Vec<IncludedFile> = Vec::new();

    collect_recursive(
        root_path,
        &mut visited,
        &mut result,
        parse_fn,
        initial_context,
        path_mappings,
    );

    result
}

fn collect_recursive<F>(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    result: &mut Vec<IncludedFile>,
    parse_fn: F,
    include_context: Vec<String>,
    path_mappings: &[PathMapping],
) where
    F: Fn(&Path) -> Result<Config, String> + Copy,
{
    // Canonicalize path to detect circular includes
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // File doesn't exist or can't be accessed
            result.push(IncludedFile {
                path: path.to_path_buf(),
                config: None,
                parse_error: Some(format!("File not found: {}", path.display())),
                include_context: include_context.clone(),
            });
            return;
        }
    };

    // Check for circular include
    if visited.contains(&canonical) {
        return;
    }
    visited.insert(canonical.clone());

    // Determine the effective context:
    // 1. If include_context is provided (from parent or CLI), use it
    // 2. Otherwise, check for # nginx-lint:context comment in the file
    let effective_context = if !include_context.is_empty() {
        include_context.clone()
    } else {
        // Try to read file and parse context comment
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| crate::ignore::parse_context_comment(&content))
            .unwrap_or_default()
    };

    // Parse the file
    match parse_fn(path) {
        Ok(mut config) => {
            // Set the include context on the config
            config.include_context = effective_context.clone();

            // Find include directives with their contexts and resolve them
            let includes = find_include_paths_with_context(&config, path, path_mappings);

            // Add this file to results
            result.push(IncludedFile {
                path: path.to_path_buf(),
                config: Some(config),
                parse_error: None,
                include_context: effective_context.clone(),
            });

            // Recursively process includes with their contexts
            for (include_path, child_context) in includes {
                collect_recursive(
                    &include_path,
                    visited,
                    result,
                    parse_fn,
                    child_context,
                    path_mappings,
                );
            }
        }
        Err(e) => {
            result.push(IncludedFile {
                path: path.to_path_buf(),
                config: None,
                parse_error: Some(e),
                include_context: effective_context,
            });
        }
    }
}

/// Find all include directives in a config and resolve their paths with context
fn find_include_paths_with_context(
    config: &Config,
    parent_path: &Path,
    path_mappings: &[PathMapping],
) -> Vec<(PathBuf, Vec<String>)> {
    let mut results = Vec::new();
    let parent_dir = parent_path.parent().unwrap_or(Path::new("."));

    // Start with the include context from the config (from parent file)
    let base_context = config.include_context.clone();

    // Recursively find includes and track their context
    find_includes_recursive(
        &config.items,
        parent_dir,
        &base_context,
        path_mappings,
        &mut results,
    );

    results
}

/// Recursively find include directives while tracking the context stack
fn find_includes_recursive(
    items: &[crate::parser::ast::ConfigItem],
    parent_dir: &Path,
    context: &[String],
    path_mappings: &[PathMapping],
    results: &mut Vec<(PathBuf, Vec<String>)>,
) {
    use crate::parser::ast::ConfigItem;

    for item in items {
        if let ConfigItem::Directive(directive) = item {
            if directive.is("include")
                && let Some(pattern) = directive.first_arg()
            {
                let resolved = resolve_include_pattern(pattern, parent_dir, path_mappings);
                for path in resolved {
                    results.push((path, context.to_vec()));
                }
            }

            // Recurse into blocks with updated context
            if let Some(block) = &directive.block {
                let mut new_context = context.to_vec();
                new_context.push(directive.name.clone());
                find_includes_recursive(
                    &block.items,
                    parent_dir,
                    &new_context,
                    path_mappings,
                    results,
                );
            }
        }
    }
}

/// Apply a single path mapping to a pattern using path-component–level matching.
///
/// `from` and `to` are matched as complete path segments separated by `/`, so
/// a mapping `from = "sites-enabled"` will match the segment `sites-enabled`
/// but will NOT match `asites-enabled` or `sites-enabled-old`.
///
/// Both `from` and `to` may span multiple segments (e.g. `from = "a/b"`).
/// All occurrences of the segment sequence are replaced.
fn apply_path_mapping(pattern: &str, mapping: &PathMapping) -> String {
    let from_comps: Vec<&str> = mapping.from.split('/').filter(|s| !s.is_empty()).collect();
    let to_comps: Vec<&str> = mapping.to.split('/').filter(|s| !s.is_empty()).collect();

    if from_comps.is_empty() {
        return pattern.to_string();
    }

    let is_absolute = pattern.starts_with('/');
    let pat_comps: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let n = pat_comps.len();
    let m = from_comps.len();

    if n < m {
        return pattern.to_string();
    }

    let mut result: Vec<&str> = Vec::with_capacity(n);
    let mut matched_at_start = false;
    let mut i = 0;
    while i < n {
        if i + m <= n && pat_comps[i..i + m] == from_comps[..] {
            if i == 0 {
                matched_at_start = true;
            }
            result.extend_from_slice(&to_comps);
            i += m;
        } else {
            result.push(pat_comps[i]);
            i += 1;
        }
    }

    // When the match starts at the beginning of the path, the result's
    // absolute/relative nature may change:
    //   - `to` is empty  → prefix removed, result becomes relative
    //   - `to` starts with `/` → result becomes absolute
    //   - otherwise → keep the original absolute/relative nature
    let result_is_absolute = if matched_at_start && to_comps.is_empty() {
        false
    } else if matched_at_start && mapping.to.starts_with('/') {
        true
    } else {
        is_absolute
    };

    if result_is_absolute {
        format!("/{}", result.join("/"))
    } else {
        result.join("/")
    }
}

/// Resolve an include pattern (which may contain glob wildcards) to actual file paths.
/// Path mappings are applied in order before glob expansion.
fn resolve_include_pattern(
    pattern: &str,
    parent_dir: &Path,
    path_mappings: &[PathMapping],
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Make the pattern absolute if it's relative
    let full_pattern = if Path::new(pattern).is_absolute() {
        pattern.to_string()
    } else {
        parent_dir.join(pattern).to_string_lossy().to_string()
    };

    // Normalize path separators to `/` so that path mappings (which split on `/`)
    // and glob patterns behave consistently across platforms, including Windows.
    let full_pattern = full_pattern.replace('\\', "/");

    // Apply path mappings in order (chained: each mapping receives the output of the previous).
    // Matching is done at the path-component level to avoid partial name matches.
    let full_pattern = path_mappings
        .iter()
        .fold(full_pattern, |p, mapping| apply_path_mapping(&p, mapping));

    // Expand glob pattern
    match glob(&full_pattern) {
        Ok(entries) => {
            for entry in entries.flatten() {
                if entry.is_file() {
                    paths.push(entry);
                }
            }
        }
        Err(_) => {
            // If glob fails, try as literal path (use the mapped pattern, not the original)
            let literal_path = PathBuf::from(&full_pattern);
            if literal_path.is_file() {
                paths.push(literal_path);
            }
        }
    }

    // Sort for consistent ordering
    paths.sort();
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_apply_path_mapping_exact_segment() {
        let m = PathMapping {
            from: "sites-enabled".to_string(),
            to: "sites-available".to_string(),
        };
        assert_eq!(
            apply_path_mapping("/etc/nginx/sites-enabled/app.conf", &m),
            "/etc/nginx/sites-available/app.conf"
        );
    }

    #[test]
    fn test_apply_path_mapping_no_partial_match() {
        let m = PathMapping {
            from: "sites-enabled".to_string(),
            to: "sites-available".to_string(),
        };
        // Prefix or suffix should not match
        assert_eq!(
            apply_path_mapping("/etc/nginx/asites-enabled/app.conf", &m),
            "/etc/nginx/asites-enabled/app.conf"
        );
        assert_eq!(
            apply_path_mapping("/etc/nginx/sites-enabled-old/app.conf", &m),
            "/etc/nginx/sites-enabled-old/app.conf"
        );
    }

    #[test]
    fn test_apply_path_mapping_multi_segment_from() {
        // "nginx" followed immediately by "sites-enabled" are consecutive segments → matches
        let m = PathMapping {
            from: "nginx/sites-enabled".to_string(),
            to: "local/sites-available".to_string(),
        };
        assert_eq!(
            apply_path_mapping("/etc/nginx/sites-enabled/app.conf", &m),
            "/etc/local/sites-available/app.conf"
        );
        // "other" before "sites-enabled" means ["other","sites-enabled"] ≠ ["nginx","sites-enabled"]
        assert_eq!(
            apply_path_mapping("/etc/other/sites-enabled/app.conf", &m),
            "/etc/other/sites-enabled/app.conf"
        );
        // Works with a different prefix too
        assert_eq!(
            apply_path_mapping("/usr/nginx/sites-enabled/app.conf", &m),
            "/usr/local/sites-available/app.conf"
        );
    }

    #[test]
    fn test_apply_path_mapping_replaces_all_occurrences() {
        let m = PathMapping {
            from: "foo".to_string(),
            to: "bar".to_string(),
        };
        assert_eq!(
            apply_path_mapping("/foo/baz/foo/qux", &m),
            "/bar/baz/bar/qux"
        );
    }

    #[test]
    fn test_apply_path_mapping_to_empty_converts_absolute_to_relative() {
        // from = "/etc/nginx", to = "" strips the prefix and makes the path relative
        let m = PathMapping {
            from: "/etc/nginx".to_string(),
            to: "".to_string(),
        };
        assert_eq!(
            apply_path_mapping("/etc/nginx/sites-enabled/app.conf", &m),
            "sites-enabled/app.conf"
        );
    }

    #[test]
    fn test_apply_path_mapping_to_empty_mid_path_keeps_absolute() {
        // When the empty-to match is NOT at the start, the path stays absolute
        let m = PathMapping {
            from: "sites-enabled".to_string(),
            to: "".to_string(),
        };
        assert_eq!(
            apply_path_mapping("/etc/nginx/sites-enabled/app.conf", &m),
            "/etc/nginx/app.conf"
        );
    }

    #[test]
    fn test_apply_path_mapping_relative_path() {
        let m = PathMapping {
            from: "sites-enabled".to_string(),
            to: "sites-available".to_string(),
        };
        assert_eq!(
            apply_path_mapping("sites-enabled/app.conf", &m),
            "sites-available/app.conf"
        );
    }

    #[test]
    fn test_resolve_include_pattern_glob() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        // Create test files
        create_test_file(dir, "conf.d/a.conf", "server {}");
        create_test_file(dir, "conf.d/b.conf", "server {}");
        create_test_file(dir, "conf.d/c.txt", "not a conf");

        let paths = resolve_include_pattern("conf.d/*.conf", dir, &[]);
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().any(|p| p.ends_with("a.conf")));
        assert!(paths.iter().any(|p| p.ends_with("b.conf")));
    }

    #[test]
    fn test_resolve_include_pattern_literal() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        create_test_file(dir, "servers/default.conf", "server {}");

        let paths = resolve_include_pattern("servers/default.conf", dir, &[]);
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("default.conf"));
    }

    #[test]
    fn test_resolve_include_pattern_not_found() {
        let temp = TempDir::new().unwrap();
        let paths = resolve_include_pattern("nonexistent/*.conf", temp.path(), &[]);
        assert!(paths.is_empty());
    }

    #[test]
    fn test_resolve_include_pattern_with_mapping() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        // Files live in sites-available, not sites-enabled
        create_test_file(dir, "sites-available/app.conf", "server {}");

        let mappings = vec![PathMapping {
            from: "sites-enabled".to_string(),
            to: "sites-available".to_string(),
        }];

        let paths = resolve_include_pattern("sites-enabled/*.conf", dir, &mappings);
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("app.conf"));
    }

    #[test]
    fn test_resolve_include_pattern_mapping_does_not_match_partial_segment() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        // Files live only in sites-available
        create_test_file(dir, "sites-available/app.conf", "server {}");

        let mappings = vec![PathMapping {
            from: "sites-enabled".to_string(),
            to: "sites-available".to_string(),
        }];

        // "asites-enabled" is a different segment name; mapping must not alter it,
        // so the glob resolves to asites-available/ which does not exist → empty
        let paths = resolve_include_pattern("asites-enabled/*.conf", dir, &mappings);
        assert!(
            paths.is_empty(),
            "asites-enabled should not be mapped to asites-available"
        );

        // "sites-enabled-old" is also a different segment → not mapped → empty
        let paths = resolve_include_pattern("sites-enabled-old/*.conf", dir, &mappings);
        assert!(
            paths.is_empty(),
            "sites-enabled-old should not be mapped to sites-available-old"
        );

        // But the exact segment "sites-enabled" IS mapped → finds the file
        let paths = resolve_include_pattern("sites-enabled/*.conf", dir, &mappings);
        assert_eq!(
            paths.len(),
            1,
            "sites-enabled should be mapped to sites-available"
        );
    }

    #[test]
    fn test_resolve_include_pattern_mapping_multi_segment_from() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        // from spans two segments: "nginx/sites-enabled" → "nginx/sites-available"
        create_test_file(dir, "nginx/sites-available/app.conf", "server {}");

        let mappings = vec![PathMapping {
            from: "nginx/sites-enabled".to_string(),
            to: "nginx/sites-available".to_string(),
        }];

        let paths = resolve_include_pattern("nginx/sites-enabled/*.conf", dir, &mappings);
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("app.conf"));
    }

    #[test]
    fn test_resolve_include_pattern_chained_mappings() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        create_test_file(dir, "conf/app.conf", "server {}");

        let mappings = vec![
            PathMapping {
                from: "sites-enabled".to_string(),
                to: "sites-available".to_string(),
            },
            PathMapping {
                from: "sites-available".to_string(),
                to: "conf".to_string(),
            },
        ];

        let paths = resolve_include_pattern("sites-enabled/*.conf", dir, &mappings);
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("app.conf"));
    }

    #[test]
    fn test_resolve_include_pattern_glob_error_fallback_uses_mapped_path() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        // A filename containing an unclosed `[` causes glob to return PatternError,
        // triggering the literal-path fallback.
        create_test_file(dir, "sites-available/app[.conf", "server {}");

        let mappings = vec![PathMapping {
            from: "sites-enabled".to_string(),
            to: "sites-available".to_string(),
        }];

        // The pattern includes `[` which is invalid glob syntax.
        // After mapping: sites-enabled → sites-available, so the fallback should
        // try the mapped literal path and find the file.
        let paths = resolve_include_pattern("sites-enabled/app[.conf", dir, &mappings);
        assert_eq!(
            paths.len(),
            1,
            "glob error fallback should use the mapped path"
        );
        assert!(paths[0].ends_with("app[.conf"));
    }
}
