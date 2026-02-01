.PHONY: build build-wasm build-web clean test

# Build CLI only
build:
	cargo build --release

# Build WASM module
build-wasm:
	wasm-pack build --target web --out-dir demo/pkg --features wasm

# Build web server with embedded WASM (builds WASM first, then embeds it)
build-web: build-wasm
	cargo build --release --features web-server-embed-wasm

# Run web demo (development mode, reads files from disk)
run-web:
	cargo run --features web-server -- web

# Run web demo with embedded WASM
run-web-embed: build-web
	cargo run --release --features web-server-embed-wasm -- web

# Run tests
test:
	cargo test

# Run clippy
lint:
	cargo clippy

# Clean build artifacts
clean:
	cargo clean
	rm -rf demo/pkg
