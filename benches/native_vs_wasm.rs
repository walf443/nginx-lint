//! Benchmark: WASM vs Native plugin execution
//!
//! Compares the performance of running server-tokens-enabled as a WASM plugin
//! versus running the same logic natively via NativePluginRule.
//!
//! Prerequisites:
//!   - `make build-plugins` must be run before this benchmark
//!
//! Run with:
//!   cargo bench --features "plugins,native-builtin-plugins" --bench native_vs_wasm

use nginx_lint::LintRule;
use nginx_lint::parser::parse_string;
use nginx_lint::plugin::PluginLoader;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// Native plugin
use nginx_lint_plugin::native::NativePluginRule;
use server_tokens_enabled_plugin::ServerTokensEnabledPlugin;

const NGINX_CONFIG: &str = r#"
http {
    server_tokens on;

    server {
        listen 80;
        server_name example.com;

        location / {
            root /var/www/html;
            index index.html;
        }

        location /api {
            proxy_pass http://backend;
            server_tokens on;
        }
    }

    server {
        listen 443 ssl;
        server_name secure.example.com;

        ssl_certificate /etc/ssl/certs/cert.pem;
        ssl_certificate_key /etc/ssl/private/key.pem;

        server_tokens off;

        location / {
            root /var/www/secure;
        }
    }
}
"#;

const ITERATIONS: u32 = 1000;

/// Find the WASM component file for a plugin
fn find_plugin_wasm(plugin_name: &str) -> PathBuf {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let wasm_name = format!("{}_plugin", plugin_name.replace('-', "_"));
    let wasm_path = project_root
        .join("plugins/builtin/security")
        .join(plugin_name)
        .join("target/wasm32-unknown-unknown/release")
        .join(format!("{wasm_name}.wasm.component.wasm"));

    if !wasm_path.exists() {
        panic!(
            "WASM plugin not found at {:?}. Run `make build-plugins` first.",
            wasm_path
        );
    }
    wasm_path
}

fn bench_wasm(
    config: &nginx_lint::parser::ast::Config,
    path: &Path,
) -> (Duration, Duration, usize) {
    let wasm_path = find_plugin_wasm("server_tokens_enabled");

    // Cold start: includes creating engine, loading, and compiling the WASM component
    let cold_start = Instant::now();
    let loader = PluginLoader::new_trusted().expect("Failed to create PluginLoader");
    let wasm_rule = loader
        .load_plugin_dynamic(&wasm_path)
        .expect("Failed to load WASM plugin");
    let cold_errors = wasm_rule.check(config, path);
    let cold_duration = cold_start.elapsed();

    // Warm iterations: reuse the compiled module
    let warm_start = Instant::now();
    for _ in 0..ITERATIONS {
        let _ = wasm_rule.check(config, path);
    }
    let warm_duration = warm_start.elapsed();

    (cold_duration, warm_duration, cold_errors.len())
}

fn bench_native(
    config: &nginx_lint::parser::ast::Config,
    path: &Path,
) -> (Duration, Duration, usize) {
    // Cold start: includes creating the NativePluginRule
    let cold_start = Instant::now();
    let native_rule = NativePluginRule::<ServerTokensEnabledPlugin>::new();
    let cold_errors = native_rule.check(config, path);
    let cold_duration = cold_start.elapsed();

    // Warm iterations: reuse the rule
    let warm_start = Instant::now();
    for _ in 0..ITERATIONS {
        let _ = native_rule.check(config, path);
    }
    let warm_duration = warm_start.elapsed();

    (cold_duration, warm_duration, cold_errors.len())
}

fn main() {
    let config = parse_string(NGINX_CONFIG).expect("Failed to parse config");
    let path = Path::new("bench.conf");

    println!("=== Native vs WASM Benchmark (server-tokens-enabled) ===");
    println!("Iterations: {}", ITERATIONS);
    println!();

    // Run WASM benchmark
    let (wasm_cold, wasm_warm, wasm_errors) = bench_wasm(&config, path);

    // Run Native benchmark
    let (native_cold, native_warm, native_errors) = bench_native(&config, path);

    // Verify both produce the same results
    assert_eq!(
        wasm_errors, native_errors,
        "WASM and Native should produce the same number of errors"
    );

    println!("Errors found: {} (both methods agree)", wasm_errors);
    println!();

    // Cold start comparison
    println!("--- Cold Start (first execution including setup) ---");
    println!("  WASM:   {:>10.3?}", wasm_cold);
    println!("  Native: {:>10.3?}", native_cold);
    if native_cold < wasm_cold {
        let speedup = wasm_cold.as_secs_f64() / native_cold.as_secs_f64();
        println!("  Native is {:.1}x faster", speedup);
    } else {
        let speedup = native_cold.as_secs_f64() / wasm_cold.as_secs_f64();
        println!("  WASM is {:.1}x faster", speedup);
    }
    println!();

    // Warm iterations comparison
    let wasm_per_iter = wasm_warm / ITERATIONS;
    let native_per_iter = native_warm / ITERATIONS;

    println!("--- Warm Iterations ({} runs) ---", ITERATIONS);
    println!(
        "  WASM:   {:>10.3?} total, {:>10.3?}/iter",
        wasm_warm, wasm_per_iter
    );
    println!(
        "  Native: {:>10.3?} total, {:>10.3?}/iter",
        native_warm, native_per_iter
    );
    if native_per_iter < wasm_per_iter {
        let speedup = wasm_per_iter.as_secs_f64() / native_per_iter.as_secs_f64();
        println!("  Native is {:.1}x faster per iteration", speedup);
    } else {
        let speedup = native_per_iter.as_secs_f64() / wasm_per_iter.as_secs_f64();
        println!("  WASM is {:.1}x faster per iteration", speedup);
    }
}
