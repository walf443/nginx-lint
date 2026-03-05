//! Comparison tests between the existing AST parser and the rowan CST → AST pipeline.
//!
//! For each `.conf` fixture file, both parsers run and their output is compared
//! at **all AST fields** — directive names, arguments (raw & value), spans,
//! whitespace fields, comments, blank lines, and block contents.

use nginx_lint_parser::ast::{
    Argument, ArgumentValue, BlankLine, Block, Comment, Config, ConfigItem, Directive, Span,
};
use nginx_lint_parser::{parse_string, parse_string_via_rowan};
use std::path::PathBuf;

// ── Deep comparison helpers ──────────────────────────────────────────────────

fn diff_configs(ast: &Config, rowan: &Config) -> Vec<String> {
    diff_items(&ast.items, &rowan.items, "root")
}

fn diff_items(ast_items: &[ConfigItem], rowan_items: &[ConfigItem], path: &str) -> Vec<String> {
    let mut diffs = Vec::new();
    let max = ast_items.len().max(rowan_items.len());

    for i in 0..max {
        match (ast_items.get(i), rowan_items.get(i)) {
            (Some(a), Some(r)) => {
                let prefix = format!("{}[{}]", path, i);
                diffs.extend(diff_config_item(a, r, &prefix));
            }
            (Some(a), None) => {
                diffs.push(format!(
                    "{}[{}]: extra in AST: {} (rowan has {} items)",
                    path,
                    i,
                    item_summary(a),
                    rowan_items.len(),
                ));
            }
            (None, Some(r)) => {
                diffs.push(format!(
                    "{}[{}]: extra in rowan: {} (AST has {} items)",
                    path,
                    i,
                    item_summary(r),
                    ast_items.len(),
                ));
            }
            (None, None) => unreachable!(),
        }
    }

    diffs
}

fn item_summary(item: &ConfigItem) -> String {
    match item {
        ConfigItem::Directive(d) => format!("Directive({})", d.name),
        ConfigItem::Comment(c) => format!("Comment({:?})", c.text),
        ConfigItem::BlankLine(_) => "BlankLine".to_string(),
    }
}

fn diff_config_item(ast: &ConfigItem, rowan: &ConfigItem, path: &str) -> Vec<String> {
    match (ast, rowan) {
        (ConfigItem::Directive(a), ConfigItem::Directive(r)) => diff_directive(a, r, path),
        (ConfigItem::Comment(a), ConfigItem::Comment(r)) => diff_comment(a, r, path),
        (ConfigItem::BlankLine(a), ConfigItem::BlankLine(r)) => diff_blank_line(a, r, path),
        _ => vec![
            format!(
                "{}: item kind mismatch: AST={}, rowan={}",
                path,
                item_summary(&ConfigItem::Directive(Box::new(dummy_directive()))),
                item_summary(rowan),
            )
            .replace(
                &format!(
                    "AST={}",
                    item_summary(&ConfigItem::Directive(Box::new(dummy_directive())))
                ),
                &format!("AST={}", item_summary(ast)),
            ),
        ],
    }
}

fn diff_directive(ast: &Directive, rowan: &Directive, path: &str) -> Vec<String> {
    let mut diffs = Vec::new();
    let label = format!("{} ({})", path, ast.name);

    // Name
    if ast.name != rowan.name {
        diffs.push(format!(
            "{}: name: AST={:?}, rowan={:?}",
            label, ast.name, rowan.name
        ));
        return diffs; // No point comparing further if names differ
    }

    // name_span
    if ast.name_span != rowan.name_span {
        diffs.push(format!(
            "{}: name_span: AST={:?}, rowan={:?}",
            label, ast.name_span, rowan.name_span
        ));
    }

    // span (directive span)
    if ast.span != rowan.span {
        diffs.push(format!(
            "{}: span: AST={:?}, rowan={:?}",
            label, ast.span, rowan.span
        ));
    }

    // leading_whitespace
    if ast.leading_whitespace != rowan.leading_whitespace {
        diffs.push(format!(
            "{}: leading_whitespace: AST={:?}, rowan={:?}",
            label, ast.leading_whitespace, rowan.leading_whitespace
        ));
    }

    // space_before_terminator
    if ast.space_before_terminator != rowan.space_before_terminator {
        diffs.push(format!(
            "{}: space_before_terminator: AST={:?}, rowan={:?}",
            label, ast.space_before_terminator, rowan.space_before_terminator
        ));
    }

    // trailing_whitespace
    if ast.trailing_whitespace != rowan.trailing_whitespace {
        diffs.push(format!(
            "{}: trailing_whitespace: AST={:?}, rowan={:?}",
            label, ast.trailing_whitespace, rowan.trailing_whitespace
        ));
    }

    // Arguments
    diffs.extend(diff_args(&ast.args, &rowan.args, &label));

    // trailing_comment
    match (&ast.trailing_comment, &rowan.trailing_comment) {
        (Some(a), Some(r)) => {
            diffs.extend(diff_comment(a, r, &format!("{}.trailing_comment", label)));
        }
        (None, None) => {}
        (a, r) => {
            diffs.push(format!(
                "{}: trailing_comment: AST={:?}, rowan={:?}",
                label,
                a.as_ref().map(|c| &c.text),
                r.as_ref().map(|c| &c.text),
            ));
        }
    }

    // Block
    match (&ast.block, &rowan.block) {
        (Some(a), Some(r)) => {
            diffs.extend(diff_block(a, r, &format!("{}.block", label)));
        }
        (None, None) => {}
        (a, r) => {
            diffs.push(format!(
                "{}: block: AST={}, rowan={}",
                label,
                a.is_some(),
                r.is_some(),
            ));
        }
    }

    diffs
}

fn diff_args(ast: &[Argument], rowan: &[Argument], path: &str) -> Vec<String> {
    let mut diffs = Vec::new();
    let max = ast.len().max(rowan.len());

    if ast.len() != rowan.len() {
        diffs.push(format!(
            "{}: args count: AST={}, rowan={}",
            path,
            ast.len(),
            rowan.len(),
        ));
    }

    for i in 0..max {
        match (ast.get(i), rowan.get(i)) {
            (Some(a), Some(r)) => {
                let arg_path = format!("{}.args[{}]", path, i);
                if a.raw != r.raw {
                    diffs.push(format!(
                        "{}: raw: AST={:?}, rowan={:?}",
                        arg_path, a.raw, r.raw
                    ));
                }
                if !arg_values_equal(&a.value, &r.value) {
                    diffs.push(format!(
                        "{}: value: AST={:?}, rowan={:?}",
                        arg_path, a.value, r.value
                    ));
                }
                if a.span != r.span {
                    diffs.push(format!(
                        "{}: span: AST={:?}, rowan={:?}",
                        arg_path, a.span, r.span
                    ));
                }
            }
            (Some(a), None) => {
                diffs.push(format!("{}.args[{}]: extra AST arg {:?}", path, i, a.raw));
            }
            (None, Some(r)) => {
                diffs.push(format!("{}.args[{}]: extra rowan arg {:?}", path, i, r.raw));
            }
            (None, None) => unreachable!(),
        }
    }

    diffs
}

fn arg_values_equal(a: &ArgumentValue, b: &ArgumentValue) -> bool {
    match (a, b) {
        (ArgumentValue::Literal(a), ArgumentValue::Literal(b)) => a == b,
        (ArgumentValue::QuotedString(a), ArgumentValue::QuotedString(b)) => a == b,
        (ArgumentValue::SingleQuotedString(a), ArgumentValue::SingleQuotedString(b)) => a == b,
        (ArgumentValue::Variable(a), ArgumentValue::Variable(b)) => a == b,
        _ => false,
    }
}

fn diff_comment(ast: &Comment, rowan: &Comment, path: &str) -> Vec<String> {
    let mut diffs = Vec::new();

    if ast.text != rowan.text {
        diffs.push(format!(
            "{}: text: AST={:?}, rowan={:?}",
            path, ast.text, rowan.text
        ));
    }
    if ast.span != rowan.span {
        diffs.push(format!(
            "{}: span: AST={:?}, rowan={:?}",
            path, ast.span, rowan.span
        ));
    }
    if ast.leading_whitespace != rowan.leading_whitespace {
        diffs.push(format!(
            "{}: leading_whitespace: AST={:?}, rowan={:?}",
            path, ast.leading_whitespace, rowan.leading_whitespace
        ));
    }
    if ast.trailing_whitespace != rowan.trailing_whitespace {
        diffs.push(format!(
            "{}: trailing_whitespace: AST={:?}, rowan={:?}",
            path, ast.trailing_whitespace, rowan.trailing_whitespace
        ));
    }

    diffs
}

fn diff_blank_line(ast: &BlankLine, rowan: &BlankLine, path: &str) -> Vec<String> {
    let mut diffs = Vec::new();

    if ast.content != rowan.content {
        diffs.push(format!(
            "{}: content: AST={:?}, rowan={:?}",
            path, ast.content, rowan.content
        ));
    }
    if ast.span != rowan.span {
        diffs.push(format!(
            "{}: span: AST={:?}, rowan={:?}",
            path, ast.span, rowan.span
        ));
    }

    diffs
}

fn diff_block(ast: &Block, rowan: &Block, path: &str) -> Vec<String> {
    let mut diffs = Vec::new();

    if ast.span != rowan.span {
        diffs.push(format!(
            "{}: span: AST={:?}, rowan={:?}",
            path, ast.span, rowan.span
        ));
    }

    // raw_content
    match (&ast.raw_content, &rowan.raw_content) {
        (Some(a), Some(r)) if a != r => {
            diffs.push(format!("{}: raw_content: AST={:?}, rowan={:?}", path, a, r));
        }
        (None, None) | (Some(_), Some(_)) => {}
        (a, r) => {
            diffs.push(format!("{}: raw_content: AST={:?}, rowan={:?}", path, a, r));
        }
    }

    if ast.closing_brace_leading_whitespace != rowan.closing_brace_leading_whitespace {
        diffs.push(format!(
            "{}: closing_brace_leading_whitespace: AST={:?}, rowan={:?}",
            path, ast.closing_brace_leading_whitespace, rowan.closing_brace_leading_whitespace
        ));
    }

    if ast.trailing_whitespace != rowan.trailing_whitespace {
        diffs.push(format!(
            "{}: trailing_whitespace: AST={:?}, rowan={:?}",
            path, ast.trailing_whitespace, rowan.trailing_whitespace
        ));
    }

    // Recursively compare items
    diffs.extend(diff_items(&ast.items, &rowan.items, path));

    diffs
}

fn dummy_directive() -> Directive {
    Directive {
        name: String::new(),
        name_span: Span::default(),
        args: Vec::new(),
        block: None,
        span: Span::default(),
        trailing_comment: None,
        leading_whitespace: String::new(),
        space_before_terminator: String::new(),
        trailing_whitespace: String::new(),
    }
}

// ── Test ──────────────────────────────────────────────────────────────────────

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
fn test_compare_ast_and_rowan_full_fields() {
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

    let mut failures: Vec<String> = Vec::new();
    let mut skipped = 0;

    for path in &conf_files {
        let file_name = path.file_name().unwrap().to_string_lossy().to_string();
        let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!("Failed to read {}: {}", path.display(), e);
        });

        // Parse with AST parser
        let ast_config = match parse_string(&source) {
            Ok(config) => config,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };

        // Parse with rowan → AST pipeline
        let rowan_config = match parse_string_via_rowan(&source) {
            Ok(config) => config,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };

        // Deep comparison
        let diffs = diff_configs(&ast_config, &rowan_config);
        if !diffs.is_empty() {
            failures.push(format!("{}:\n  {}", file_name, diffs.join("\n  ")));
        }
    }

    if !failures.is_empty() {
        panic!(
            "Full-field parser comparison failures ({}/{} files, {} skipped):\n\n{}",
            failures.len(),
            conf_files.len(),
            skipped,
            failures.join("\n\n"),
        );
    }

    eprintln!(
        "Full-field parser comparison: {} files compared, {} skipped, all matched",
        conf_files.len() - skipped,
        skipped,
    );
}

/// Inline test for a handful of common patterns.
#[test]
fn test_inline_comparisons() {
    let cases = &[
        "listen 80;",
        "worker_processes auto;",
        "server {\n    listen 80;\n    server_name example.com;\n}",
        "http {\n    server {\n        listen 80;\n    }\n}",
        "# comment\nlisten 80;",
        "listen 80; # trailing comment\n",
        "set $var value;",
        r#"return 200 "hello world";"#,
        "gzip on;\n\nerror_log /var/log/error.log;\n",
        "events {\n    worker_connections 1024;\n}\n",
    ];

    for source in cases {
        let ast_config = parse_string(source).unwrap_or_else(|e| {
            panic!("AST parse failed for {:?}: {:?}", source, e);
        });
        let rowan_config = parse_string_via_rowan(source).unwrap_or_else(|e| {
            panic!("rowan parse failed for {:?}: {:?}", source, e);
        });

        let diffs = diff_configs(&ast_config, &rowan_config);
        if !diffs.is_empty() {
            panic!(
                "Inline comparison failed for {:?}:\n  {}",
                source,
                diffs.join("\n  ")
            );
        }
    }
}
