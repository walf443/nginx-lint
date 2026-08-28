WIT_DIR := ../../../wit
WASM := server-tokens-enabled-py.wasm

.PHONY: build bindings test test-e2e clean

build:
	componentize-py -d $(WIT_DIR) -w plugin componentize app -o $(WASM) --stub-wasi

# Generate typed bindings for IDE / type checking (not needed for build;
# componentize-py generates its own bindings internally)
bindings:
	componentize-py -d $(WIT_DIR) -w plugin bindings ./bindings

# Unit tests (plain pytest; requires the SDK installed:
# pip install ../nginx-lint-plugin)
test:
	pytest

# End-to-end test through the nginx-lint CLI (requires the plugin built).
# --rule-only makes the exit code meaningful: bad.conf must fail with this
# rule's warning, and good.conf must exit 0 (so a CLI crash or plugin-load
# failure fails the test instead of masquerading as "no findings").
test-e2e: build
	cd ../../.. && \
	cargo run --features plugins -- --plugins plugins/python/server-tokens-enabled-py \
		--rule-only server-tokens-enabled-py \
		plugins/python/server-tokens-enabled-py/examples/bad.conf \
		| grep -q 'security/server-tokens-enabled-py' && \
	cargo run --features plugins -- --plugins plugins/python/server-tokens-enabled-py \
		--rule-only server-tokens-enabled-py \
		plugins/python/server-tokens-enabled-py/examples/good.conf && \
	echo "OK"

clean:
	rm -rf $(WASM) bindings __pycache__
