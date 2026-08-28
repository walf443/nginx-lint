# nginx-lint-plugin (Python SDK)

Python SDK for writing and testing nginx-lint WASM plugins. The Python
counterpart of the TypeScript SDK at `plugins/typescript/nginx-lint-plugin`,
built as a single maturin mixed Rust/Python project: one wheel ships the
pure-Python SDK, the componentize-py bindings, and the Rust parser compiled
as a native module.

## Layout

- `python/nginx_lint_plugin/` — the SDK package
  - `testing` — `parse_config()` and `PluginTestRunner` for plain-pytest
    unit tests against the real Rust parser
  - `config_builder` — reconstructs method-based `Config`/`Directive`
    objects (matching the componentize-py binding surface, e.g.
    `directive.is_(...)`) from parser output or a host snapshot
  - `_native` — the Rust parser bridge (built by maturin from
    `src/lib.rs`; only `testing` imports it, so the rest of the SDK stays
    bundleable into a WASM component)
  - `API_VERSION` — the plugin API version, kept in sync with
    `crates/nginx-lint-plugin`
- `python/wit_world/`, `python/componentize_py_types.py` — committed
  componentize-py bindings generated from `wit/nginx-lint-plugin.wit`, shipped
  as top-level modules so the same imports resolve inside a componentized
  plugin (the Python analog of the TS SDK's `dist/generated`). Both pytest and
  the componentized plugin import these, so they must be regenerated whenever
  the WIT changes; CI enforces it (the `python-plugin` job diffs them against
  freshly generated bindings). componentize-py refuses to write into an
  existing directory, so regenerate via a temporary one:

  ```bash
  # from the repository root
  rm -rf /tmp/py-bindings && componentize-py -d wit -w plugin bindings /tmp/py-bindings
  sdk=plugins/python/nginx-lint-plugin/python
  rm -rf "$sdk/wit_world" && cp -R /tmp/py-bindings/wit_world "$sdk/"
  cp /tmp/py-bindings/componentize_py_types.py "$sdk/"
  ```
- `Cargo.toml` / `src/lib.rs` — the native parser module (its own cargo
  workspace, excluded from the repository's root workspace; the crate
  version is the wheel version, kept in sync with the repository version)

## Install

```bash
# from the repository
pip install ./plugins/python/nginx-lint-plugin

# for SDK development (editable, rebuilds the native module)
cd plugins/python/nginx-lint-plugin && maturin develop
```

## Testing a plugin

Tests are ordinary pytest. Parsing goes through the same Rust parser the
production linter uses:

```python
from app import WitWorld
from nginx_lint_plugin.testing import PluginTestRunner, parse_config

plugin = WitWorld()
runner = PluginTestRunner(plugin.spec, plugin.check)

def test_detects_server_tokens_on():
    runner.assert_errors("http {\n    server_tokens on;\n}", 1)

def test_include_context():
    cfg = parse_config("server_tokens on;", include_context=["http"])
    assert len(plugin.check(cfg, "test.conf")) == 1
```

The same `WitWorld` class runs unmodified under pytest and inside the WASM
component — the test Config/Directive objects reproduce the exact method
surface of the componentize-py bindings.

## Why a native module instead of the parser WASM component?

The TS SDK runs the parser as a WASM component inside Node (via jco).
Python currently has no maintained component-model runtime — wasmtime-py
removed its `bindgen` support — so this SDK compiles the parser natively
via pyo3 instead. Same parser code, same output shape. If wasmtime-py
regrows component support, the WASM path can return without changing the
test-writing API.
