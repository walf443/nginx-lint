WASM := server-tokens-enabled-go.wasm
RULE := server-tokens-enabled-go
NGINX_LINT := ../../../target/debug/nginx-lint

.PHONY: build test vet check test-e2e clean

# No WIT path and no world: the SDK carries a componentize-go.toml, and
# componentize-go finds it by scanning this module's dependencies.
build:
	@command -v componentize-go >/dev/null || { \
		echo "componentize-go not found: go install github.com/bytecodealliance/componentize-go@v0.4.0"; \
		exit 1; \
	}
	componentize-go build -o $(WASM)

# Runs on the host, with no wasm and no component runtime: nothing this
# package imports reaches the generated bindings.
test:
	go test ./...

vet:
	go vet ./...

check: vet test

# Everything the unit tests cannot reach: the real parser, the bindings,
# handle ownership and the host boundary. --allow-wasi-plugins is required
# because a Go component imports wasi:* even when the plugin never uses it,
# and --rule-only makes the exit code meaningful, so a plugin that fails to
# load fails the test instead of looking like "no findings".
test-e2e: build
	$(NGINX_LINT) --plugins . --allow-wasi-plugins --rule-only $(RULE) examples/bad.conf; \
		test $$? -eq 1
	$(NGINX_LINT) --plugins . --allow-wasi-plugins --rule-only $(RULE) examples/good.conf
	@tmp=$$(mktemp -d); cp examples/bad.conf $$tmp/nginx.conf; \
		$(NGINX_LINT) --plugins . --allow-wasi-plugins --rule-only $(RULE) --fix $$tmp/nginx.conf >/dev/null; \
		diff examples/good.conf $$tmp/nginx.conf && echo "  --fix produced examples/good.conf"; \
		status=$$?; rm -rf $$tmp; exit $$status

clean:
	rm -f $(WASM)
