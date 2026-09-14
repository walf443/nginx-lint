WASM := server-tokens-enabled-lua.wasm
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

clean:
	rm -f $(WASM)
