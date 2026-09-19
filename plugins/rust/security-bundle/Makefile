WASM := security-bundle.wasm
CORE := ../../../target/wasm32-unknown-unknown/release/security_bundle_plugin.wasm
NGINX_LINT := ../../../target/debug/nginx-lint

.PHONY: build test-e2e clean

# Built like a builtin: a core module from cargo, wrapped into a component
# by wasm-tools. The one difference is export_component_plugins! in
# src/lib.rs, listing two rules where a builtin exports one.
build:
	cargo build --target wasm32-unknown-unknown --release
	wasm-tools component new $(CORE) -o $(WASM)

# test-plugins loads the component as two rules and checks each against its
# own examples.
test-e2e: build
	$(NGINX_LINT) test-plugins --plugins .

clean:
	rm -f $(WASM)
