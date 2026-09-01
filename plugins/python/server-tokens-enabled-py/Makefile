WASM := server-tokens-enabled-py.wasm
# Both paths come from the installed SDK rather than the repository, so this
# Makefile works unchanged outside the monorepo — `pip install
# nginx-lint-plugin` is the only prerequisite.
WIT_DIR = $(shell python -c 'import nginx_lint_plugin as p; print(p.wit_dir())')
# componentize-py does not follow the .pth link an editable install leaves in
# site-packages, so point it at wherever the SDK actually lives (site-packages
# for a regular install, the source tree for `maturin develop`).
SDK_DIR = $(shell python -c 'import nginx_lint_plugin as p, pathlib; print(pathlib.Path(p.__file__).parent.parent)')

.PHONY: build test typecheck test-e2e clean

build:
	componentize-py -d "$(WIT_DIR)" -w plugin componentize app -o $(WASM) --stub-wasi \
		-p . -p "$(SDK_DIR)"

# Unit tests (plain pytest; requires the SDK installed:
# pip install ../nginx-lint-plugin)
test:
	pytest

# Type-check against the SDK's published annotations (it ships py.typed)
typecheck:
	mypy app.py test_plugin.py

# End-to-end test through the nginx-lint CLI (requires the plugin built).
# --rule-only makes the exit code meaningful: bad.conf must fail with this
# rule's warning, and good.conf must exit 0 (so a CLI crash or plugin-load
# failure fails the test instead of masquerading as "no findings").
# The plugin carries its own bad and good examples in its spec, so
# test-plugin needs nothing but the directory: it requires the bad one to be
# reported, the good one to be clean, and the fixes to resolve the bad one.
test-e2e: build
	cd ../../.. && \
	cargo run --features plugins -- test-plugin \
		--plugins plugins/python/server-tokens-enabled-py

clean:
	rm -rf $(WASM) __pycache__
