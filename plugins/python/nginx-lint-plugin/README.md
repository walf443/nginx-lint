# nginx-lint-plugin (Python SDK)

Python SDK for writing and testing nginx-lint WASM plugins. The Python
counterpart of the TypeScript SDK at `plugins/typescript/nginx-lint-plugin/`.

## What's in here

- `nginx_lint_plugin/` — the SDK package
  - `testing` — `parse_config()` and `PluginTestRunner` for plain-pytest
    unit tests against the real Rust parser
  - `config_builder` — reconstructs method-based `Config`/`Directive`
    objects (matching the componentize-py binding surface, e.g.
    `directive.is_(...)`) from parser output or a host snapshot
  - `API_VERSION` — the plugin API version, kept in sync with
    `crates/nginx-lint-plugin`
- `wit_world/`, `componentize_py_types.py` — committed componentize-py
  bindings generated from `wit/nginx-lint-plugin.wit` (the Python analog of
  the TS SDK's `dist/generated`). Regenerate after WIT changes with
  `componentize-py -d ../../../wit -w plugin bindings .` — this committed
  copy is what pytest imports; a plugin's local `make bindings` output is
  IDE-only and must be regenerated together with it, or the two diverge.

## Testing a plugin

Tests are ordinary pytest. Parsing goes through the same Rust parser the
production linter uses, via the `nginx_lint_parser_py` native module:

```bash
# one-time setup (from the repo root)
pip install maturin pytest
cd crates/nginx-lint-parser-py && maturin develop

# then, in a plugin directory (see server-tokens-enabled-py/conftest.py
# for the sys.path setup)
pytest
```

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
removed its `bindgen` support — so this SDK uses a pyo3 native extension
(`crates/nginx-lint-parser-py`) instead. Same parser code, same output
shape. If wasmtime-py regrows component support, the WASM path can return
without changing the test-writing API.
