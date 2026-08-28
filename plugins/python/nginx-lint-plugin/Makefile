WIT_DIR := ../../../wit
PY_DIR := python
TMP_DIR := .bindings-tmp

.PHONY: bindings install develop test clean

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
	rm -rf $(TMP_DIR)

install: bindings
	python -m pip install .

# Editable install for SDK development (rebuilds the native module)
develop: bindings
	maturin develop

test:
	pytest

clean:
	rm -rf $(TMP_DIR) $(PY_DIR)/wit_world $(PY_DIR)/componentize_py_types.py \
		target dist __pycache__
