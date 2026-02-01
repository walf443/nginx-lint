//! Include directive resolution for nginx configuration files
//!
//! This module provides functionality to recursively resolve `include` directives
//! and collect all files that should be linted.

use crate::parser::ast::Config;
use glob::glob;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Represents a file to be linted with its parsed config (if parseable)
#[derive(Debug)]
pub struct IncludedFile {
    pub path: PathBuf,
    pub config: Option<Config>,
    pub parse_error: Option<String>,
}

/// Collect all files to lint, including those referenced by `include` directives.
///
/// This function recursively follows include directives and resolves glob patterns.
/// It detects and prevents circular includes.
///
/// # Arguments
/// * `root_path` - The root configuration file to start from
/// * `parse_fn` - A function to parse a config file
///
/// # Returns
/// A vector of `IncludedFile` containing all files to lint
pub fn collect_included_files<F>(root_path: &Path, parse_fn: F) -> Vec<IncludedFile>
where
    F: Fn(&Path) -> Result<Config, String> + Copy,
{
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut result: Vec<IncludedFile> = Vec::new();

    collect_recursive(root_path, &mut visited, &mut result, parse_fn);

    result
}

fn collect_recursive<F>(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    result: &mut Vec<IncludedFile>,
    parse_fn: F,
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
            });
            return;
        }
    };

    // Check for circular include
    if visited.contains(&canonical) {
        return;
    }
    visited.insert(canonical.clone());

    // Parse the file
    match parse_fn(path) {
        Ok(config) => {
            // Find include directives and resolve them
            let includes = find_include_paths(&config, path);

            // Add this file to results
            result.push(IncludedFile {
                path: path.to_path_buf(),
                config: Some(config),
                parse_error: None,
            });

            // Recursively process includes
            for include_path in includes {
                collect_recursive(&include_path, visited, result, parse_fn);
            }
        }
        Err(e) => {
            result.push(IncludedFile {
                path: path.to_path_buf(),
                config: None,
                parse_error: Some(e),
            });
        }
    }
}

/// Find all include directives in a config and resolve their paths
fn find_include_paths(config: &Config, parent_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let parent_dir = parent_path.parent().unwrap_or(Path::new("."));

    for directive in config.all_directives() {
        if directive.is("include") && let Some(pattern) = directive.first_arg() {
            let resolved = resolve_include_pattern(pattern, parent_dir);
            paths.extend(resolved);
        }
    }

    paths
}

/// Resolve an include pattern (which may contain glob wildcards) to actual file paths
fn resolve_include_pattern(pattern: &str, parent_dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Make the pattern absolute if it's relative
    let full_pattern = if Path::new(pattern).is_absolute() {
        pattern.to_string()
    } else {
        parent_dir.join(pattern).to_string_lossy().to_string()
    };

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
            // If glob fails, try as literal path
            let literal_path = parent_dir.join(pattern);
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
    fn test_resolve_include_pattern_glob() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        // Create test files
        create_test_file(dir, "conf.d/a.conf", "server {}");
        create_test_file(dir, "conf.d/b.conf", "server {}");
        create_test_file(dir, "conf.d/c.txt", "not a conf");

        let paths = resolve_include_pattern("conf.d/*.conf", dir);
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().any(|p| p.ends_with("a.conf")));
        assert!(paths.iter().any(|p| p.ends_with("b.conf")));
    }

    #[test]
    fn test_resolve_include_pattern_literal() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        create_test_file(dir, "servers/default.conf", "server {}");

        let paths = resolve_include_pattern("servers/default.conf", dir);
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("default.conf"));
    }

    #[test]
    fn test_resolve_include_pattern_not_found() {
        let temp = TempDir::new().unwrap();
        let paths = resolve_include_pattern("nonexistent/*.conf", temp.path());
        assert!(paths.is_empty());
    }
}
