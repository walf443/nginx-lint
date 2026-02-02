.PHONY: build build-wasm build-web build-plugins build-with-plugins clean test lint lint-plugin-examples help

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

# Build all builtin plugins
build-plugins:
	@echo "Building builtin plugins..."
	@for dir in plugins/builtin/*/; do \
		if [ -f "$$dir/Cargo.toml" ]; then \
			echo "  Building $$(basename $$dir)..."; \
			(cd "$$dir" && cargo build --target wasm32-unknown-unknown --release); \
		fi \
	done
	@echo "Done building plugins."

# Collect built plugins to target/builtin-plugins/
collect-plugins: build-plugins
	@echo "Collecting plugins..."
	@mkdir -p target/builtin-plugins
	@for dir in plugins/builtin/*/; do \
		if [ -f "$$dir/Cargo.toml" ]; then \
			name=$$(basename "$$dir"); \
			wasm_file="$$dir/target/wasm32-unknown-unknown/release/$$(echo $$name | tr '-' '_')_plugin.wasm"; \
			if [ -f "$$wasm_file" ]; then \
				cp "$$wasm_file" "target/builtin-plugins/$${name}.wasm"; \
				echo "  Collected $$name.wasm"; \
			fi \
		fi \
	done
	@echo "Done collecting plugins."

# Build binary with embedded builtin plugins
build-with-plugins: collect-plugins
	@echo "Building nginx-lint with embedded builtin plugins..."
	cargo build --release --features builtin-plugins
	@echo "Done."

# Run tests
test:
	cargo test

# Run tests including plugin tests
test-all: test
	@for dir in plugins/builtin/*/; do \
		if [ -f "$$dir/Cargo.toml" ]; then \
			echo "Testing $$(basename $$dir)..."; \
			(cd "$$dir" && cargo test); \
		fi \
	done

# Lint plugin example files to ensure they are valid nginx configs
lint-plugin-examples:
	@echo "Linting plugin examples..."
	@fail=0; \
	for dir in plugins/builtin/*/; do \
		if [ -d "$$dir/examples" ]; then \
			name=$$(basename "$$dir"); \
			echo "  Checking $$name examples..."; \
			for conf in "$$dir"/examples/*.conf; do \
				if ! cargo run --quiet --features cli -- --no-fail-on-warnings "$$conf" 2>/dev/null; then \
					echo "    ERROR: $$conf failed to parse"; \
					fail=1; \
				else \
					echo "    OK: $$(basename $$conf)"; \
				fi \
			done \
		fi \
	done; \
	if [ $$fail -eq 1 ]; then \
		echo "Some plugin examples failed validation."; \
		exit 1; \
	fi
	@echo "All plugin examples are valid."

# Run clippy
lint:
	cargo clippy

# Clean build artifacts
clean:
	cargo clean
	rm -rf demo/pkg
	rm -rf target/builtin-plugins
	@for dir in plugins/builtin/*/; do \
		if [ -f "$$dir/Cargo.toml" ]; then \
			(cd "$$dir" && cargo clean); \
		fi \
	done

# Show help
help:
	@echo "nginx-lint build targets:"
	@echo ""
	@echo "  make build              - Build CLI (release)"
	@echo "  make build-plugins      - Build WASM builtin plugins"
	@echo "  make build-with-plugins - Build CLI with embedded builtin plugins"
	@echo "  make build-wasm         - Build WASM for web demo"
	@echo "  make build-web          - Build web server with embedded WASM"
	@echo "  make run-web            - Run web demo (development)"
	@echo "  make run-web-embed      - Run web demo with embedded WASM"
	@echo "  make test               - Run tests"
	@echo "  make test-all           - Run all tests including plugins"
	@echo "  make lint               - Run clippy"
	@echo "  make lint-plugin-examples - Lint plugin example files"
	@echo "  make clean              - Clean all build artifacts"
	@echo "  make help               - Show this help"
