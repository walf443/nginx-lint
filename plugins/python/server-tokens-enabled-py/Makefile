WIT_DIR := ../../../wit
WASM := server-tokens-enabled-py.wasm
# The SDK is bundled into the component from its source tree rather than
# from site-packages: `maturin develop` installs it as a .pth link, which
# componentize-py does not follow.
SDK_DIR := ../nginx-lint-plugin/python

.PHONY: build test test-e2e clean

build:
	componentize-py -d $(WIT_DIR) -w plugin componentize app -o $(WASM) --stub-wasi \
		-p . -p $(SDK_DIR)

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
	rm -rf $(WASM) __pycache__
