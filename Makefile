# Plugin directories
PLUGIN_DIRS := $(wildcard plugins/builtin/*/*/)
PLUGIN_NAMES := $(foreach dir,$(PLUGIN_DIRS),$(notdir $(patsubst %/,%,$(dir))))
PLUGIN_WASMS := $(foreach name,$(PLUGIN_NAMES),target/builtin-plugins/$(name).wasm)

.PHONY: build-testkit-wasm build build-wasm build-wasm-with-plugins build-web build-plugins collect-plugins collect-plugins-only build-with-wasm-plugins build-parser-wasm copy-wit build-fixer-wasm clean test lint lint-plugin-examples doc help

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

# Shared target directory for plugin builds (dependencies compiled once)
SHARED_PLUGIN_TARGET := target/wasm-plugins

# Build all WASM builtin plugins as WIT components (requires: wasm-tools)
# Uses a shared target directory so nginx-lint-plugin and other dependencies
# are compiled only once instead of per-plugin.
# copy-wit first: these compile the plugin SDK, which reads its vendored WIT,
# while the host reads the root one — building against a stale copy yields
# components that fail to instantiate with a component-type mismatch rather
# than anything that names the cause.
build-plugins: copy-wit
	@command -v wasm-tools >/dev/null 2>&1 || { echo "Error: wasm-tools not found. Install with: cargo install wasm-tools"; exit 1; }
	@echo "Building plugins (shared target: $(SHARED_PLUGIN_TARGET))..."
	@for dir in $(PLUGIN_DIRS); do \
		if [ -f "$$dir/Cargo.toml" ]; then \
			name=$$(basename "$$dir"); \
			echo "  Compiling $$name..."; \
			cargo build --manifest-path "$$dir/Cargo.toml" \
				--target wasm32-unknown-unknown \
				--target-dir $(SHARED_PLUGIN_TARGET) \
				--release || exit 1; \
		fi; \
	done
	@echo "Creating components..."
	@for dir in $(PLUGIN_DIRS); do \
		if [ -f "$$dir/Cargo.toml" ]; then \
			name=$$(basename "$$dir"); \
			wasm_name=$$(echo $$name | tr '-' '_')_plugin; \
			core_wasm=$(SHARED_PLUGIN_TARGET)/wasm32-unknown-unknown/release/$$wasm_name.wasm; \
			out_dir="$$dir/target/wasm32-unknown-unknown/release"; \
			mkdir -p "$$out_dir"; \
			wasm-tools component new "$$core_wasm" -o "$$out_dir/$$wasm_name.wasm.component.wasm" && \
			echo "  Component: $$out_dir/$$wasm_name.wasm.component.wasm"; \
		fi; \
	done
	@echo "Done building plugins."

# Collect built plugins to target/builtin-plugins/
collect-plugins: build-plugins collect-plugins-only

# The copy half of collect-plugins, without rebuilding. CI restores the
# components from build artifacts, so it needs the copy on its own.
collect-plugins-only:
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

# Build the fix applier as a WASM Component for TypeScript plugin testing.
# It lives in nginx-lint-common rather than alongside the parser component
# because that is where the applier is, and common depends on the parser —
# the reverse would be a cycle.
build-fixer-wasm:
	@command -v wasm-tools >/dev/null 2>&1 || { echo "Error: wasm-tools not found. Install with: cargo install wasm-tools"; exit 1; }
	cargo build --manifest-path crates/nginx-lint-common/Cargo.toml \
		--target wasm32-unknown-unknown --release --features wasm
	wasm-tools component new \
		target/wasm32-unknown-unknown/release/nginx_lint_common.wasm \
		-o target/wasm32-unknown-unknown/release/nginx_lint_common.component.wasm
	cd plugins/typescript/nginx-lint-plugin && \
		npx jco transpile \
			../../../target/wasm32-unknown-unknown/release/nginx_lint_common.component.wasm \
			-o wasm/fixer --name fixer --instantiation async
	@echo "Fixer component built and transpiled."

# The parser and plugin crates vendor the WIT so their wit-bindgen features
# work from the published crates too (the macro cannot read a path outside
# the package). The Go SDK vendors it for the same reason: its
# componentize-go.toml points a consuming plugin at that copy, and someone
# who `go get`s the module has no other one. All the copies are committed;
# CI checks them against the root.

# Only copy when the content differs: wit-bindgen tracks the WIT through an
# include_bytes!, so bumping its mtime alone rebuilds the SDK and every
# plugin that depends on it — which build-plugins would then do every run.
copy-wit:
	@mkdir -p crates/nginx-lint-parser/wit crates/nginx-lint-plugin/wit \
		plugins/go/nginx-lint-plugin/wit
	@for dir in crates/nginx-lint-parser crates/nginx-lint-plugin \
			plugins/go/nginx-lint-plugin; do \
		dest="$$dir/wit/nginx-lint-plugin.wit"; \
		if ! cmp -s wit/nginx-lint-plugin.wit "$$dest"; then \
			cp wit/nginx-lint-plugin.wit "$$dest"; \
			echo "  Refreshed $$dest"; \
		fi; \
	done

# Build the two core wasm modules the Go SDK's test helper runs. They are the
# same parser and fix applier the CLI uses, exposed through a JSON entry point
# instead of the component model, which Go has no runtime for. Committed under
# nginxlinttest/ because a Go module is consumed as source.
build-testkit-wasm: copy-wit
	cargo build --manifest-path crates/nginx-lint-parser/Cargo.toml \
		--target wasm32-unknown-unknown --release --features wasm-json
	cargo build --manifest-path crates/nginx-lint-common/Cargo.toml \
		--target wasm32-unknown-unknown --release --features wasm-json
	cp target/wasm32-unknown-unknown/release/nginx_lint_parser.wasm \
		plugins/go/nginx-lint-plugin/nginxlinttest/parser.wasm
	cp target/wasm32-unknown-unknown/release/nginx_lint_common.wasm \
		plugins/go/nginx-lint-plugin/nginxlinttest/fixer.wasm
	@echo "Go test-helper modules rebuilt."

# Build nginx-lint-parser as WASM Component for TypeScript plugin testing
build-parser-wasm: copy-wit
	@command -v wasm-tools >/dev/null 2>&1 || { echo "Error: wasm-tools not found. Install with: cargo install wasm-tools"; exit 1; }
	cargo build --manifest-path crates/nginx-lint-parser/Cargo.toml \
		--target wasm32-unknown-unknown --release --features wasm
	wasm-tools component new \
		target/wasm32-unknown-unknown/release/nginx_lint_parser.wasm \
		-o target/wasm32-unknown-unknown/release/nginx_lint_parser.component.wasm
	cd plugins/typescript/nginx-lint-plugin && \
		npx jco transpile \
			../../../target/wasm32-unknown-unknown/release/nginx_lint_parser.component.wasm \
			-o wasm/parser --name parser --instantiation async
	@echo "Parser component built and transpiled."

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
	@echo "  make build-parser-wasm  - Build parser WASM for TypeScript plugin testing"
	@echo "  make copy-wit           - Refresh the WIT vendored into the parser/plugin crates"
	@echo "  make build-fixer-wasm   - Build fix-applier WASM for TypeScript plugin testing"
	@echo "                            (the TypeScript SDK imports both; build them together)"
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
