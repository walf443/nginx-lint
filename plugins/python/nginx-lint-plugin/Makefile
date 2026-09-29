WIT_DIR := ../../../wit
TESTKIT_DIR := ../../go/nginx-lint-plugin/nginxlinttest
PY_DIR := python
PKG_DIR := $(PY_DIR)/nginx_lint_plugin
TMP_DIR := .bindings-tmp

.PHONY: bindings copy-wit copy-testkit install develop test clean

# Ship the WIT inside the package (like the TS SDK's `files: [..., "wit"]`)
# so plugin authors who only `pip install nginx-lint-plugin` can still run
# componentize-py, which requires the interface definition. The copy is
# generated, not committed; nginx_lint_plugin.wit_dir() resolves it.
copy-wit:
	mkdir -p $(PKG_DIR)/wit
	cp $(WIT_DIR)/nginx-lint-plugin.wit $(PKG_DIR)/wit/

# The parser and fix applier `testing` runs, as core wasm modules. The Go
# SDK's test helper uses the same pair and commits them (a Go module cannot
# build anything on the way in), and `make check-testkit-wasm` at the root
# keeps that copy current, so this copies it rather than keeping a second
# one. Like the WIT, the copy is not committed but ships in the package.
copy-testkit:
	mkdir -p $(PKG_DIR)/wasm
	cp $(TESTKIT_DIR)/parser.wasm $(TESTKIT_DIR)/fixer.wasm $(PKG_DIR)/wasm/

# Generate the componentize-py bindings from the WIT. They are not committed
# (same convention as the TS SDK, which regenerates on every npm run build),
# so this must run before installing or testing from a fresh checkout.
# componentize-py refuses to write into an existing directory, hence the
# temporary one.
bindings:
	rm -rf $(TMP_DIR) $(PY_DIR)/wit_world $(PY_DIR)/componentize_py_types.py
	componentize-py -d $(WIT_DIR) -w plugin-rules bindings $(TMP_DIR)
	cp -R $(TMP_DIR)/wit_world $(PY_DIR)/
	cp $(TMP_DIR)/componentize_py_types.py $(PY_DIR)/
	# PEP 561 marker: without it type checkers ignore the generated annotations
	touch $(PY_DIR)/wit_world/py.typed
	rm -rf $(TMP_DIR)

install: bindings copy-wit copy-testkit
	python -m pip install .

# Editable install for SDK development
develop: bindings copy-wit copy-testkit
	python -m pip install -e .

test:
	pytest

clean:
	rm -rf $(TMP_DIR) $(PY_DIR)/wit_world $(PY_DIR)/componentize_py_types.py \
		$(PKG_DIR)/wit $(PKG_DIR)/wasm dist __pycache__
