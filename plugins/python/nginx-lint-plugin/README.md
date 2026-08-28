# nginx-lint-plugin (Python SDK)

Python SDK for writing and testing nginx-lint WASM plugins. The Python
counterpart of the TypeScript SDK at `plugins/typescript/nginx-lint-plugin`,
built as a single maturin mixed Rust/Python project: one wheel ships the
pure-Python SDK, the componentize-py bindings, and the Rust parser compiled
as a native module.

## Layout

- `python/nginx_lint_plugin/` — the SDK package. Everything a plugin needs
  is re-exported from its root (`Plugin`, `Config`, `LintError`, `Fix`,
  `Severity`, …), so plugin code never imports from the generated bindings
  directly. It ships a PEP 561 `py.typed` marker, so mypy and pyright use
  the annotations.
  - `builders` — `plugin_spec()` and `error_builder()`, mirroring the Rust
    SDK's `PluginSpec::new()` and `spec().error_builder()`. The generated
    dataclasses have no defaults, so without these a spec means spelling out
    all eleven fields and every error repeats the plugin's rule and category.
  - `testing` — `parse_config()` and `PluginTestRunner` for plain-pytest
    unit tests against the real Rust parser
  - `config_builder` — reconstructs method-based `Config`/`Directive`
    objects (matching the componentize-py binding surface, e.g.
    `directive.is_(...)`) from parser output or a host snapshot
  - `_native` — the Rust parser bridge (built by maturin from
    `src/lib.rs`; only `testing` imports it, so the rest of the SDK stays
    bundleable into a WASM component). Built against the stable ABI
    (`abi3-py311`), so one wheel per platform covers every Python ≥ 3.11.
  - `API_VERSION` — the plugin API version, kept in sync with
    `crates/nginx-lint-plugin`
- `python/wit_world/`, `python/componentize_py_types.py` — componentize-py
  bindings generated from `wit/nginx-lint-plugin.wit` by `make bindings`,
  placed as top-level modules so the same imports resolve inside a
  componentized plugin (the Python analog of the TS SDK's `dist/generated`).
  Like the TS SDK's, they are **not committed** — regenerated before every
  install, so they cannot drift from the WIT. They still ship in the wheel
  and sdist via `tool.maturin.include`, which applies regardless of
  `.gitignore`.
- `Cargo.toml` / `src/lib.rs` — the native parser module (its own cargo
  workspace, excluded from the repository's root workspace; the crate
  version is the wheel version, kept in sync with the repository version)

## Install

Both targets regenerate the bindings and copy the WIT in first, so
`componentize-py` must be on `PATH`:

```bash
cd plugins/python/nginx-lint-plugin

make install   # pip install .
make develop   # editable install for SDK work (maturin develop)
```

Outside this repository the SDK is a plain dependency — `pip install
nginx-lint-plugin` is all a plugin author needs, for both testing and
building. The WIT ships inside the package, so componentize-py has an
interface definition to build against:

```bash
componentize-py -d "$(python -c 'import nginx_lint_plugin as p; print(p.wit_dir())')" \
    -w plugin componentize app -o plugin.wasm --stub-wasi \
    -p . -p "$(python -c 'import nginx_lint_plugin as p, pathlib; print(pathlib.Path(p.__file__).parent.parent)')"
```

The second `-p` is only needed for editable installs, whose `.pth` link
componentize-py does not follow; it is harmless otherwise. See
`../server-tokens-enabled-py/Makefile` for the same commands in a form you
can copy.

## Writing a plugin

```python
from nginx_lint_plugin import Config, LintError, Plugin, plugin_spec, error_builder


class WitWorld(Plugin):          # the class must keep this name
    def spec(self):
        return plugin_spec("my-rule", "style", "What it checks",
                           severity="warning")

    def check(self, cfg: Config, path: str) -> list[LintError]:
        err = error_builder(self.spec())
        return [
            err.warning_at("autoindex should be off", ctx.directive,
                           fixes=[ctx.directive.replace_with("autoindex off;")])
            for ctx in cfg.all_directives_with_context()
            if ctx.directive.is_("autoindex") and ctx.directive.first_arg_is("on")
        ]
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
