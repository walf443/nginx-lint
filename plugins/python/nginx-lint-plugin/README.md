# nginx-lint-plugin (Python SDK)

Python SDK for writing and testing nginx-lint WASM plugins. The Python
counterpart of the TypeScript SDK at `plugins/typescript/nginx-lint-plugin`,
built as a single maturin mixed Rust/Python project: one wheel ships the
pure-Python SDK, the componentize-py bindings, and the Rust parser compiled
as a native module.

## Layout

- `python/nginx_lint_plugin/` — the SDK package. Everything a plugin needs
  is re-exported from its root (`Rule`, `define_rules`, `Config`,
  `LintError`, `Fix`, `Severity`, …), so plugin code never imports from the
  generated bindings directly. It ships a PEP 561 `py.typed` marker, so mypy
  and pyright use the annotations.
  - `rules` — `Rule`, the base class of a lint rule, and `define_rules()`,
    which builds the `plugin-rules` world class for one or more of them.
  - `builders` — `plugin_spec()` and `error_builder()`, mirroring the Rust
    SDK's `PluginSpec::new()` and `spec().error_builder()`. The generated
    dataclasses have no defaults, so without these a spec means spelling out
    all eleven fields and every error repeats the plugin's rule and category.
  - `testing` — `parse_config()` and `PluginTestRunner` for plain-pytest
    unit tests against the real Rust parser, plus `apply_fixes()` and
    `PluginTestRunner.assert_fixed()` to check what a rule's fixes actually
    produce, through the same applier `nginx-lint --fix` uses
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
  version is the wheel version, kept in sync with the repository version).
  It depends on `nginx-lint-parser` and `nginx-lint-common` by exact
  version rather than by path so that the sdist stands alone; inside this
  repository `../.cargo/config.toml` patches both back to the working tree,
  and that file explains why.

## Install

Both targets regenerate the bindings and copy the WIT in first, which needs
the `componentize-py` CLI; `make develop` also needs `maturin`. Both are in
the `dev` dependency group:

```bash
cd plugins/python/nginx-lint-plugin
pip install --group dev

make install   # pip install .
make develop   # editable install for SDK work (maturin develop)
make test      # pytest
```

Outside this repository the SDK is a plain dependency — `pip install
nginx-lint-plugin` is all a plugin author needs, for both testing and
building. The WIT ships inside the package, so componentize-py has an
interface definition to build against:

```bash
componentize-py -d "$(python -c 'import nginx_lint_plugin as p; print(p.wit_dir())')" \
    -w plugin-rules componentize app -o plugin.wasm --stub-wasi \
    -p . -p "$(python -c 'import nginx_lint_plugin as p, pathlib; print(pathlib.Path(p.__file__).parent.parent)')"
```

The second `-p` is only needed for editable installs, whose `.pth` link
componentize-py does not follow; it is harmless otherwise. See
`../server-tokens-enabled-py/Makefile` for the same commands in a form you
can copy.

## Writing a plugin

A plugin is one or more rules. Each rule is a `Rule` subclass: its metadata
(`spec`), the directive names it reads (`relevant_directives`), and a
`check`. `define_rules` builds the world class the host calls, which
componentize-py looks up as `WitWorld`:

```python
from nginx_lint_plugin import (
    LintError, ReconstructedConfig, Rule, define_rules, error_builder, plugin_spec,
)


class NoAutoindex(Rule):
    spec = plugin_spec("no-autoindex", "style", "What it checks", severity="warning")
    relevant_directives = ["autoindex"]      # None: the whole config

    def check(self, cfg: ReconstructedConfig, path: str) -> list[LintError]:
        err = error_builder(self.spec)
        return [
            err.warning_at("autoindex should be off", ctx.directive,
                           fixes=[ctx.directive.replace_with("autoindex off;")])
            for ctx in cfg.all_directives_with_context()
            if ctx.directive.is_("autoindex") and ctx.directive.first_arg_is("on")
        ]


# A plugin with several rules lists them all here; the host loads each as
# its own rule, with its own name, documentation and configuration.
WitWorld = define_rules(NoAutoindex())
```

`check` gets the config already fetched from the host and rebuilt. With
`relevant_directives` set, it is pruned to those directives plus the ancestor
blocks needed for `parent_stack` and include-context checks — one host call,
proportional to what is relevant rather than to the file. A rule that warns
when a directive is *missing* inside a block has to list that block's name
too (`"http"` beside `"server_tokens"`, say): with none of the listed names
inside it, the block is pruned away with the evidence. Leave it `None` to
get the whole config, which is also the only way to see comments and blank
lines. When the host asks for several rules of one component at once, the
config is fetched once, pruned to the union of their lists — so the list is
a floor, not a ceiling: match by name, and do not read anything into a block
being empty.

`define_rules` raises `ValueError` on an empty list, a rule without a name,
two rules with one name, or an empty `relevant_directives`.

The component targets the `plugin-rules` world, which the host loads from
the same release of nginx-lint as this SDK onwards (the two share a version
number). A component built with this SDK does not load on an older
nginx-lint.

## Testing a plugin

Tests are ordinary pytest. Parsing goes through the same Rust parser the
production linter uses:

```python
from app import NoAutoindex, WitWorld
from nginx_lint_plugin.testing import PluginTestRunner, parse_config

# The runner hands the rule its config the way the host does: pruned to
# its relevant_directives when it declares them
runner = PluginTestRunner(NoAutoindex())

def test_detects_autoindex_on():
    runner.assert_errors("http {\n    autoindex on;\n}", 1)

def test_include_context():
    # The world's check, as the host calls it: the rules asked for by name
    cfg = parse_config("autoindex on;", include_context=["http"])
    assert len(WitWorld().check(cfg, "test.conf", ["no-autoindex"])) == 1

def test_fix():
    # Asserting on the applied output, not just the reported findings: a fix
    # the linter normalizes into a different operation than intended shows up
    # here rather than in a user's config.
    runner.assert_fixed(
        "http {\n    autoindex on;\n}\n",
        "http {\n    autoindex off;\n}\n",
    )
```

The same `Rule` classes run unmodified under pytest and inside the WASM
component — the test Config/Directive objects reproduce the exact method
surface of the componentize-py bindings.

## Why a native module instead of the parser WASM component?

The TS SDK runs the parser as a WASM component inside Node (via jco).
Python currently has no maintained component-model runtime — wasmtime-py
removed its `bindgen` support — so this SDK compiles the parser natively
via pyo3 instead. Same parser code, same output shape. If wasmtime-py
regrows component support, the WASM path can return without changing the
test-writing API.

## Checking the built plugin

The CLI can check a built plugin end to end from the examples in its
spec — the bad one has to be reported, the good one clean, and the fixes
have to resolve the bad one:

```bash
nginx-lint test-plugins --plugins <dir>
```
