//! Benchmark: WASM vs Native plugin execution
//!
//! Compares the performance of running server-tokens-enabled as a WASM plugin
//! versus running the same logic natively via NativePluginRule.
//!
//! Includes breakdown by config size to isolate instantiation vs reconstruction costs.
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

const EMPTY_CONFIG: &str = "# empty\n";

const SMALL_CONFIG: &str = r#"
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

/// Generate a large nginx config with many directives
fn generate_large_config() -> String {
    let mut config = String::from("http {\n    server_tokens on;\n\n");
    for i in 0..50 {
        config.push_str(&format!(
            "    server {{\n        listen {};\n        server_name server{}.example.com;\n        root /var/www/site{};\n        access_log /var/log/nginx/site{}.log;\n        error_log /var/log/nginx/site{}.error.log;\n\n        location / {{\n            proxy_pass http://backend{};\n            proxy_set_header Host $host;\n            proxy_set_header X-Real-IP $remote_addr;\n        }}\n\n        location /static {{\n            root /var/www/static{};\n            expires 30d;\n        }}\n    }}\n\n",
            8000 + i, i, i, i, i, i, i
        ));
    }
    config.push_str("}\n");
    config
}

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

/// Benchmark warm iterations for a given rule and config
fn bench_warm(
    rule: &dyn LintRule,
    config: &nginx_lint::parser::ast::Config,
    path: &Path,
    iterations: u32,
) -> Duration {
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = rule.check(config, path);
    }
    start.elapsed()
}

fn main() {
    let empty_config = parse_string(EMPTY_CONFIG).expect("Failed to parse empty config");
    let small_config = parse_string(SMALL_CONFIG).expect("Failed to parse small config");
    let large_config_str = generate_large_config();
    let large_config = parse_string(&large_config_str).expect("Failed to parse large config");
    let path = Path::new("bench.conf");

    // Count directives for context
    let empty_directives = empty_config.all_directives().count();
    let small_directives = small_config.all_directives().count();
    let large_directives = large_config.all_directives().count();

    println!("=== Native vs WASM Benchmark (server-tokens-enabled) ===");
    println!("Iterations: {}", ITERATIONS);
    println!();
    println!("Config sizes:");
    println!("  Empty: {} directives", empty_directives);
    println!("  Small: {} directives", small_directives);
    println!("  Large: {} directives", large_directives);
    println!();

    // Load WASM plugin
    let wasm_path = find_plugin_wasm("server_tokens_enabled");
    let cold_start = Instant::now();
    let loader = PluginLoader::new_trusted().expect("Failed to create PluginLoader");
    let wasm_rule = loader
        .load_plugin_dynamic(&wasm_path)
        .expect("Failed to load WASM plugin");
    let wasm_cold = cold_start.elapsed();

    // Create native plugin
    let native_rule = NativePluginRule::<ServerTokensEnabledPlugin>::new();

    // Verify both produce the same results
    let wasm_errors = wasm_rule.check(&small_config, path).len();
    let native_errors = native_rule.check(&small_config, path).len();
    assert_eq!(
        wasm_errors, native_errors,
        "WASM and Native should produce the same number of errors"
    );
    println!(
        "Errors found (small config): {} (both methods agree)",
        wasm_errors
    );
    println!();

    // Cold start
    println!("--- Cold Start (engine + compile + spec) ---");
    println!("  WASM:   {:>10.3?}", wasm_cold);
    println!();

    // Warm benchmarks by config size
    let wasm_empty = bench_warm(wasm_rule.as_ref(), &empty_config, path, ITERATIONS);
    let wasm_small = bench_warm(wasm_rule.as_ref(), &small_config, path, ITERATIONS);
    let wasm_large = bench_warm(wasm_rule.as_ref(), &large_config, path, ITERATIONS);

    let native_empty = bench_warm(&native_rule, &empty_config, path, ITERATIONS);
    let native_small = bench_warm(&native_rule, &small_config, path, ITERATIONS);
    let native_large = bench_warm(&native_rule, &large_config, path, ITERATIONS);

    println!("--- Warm Iterations ({} runs) ---", ITERATIONS);
    println!();
    println!(
        "  {:>22} {:>14} {:>14} {:>10}",
        "Config", "WASM/iter", "Native/iter", "Ratio"
    );
    println!(
        "  {:>22} {:>14} {:>14} {:>10}",
        "------", "---------", "-----------", "-----"
    );

    let configs = [
        ("Empty (0 dirs)", wasm_empty, native_empty),
        (
            &format!("Small ({} dirs)", small_directives),
            wasm_small,
            native_small,
        ),
        (
            &format!("Large ({} dirs)", large_directives),
            wasm_large,
            native_large,
        ),
    ];

    for (label, wasm_dur, native_dur) in &configs {
        let wasm_per = *wasm_dur / ITERATIONS;
        let native_per = *native_dur / ITERATIONS;
        let ratio = wasm_per.as_secs_f64() / native_per.as_secs_f64();
        println!(
            "  {:>22} {:>14.3?} {:>14.3?} {:>9.1}x",
            label, wasm_per, native_per, ratio
        );
    }

    println!();
    println!("--- Cost Breakdown (WASM, per iteration) ---");
    println!();

    let wasm_empty_per = wasm_empty / ITERATIONS;
    let wasm_small_per = wasm_small / ITERATIONS;
    let wasm_large_per = wasm_large / ITERATIONS;

    println!(
        "  Instantiation (empty config):      {:>10.3?}",
        wasm_empty_per
    );
    println!(
        "  Reconstruction + check (small):     {:>10.3?}  (= small - empty)",
        wasm_small_per.saturating_sub(wasm_empty_per)
    );
    println!(
        "  Reconstruction + check (large):     {:>10.3?}  (= large - empty)",
        wasm_large_per.saturating_sub(wasm_empty_per)
    );

    if large_directives > small_directives {
        let small_recon = wasm_small_per.saturating_sub(wasm_empty_per);
        let large_recon = wasm_large_per.saturating_sub(wasm_empty_per);
        let per_directive = large_recon.saturating_sub(small_recon).as_nanos()
            / (large_directives - small_directives) as u128;
        println!(
            "  Per-directive marginal cost:         {:>7}ns  (= (large - small) / delta dirs)",
            per_directive
        );
    }
}
