# Plugin directories
PLUGIN_DIRS := $(wildcard plugins/builtin/*/*/)
PLUGIN_NAMES := $(foreach dir,$(PLUGIN_DIRS),$(notdir $(patsubst %/,%,$(dir))))
PLUGIN_WASMS := $(foreach name,$(PLUGIN_NAMES),target/builtin-plugins/$(name).wasm)

.PHONY: build build-wasm build-wasm-with-plugins build-web build-plugins build-with-wasm-plugins clean test lint lint-plugin-examples doc help

# Build CLI with native plugins (release, default)
build:
	cargo build --release

# Build WASM module (for web, without builtin plugins)
build-wasm:
	wasm-pack build --target web --out-dir web/pkg --no-default-features --features wasm

# Build WASM module with builtin plugins (for web)
build-wasm-with-plugins:
	wasm-pack build --target web --out-dir web/pkg --no-default-features --features wasm,native-builtin-plugins

# Build web server with embedded WASM (builds WASM first, then embeds it)
build-web: build-wasm-with-plugins
	cargo build --release --features web-server-embed-wasm

# Run web server (development mode, reads files from disk)
run-web:
	cargo run --features web-server -- web

# Run web server with embedded WASM
run-web-embed: build-web
	cargo run --release --features web-server-embed-wasm -- web

# Build all WASM builtin plugins as WIT components (requires: cargo install wasm-tools)
build-plugins:
	@command -v wasm-tools >/dev/null 2>&1 || { echo "Error: wasm-tools not found. Install with: cargo install wasm-tools"; exit 1; }
	@echo "Building plugins..."
	@for name in $(PLUGIN_NAMES); do \
		dir=$$(find plugins/builtin -type d -name "$$name" 2>/dev/null | head -1); \
		if [ -n "$$dir" ] && [ -f "$$dir/Cargo.toml" ]; then \
			echo "  Building $$name..."; \
			cargo build --manifest-path "$$dir/Cargo.toml" \
				--target wasm32-unknown-unknown \
				--target-dir "$$dir/target" \
				--release && \
			core_wasm="$$dir/target/wasm32-unknown-unknown/release/$$(echo $$name | tr '-' '_')_plugin.wasm"; \
			if [ -f "$$core_wasm" ]; then \
				wasm-tools component new "$$core_wasm" -o "$$core_wasm.component.wasm" && \
				echo "    Component: $$core_wasm.component.wasm"; \
			fi \
		fi \
	done
	@echo "Done building plugins."

# Collect built plugins to target/builtin-plugins/
collect-plugins: build-plugins
	@echo "Collecting plugins..."
	@mkdir -p target/builtin-plugins
	@for dir in plugins/builtin/*/*/; do \
		if [ -f "$$dir/Cargo.toml" ]; then \
			name=$$(basename "$$dir"); \
			component_wasm="$$dir/target/wasm32-unknown-unknown/release/$$(echo $$name | tr '-' '_')_plugin.wasm.component.wasm"; \
			if [ -f "$$component_wasm" ]; then \
				cp "$$component_wasm" "target/builtin-plugins/$${name}.wasm"; \
				echo "  Collected $$name.wasm"; \
			fi \
		fi \
	done
	@echo "Done collecting plugins."

# Build binary with embedded WASM builtin plugins (instead of native)
build-with-wasm-plugins: collect-plugins
	@echo "Building nginx-lint with embedded WASM builtin plugins..."
	cargo build --release --no-default-features --features cli,wasm-builtin-plugins
	@echo "Done."

# Run tests
test:
	cargo test

# Run tests including plugin tests
test-all: test
	@for dir in plugins/builtin/*/*/; do \
		if [ -f "$$dir/Cargo.toml" ]; then \
			echo "Testing $$(basename $$dir)..."; \
			(cd "$$dir" && cargo test); \
		fi \
	done

# Lint plugin example files to ensure they are valid nginx configs
lint-plugin-examples:
	@echo "Linting plugin examples..."
	@fail=0; \
	for dir in plugins/builtin/*/*/; do \
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

# Build API documentation
doc:
	cargo doc --no-deps -p nginx-lint-plugin -p nginx-lint-parser -p nginx-lint-common --open

# Run clippy
lint:
	cargo clippy

# Clean build artifacts
clean:
	cargo clean
	rm -rf web/pkg
	rm -rf target/builtin-plugins
	@for dir in plugins/builtin/*/*/; do \
		if [ -f "$$dir/Cargo.toml" ]; then \
			(cd "$$dir" && cargo clean); \
		fi \
	done

# Show help
help:
	@echo "nginx-lint build targets:"
	@echo ""
	@echo "  make build              - Build CLI with native plugins (release, default)"
	@echo "  make build-plugins      - Build WASM builtin plugins as WIT components"
	@echo "  make build-with-wasm-plugins - Build CLI with embedded WASM plugins"
	@echo "  make build-wasm         - Build WASM for web (without plugins)"
	@echo "  make build-wasm-with-plugins - Build WASM for web (with plugins)"
	@echo "  make build-web          - Build web server with embedded WASM (with plugins)"
	@echo "  make run-web            - Run web server (development)"
	@echo "  make run-web-embed      - Run web server with embedded WASM"
	@echo "  make test               - Run tests"
	@echo "  make test-all           - Run all tests including plugins"
	@echo "  make doc                - Build API documentation (opens in browser)"
	@echo "  make lint               - Run clippy"
	@echo "  make lint-plugin-examples - Lint plugin example files"
	@echo "  make clean              - Clean all build artifacts"
	@echo "  make help               - Show this help"
