# nginx-lint-plugin (Python SDK)

Python SDK for writing and testing nginx-lint WASM plugins. The Python
counterpart of the TypeScript SDK at `plugins/typescript/nginx-lint-plugin`,
shipped as one pure-Python wheel: the SDK, the componentize-py bindings,
and nginx-lint's parser and fix applier compiled to wasm, which the testing
utilities run with wasmtime.

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
    unit tests against nginx-lint's own parser, plus `apply_fixes()` and
    `PluginTestRunner.assert_fixed()` to check what a rule's fixes actually
    produce, through the same applier `nginx-lint --fix` uses
  - `config_builder` — reconstructs method-based `Config`/`Directive`
    objects (matching the componentize-py binding surface, e.g.
    `directive.is_(...)`) from parser output or a host snapshot
  - `_testkit` — runs `wasm/parser.wasm` and `wasm/fixer.wasm` under
    wasmtime. Only `testing` imports it: wasmtime is a native extension,
    which a WASM component cannot load, so the rest of the SDK stays
    bundleable.
  - `wasm/` — `nginx-lint-parser` and `nginx-lint-common` built as core wasm
    modules with a JSON entry point. They are the Go SDK's test-helper
    modules, committed under `plugins/go/nginx-lint-plugin/nginxlinttest/`
    and copied here by `make copy-testkit`; not committed here, but shipped
    in the wheel and sdist.
  - `API_VERSION` — the plugin API version, kept in sync with
    `crates/nginx-lint-plugin`
- `python/wit_world/`, `python/componentize_py_types.py` — componentize-py
  bindings generated from `wit/nginx-lint-plugin.wit` by `make bindings`,
  placed as top-level modules so the same imports resolve inside a
  componentized plugin (the Python analog of the TS SDK's `dist/generated`).
  Like the TS SDK's, they are **not committed** — regenerated before every
  install, so they cannot drift from the WIT. They still ship in the wheel
  and sdist via hatchling's `artifacts`, which applies regardless of
  `.gitignore`.

## Install

Both targets regenerate the bindings and copy the WIT and the wasm modules
in first. Regenerating needs the `componentize-py` CLI, which is in the `dev`
dependency group:

```bash
cd plugins/python/nginx-lint-plugin
pip install --group dev

make install   # pip install .
make develop   # editable install for SDK work
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

Tests are ordinary pytest. Parsing goes through the same parser the
production linter uses, compiled to wasm:

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

## Why core wasm modules instead of the parser WASM component?

The TS SDK runs the parser as a WASM component inside Node (via jco).
wasmtime-py can run components too (`wasmtime.component`, since 39.0.0), but
the parser and fixer components are not committed anywhere, so using them
would mean building them in this package's release or keeping a second
committed copy with its own freshness check. The Go SDK already commits the
same two crates as core modules with a JSON entry point, kept current by
`make check-testkit-wasm`, so this SDK runs that pair instead. Same parser
code, same output shape, and no compiled code of its own: the wheel is
`py3-none-any` and runs wherever wasmtime-py has a wheel.

## Checking the built plugin

The CLI can check a built plugin end to end from the examples in its
spec — the bad one has to be reported, the good one clean, and the fixes
have to resolve the bad one:

```bash
nginx-lint test-plugins --plugins <dir>
```
