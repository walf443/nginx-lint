//! Benchmark to compare built-in rule vs WASM plugin performance
//!
//! Run with:
//!   cargo run --features plugins --release --example bench_plugin
//!
//! To test with multiple plugins:
//!   mkdir test_plugins
//!   cp examples/plugins/example_plugin/target/wasm32-unknown-unknown/release/example_plugin.wasm test_plugins/plugin1.wasm
//!   cp examples/plugins/example_plugin/target/wasm32-unknown-unknown/release/example_plugin.wasm test_plugins/plugin2.wasm
//!   cp examples/plugins/example_plugin/target/wasm32-unknown-unknown/release/example_plugin.wasm test_plugins/plugin3.wasm

use nginx_lint::linter::LintRule;
use nginx_lint::plugin::PluginLoader;
use nginx_lint::rules::DeprecatedSslProtocol;
use nginx_lint::{parse_string, Linter};
use std::path::Path;
use std::time::Instant;

fn main() {
    let config_content = r#"
events {
    debug_connection 192.168.1.1;
    worker_connections 1024;
}
http {
    server {
        listen 80;
        ssl_protocols TLSv1 TLSv1.1 TLSv1.2;
    }
    server {
        listen 8080;
        ssl_protocols TLSv1 TLSv1.2;
    }
}
"#;

    let config = parse_string(config_content).expect("Failed to parse");
    let path = Path::new("test.conf");
    let iterations = 1000;

    // Benchmark built-in rules (all rules)
    println!("=== Built-in linter (all {} rules) ===", {
        Linter::with_default_rules().rules().len()
    });
    let linter = Linter::with_default_rules();

    // Warmup
    for _ in 0..10 {
        let _ = linter.lint(&config, path);
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = linter.lint(&config, path);
    }
    let all_rules_duration = start.elapsed();
    println!(
        "{} iterations: {:?} total, {:?}/iter",
        iterations,
        all_rules_duration,
        all_rules_duration / iterations as u32
    );

    // Benchmark single built-in rule
    println!("\n=== Single built-in rule (DeprecatedSslProtocol) ===");
    let single_rule = DeprecatedSslProtocol::default();

    // Warmup
    for _ in 0..10 {
        let _ = single_rule.check(&config, path);
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = single_rule.check(&config, path);
    }
    let single_duration = start.elapsed();
    println!(
        "{} iterations: {:?} total, {:?}/iter",
        iterations,
        single_duration,
        single_duration / iterations as u32
    );

    // Benchmark WASM plugin
    let plugins_dir = Path::new("test_plugins");

    if !plugins_dir.exists() {
        println!("\ntest_plugins/ directory not found.");
        println!("To run WASM benchmark:");
        println!("  mkdir test_plugins");
        println!("  cp examples/plugins/example_plugin/target/wasm32-unknown-unknown/release/example_plugin.wasm test_plugins/");
        println!("\nTo test serialization cache with multiple plugins:");
        println!("  cp test_plugins/example_plugin.wasm test_plugins/plugin2.wasm");
        println!("  cp test_plugins/example_plugin.wasm test_plugins/plugin3.wasm");
        return;
    }

    let loader = PluginLoader::new().expect("Failed to create loader");
    let plugins = loader
        .load_plugins(plugins_dir)
        .expect("Failed to load plugins");

    if plugins.is_empty() {
        println!("\nNo plugins found in test_plugins/");
        return;
    }

    let plugin_count = plugins.len();
    println!("\n=== WASM plugins ({} rules) ===", plugin_count);

    let mut plugin_linter = Linter::new();
    for plugin in plugins {
        println!("Loaded: {} - {}", plugin.name(), plugin.description());
        plugin_linter.add_rule(Box::new(plugin));
    }

    // Warmup
    for _ in 0..10 {
        let _ = plugin_linter.lint(&config, path);
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = plugin_linter.lint(&config, path);
    }
    let plugin_duration = start.elapsed();
    println!(
        "{} iterations: {:?} total, {:?}/iter",
        iterations,
        plugin_duration,
        plugin_duration / iterations as u32
    );

    // Summary
    println!("\n=== Summary ===");
    println!(
        "Single built-in rule: {:?}/iter",
        single_duration / iterations as u32
    );
    println!(
        "WASM plugin (1 rule): {:?}/iter",
        plugin_duration / iterations as u32
    );

    let overhead = plugin_duration.as_secs_f64() / single_duration.as_secs_f64();
    println!("\nWASM overhead: {:.1}x slower than native", overhead);

    // Breakdown of overhead sources
    println!("\n=== Overhead breakdown (estimated) ===");
    println!("1. JSON serialization of Config AST");
    println!("2. WASM instance creation per check");
    println!("3. Memory copy (host <-> WASM)");
    println!("4. JSON deserialization in WASM");
    println!("5. WASM execution vs native");
}
