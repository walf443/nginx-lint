//! Integration tests for the rowan-based parser against all generated fixtures.
//!
//! For each `.conf` file in `tests/fixtures/test_generated/`, this verifies:
//! 1. Parsing completes without panic
//! 2. The tree is lossless (tree text == original source)
//! 3. No parse errors are reported

use nginx_lint_parser::parse_string_rowan;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
fn test_rowan_parse_generated_fixtures_lossless() {
    let test_generated_dir = fixtures_dir().join("test_generated");

    let mut conf_files: Vec<PathBuf> = std::fs::read_dir(&test_generated_dir)
        .expect("Failed to read test_generated directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "conf"))
        .collect();
    conf_files.sort();

    assert!(
        !conf_files.is_empty(),
        "No .conf files found in test_generated directory"
    );

    let mut lossless_failures: Vec<String> = Vec::new();
    let mut error_failures: Vec<String> = Vec::new();

    for path in &conf_files {
        let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!("Failed to read {}: {}", path.display(), e);
        });

        let (root, errors) = parse_string_rowan(&source);

        // Check lossless round-trip
        let tree_text = root.text().to_string();
        if tree_text != source {
            let first_diff = source
                .chars()
                .zip(tree_text.chars())
                .position(|(a, b)| a != b)
                .unwrap_or(source.len().min(tree_text.len()));
            lossless_failures.push(format!(
                "{}: lossless mismatch at byte {} (source len={}, tree len={})",
                path.file_name().unwrap().to_string_lossy(),
                first_diff,
                source.len(),
                tree_text.len(),
            ));
        }

        // Check no parse errors
        if !errors.is_empty() {
            let msgs: Vec<String> = errors
                .iter()
                .map(|e| format!("  offset {}: {}", e.offset, e.message))
                .collect();
            error_failures.push(format!(
                "{}:\n{}",
                path.file_name().unwrap().to_string_lossy(),
                msgs.join("\n"),
            ));
        }
    }

    let mut report = String::new();
    if !lossless_failures.is_empty() {
        report.push_str(&format!(
            "Lossless failures ({}/{}):\n{}\n\n",
            lossless_failures.len(),
            conf_files.len(),
            lossless_failures.join("\n"),
        ));
    }
    if !error_failures.is_empty() {
        report.push_str(&format!(
            "Parse error failures ({}/{}):\n{}\n",
            error_failures.len(),
            conf_files.len(),
            error_failures.join("\n"),
        ));
    }

    assert!(
        lossless_failures.is_empty() && error_failures.is_empty(),
        "Rowan parser fixture failures ({} files tested):\n\n{}",
        conf_files.len(),
        report,
    );

    eprintln!(
        "Rowan parser: all {} fixture files passed (lossless + no errors)",
        conf_files.len()
    );
}
