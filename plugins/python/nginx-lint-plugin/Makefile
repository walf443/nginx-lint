WIT_DIR := ../../../wit
PY_DIR := python
PKG_DIR := $(PY_DIR)/nginx_lint_plugin
TMP_DIR := .bindings-tmp

.PHONY: bindings copy-wit install develop test clean

# Ship the WIT inside the package (like the TS SDK's `files: [..., "wit"]`)
# so plugin authors who only `pip install nginx-lint-plugin` can still run
# componentize-py, which requires the interface definition. The copy is
# generated, not committed; nginx_lint_plugin.wit_dir() resolves it.
copy-wit:
	mkdir -p $(PKG_DIR)/wit
	cp $(WIT_DIR)/nginx-lint-plugin.wit $(PKG_DIR)/wit/

# Generate the componentize-py bindings from the WIT. They are not committed
# (same convention as the TS SDK, which regenerates on every npm run build),
# so this must run before installing or testing from a fresh checkout.
# componentize-py refuses to write into an existing directory, hence the
# temporary one.
bindings:
	rm -rf $(TMP_DIR) $(PY_DIR)/wit_world $(PY_DIR)/componentize_py_types.py
	componentize-py -d $(WIT_DIR) -w plugin bindings $(TMP_DIR)
	cp -R $(TMP_DIR)/wit_world $(PY_DIR)/
	cp $(TMP_DIR)/componentize_py_types.py $(PY_DIR)/
	# PEP 561 marker: without it type checkers ignore the generated annotations
	touch $(PY_DIR)/wit_world/py.typed
	rm -rf $(TMP_DIR)

install: bindings copy-wit
	python -m pip install .

# Editable install for SDK development (rebuilds the native module)
develop: bindings copy-wit
	maturin develop

test:
	pytest

clean:
	rm -rf $(TMP_DIR) $(PY_DIR)/wit_world $(PY_DIR)/componentize_py_types.py \
		$(PKG_DIR)/wit target dist __pycache__
