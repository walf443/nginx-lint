//! Integration tests for parsing various nginx configurations
//!
//! These tests verify that the parser can handle real-world nginx configurations.

use nginx_lint_parser::parse_config;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
fn test_parse_generated_fixtures() {
    let test_generated_dir = fixtures_dir().join("test_generated");

    // Collect all .conf files
    let conf_files: Vec<PathBuf> = std::fs::read_dir(&test_generated_dir)
        .expect("Failed to read test_generated directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "conf"))
        .collect();

    assert!(
        !conf_files.is_empty(),
        "No .conf files found in test_generated directory"
    );

    // Test parsing each file
    let mut failures: Vec<String> = Vec::new();

    for path in &conf_files {
        match parse_config(path) {
            Ok(_) => {}
            Err(e) => {
                failures.push(format!("Failed to parse {}: {}", path.display(), e));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "Parse failures:\n{}",
        failures.join("\n")
    );

    println!("Successfully parsed {} config files", conf_files.len());
}
