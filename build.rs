use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    // Only run wasm-pack when web-server-embed-wasm feature is enabled
    let embed_wasm = env::var("CARGO_FEATURE_WEB_SERVER_EMBED_WASM").is_ok();

    if !embed_wasm {
        return;
    }

    // Track source files for rebuild
    println!("cargo:rerun-if-changed=src/wasm.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/linter.rs");
    println!("cargo:rerun-if-changed=src/parser/mod.rs");
    println!("cargo:rerun-if-changed=src/ignore.rs");
    println!("cargo:rerun-if-changed=src/rules/mod.rs");
    println!("cargo:rerun-if-changed=demo/pkg/nginx_lint_bg.wasm");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let wasm_path = Path::new(&manifest_dir).join("demo/pkg/nginx_lint_bg.wasm");

    // Check if WASM already exists and has a reasonable size (> 10KB means it's not a stub)
    if let Ok(metadata) = wasm_path.metadata() {
        if metadata.len() > 10_000 {
            eprintln!("WASM file already exists ({}KB), skipping build", metadata.len() / 1024);
            eprintln!("To rebuild, run: wasm-pack build --target web --out-dir demo/pkg --features wasm");
            return;
        }
    }

    // Check if wasm-pack is available
    let wasm_pack_check = Command::new("wasm-pack").arg("--version").output();

    if wasm_pack_check.is_err() {
        panic!(
            "wasm-pack not found and demo/pkg/nginx_lint_bg.wasm does not exist.\n\
             Please install wasm-pack: cargo install wasm-pack\n\
             Then run: wasm-pack build --target web --out-dir demo/pkg --features wasm"
        );
    }

    eprintln!("Building WASM with wasm-pack...");
    eprintln!("(This may take a while on first build)");

    // Use a separate target directory for wasm-pack to avoid cargo lock conflicts
    let wasm_target_dir = format!("{}/target/wasm-pack", manifest_dir);

    let status = Command::new("wasm-pack")
        .current_dir(&manifest_dir)
        .env("CARGO_TARGET_DIR", &wasm_target_dir)
        .args([
            "build",
            "--target",
            "web",
            "--out-dir",
            "demo/pkg",
            "--features",
            "wasm",
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            eprintln!("WASM build successful");
        }
        Ok(s) => {
            panic!(
                "wasm-pack failed with exit code: {:?}\n\
                 If this is a lock conflict, try building WASM first:\n\
                 wasm-pack build --target web --out-dir demo/pkg --features wasm\n\
                 Then rebuild:\n\
                 cargo build --features web-server-embed-wasm",
                s.code()
            );
        }
        Err(e) => {
            panic!("Failed to run wasm-pack: {}", e);
        }
    }
}
