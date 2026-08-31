WASM := server-tokens-enabled-go.wasm
RULE := server-tokens-enabled-go
WIT_DIR := ../../../wit
NGINX_LINT := ../../../target/debug/nginx-lint

.PHONY: bindings build vet test test-e2e clean

# componentize-go is used as an installed binary rather than through a
# `tool` directive in go.mod: its `bindings` subcommand rewrites go.mod and
# drops the directive, so the second invocation would not find itself.
COMPONENTIZE_GO := componentize-go

# Generate the Go bindings from the WIT. They are not committed (the same
# convention as the TypeScript and Python SDKs), so this runs before every
# build from a fresh checkout.
bindings:
	@command -v $(COMPONENTIZE_GO) >/dev/null || { \
		echo "componentize-go not found: go install github.com/bytecodealliance/componentize-go@v0.4.0"; \
		exit 1; \
	}
	$(COMPONENTIZE_GO) -d $(WIT_DIR) -w plugin bindings --format

build: bindings
	$(COMPONENTIZE_GO) -d $(WIT_DIR) -w plugin build -o $(WASM)

# Unit tests for the rule itself. They run on the host, without wasm or a
# component runtime, because ./rule imports none of the generated bindings —
# which is the reason it is a separate package.
test:
	go test ./rule/

# The WIT runtime this depends on has assembly-only functions that do not
# build for the host, so vetting has to target wasm; and `./...` would drag
# in the generated packages, which are not worth vetting.
vet: bindings
	GOOS=wasip1 GOARCH=wasm go vet ./export_wit_world/ ./examples/
	go vet ./rule/

# End to end through the CLI. --allow-wasi-plugins is required: a Go
# component imports wasi:* even when the plugin itself never uses it.
# --rule-only makes the exit code meaningful, so a plugin that fails to load
# fails the test instead of looking like "no findings".
test-e2e: build
	$(NGINX_LINT) --plugins . --allow-wasi-plugins --rule-only $(RULE) examples/bad.conf; \
		test $$? -eq 1
	$(NGINX_LINT) --plugins . --allow-wasi-plugins --rule-only $(RULE) examples/good.conf

clean:
	rm -rf $(WASM) wit_exports.go wit_world nginx_lint_plugin_config_api \
		nginx_lint_plugin_data_types nginx_lint_plugin_parser_types \
		nginx_lint_plugin_types
