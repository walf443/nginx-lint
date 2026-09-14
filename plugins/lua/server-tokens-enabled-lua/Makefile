WASM := server-tokens-enabled-lua.wasm
RULE := server-tokens-enabled-lua
NGINX_LINT := ../../../target/debug/nginx-lint

.PHONY: build test-e2e clean

# Nothing but the builder: it carries the Lua runtime and wraps the script
# into a component itself.
build:
	cargo run -q -p nginx-lint-plugin-sdk -- build plugin.lua -o $(WASM)

# No --allow-wasi-plugins: the Lua runtime imports nothing from wasi:*, so
# the plugin runs under the default sandbox.
test-e2e: build
	$(NGINX_LINT) test-plugins --plugins .
	@# test-plugins requires the fixes to resolve the finding, not to produce
	@# a particular text. Applying them to a file on disk and diffing pins
	@# the Lua fix offsets to the exact bytes the host writes.
	@tmp=$$(mktemp -d); cp examples/bad.conf $$tmp/nginx.conf; \
		$(NGINX_LINT) --plugins . --rule-only $(RULE) --fix $$tmp/nginx.conf >/dev/null; \
		diff examples/good.conf $$tmp/nginx.conf && echo "  --fix produced examples/good.conf"; \
		status=$$?; rm -rf $$tmp; exit $$status

clean:
	rm -f $(WASM)
