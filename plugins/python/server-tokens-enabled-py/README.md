# server-tokens-enabled-py

A Python implementation of the `server-tokens-enabled` rule, built as a WASM
component with [componentize-py](https://github.com/bytecodealliance/componentize-py).
This is a proof of concept that nginx-lint plugins can be written in Python,
mirroring the TypeScript example at `plugins/typescript/server-tokens-enabled-ts/`.

## Requirements

- Python 3.11+ (the generated bindings use `typing.Self`)
- The `nginx-lint-plugin` SDK (`pip install nginx-lint-plugin`, or
  `make -C ../nginx-lint-plugin install` in this repository). Nothing else
  here depends on the monorepo: the Makefile takes both the WIT and the SDK
  path from the installed package, so this directory works as a template for
  a plugin developed outside the repository.
- componentize-py 0.25+ (`uv tool install componentize-py` or `pip install componentize-py`)

## Build

```bash
make build
# → server-tokens-enabled-py.wasm (~18MB; embeds CPython)
```

`--stub-wasi` replaces all WASI imports with trapping stubs, so the component
needs no WASI support from the host — the same zero-capability posture as the
TypeScript plugins built with `jco componentize --disable all`. Note that this
bakes the build-time PRNG seed into the component, so Python's `random` module
is deterministic; lint rules should not need randomness.

## Run

```bash
cd ../../..
cargo run --features plugins -- \
    --plugins plugins/python/server-tokens-enabled-py \
    plugins/python/server-tokens-enabled-py/examples/bad.conf
```

## Test

Unit tests are plain pytest (`test_plugin.py`, a 1:1 port of the TS plugin's
`plugin.test.ts`), using the SDK at `../nginx-lint-plugin`:

```bash
# one-time setup: dev tools from pyproject.toml, then the SDK itself
# (from the sibling directory, since it is not on PyPI yet)
pip install --group dev
make -C ../nginx-lint-plugin install

make test        # pytest
make typecheck   # mypy, against the SDK's shipped type annotations
make test-e2e    # componentize + run through the nginx-lint CLI
```

`pyproject.toml` declares the dependencies and points pytest at this
directory, so `app.py` is importable from the tests without a `sys.path`
shim. There is no `[build-system]`: a plugin is compiled to a WASM
component, not installed as a Python distribution.

The same `WitWorld` class in `app.py` runs unmodified under pytest and
inside the WASM component.

## Development

The plugin implements the `plugin` world from `wit/nginx-lint-plugin.wit`:
a `WitWorld` class with `spec()` and `check(cfg, path)`. The typed bindings
(`wit_world`) come from the SDK, so no per-plugin binding generation is
needed.

`cfg` is a host-backed resource: every method call crosses the component
boundary, so `check()` starts by pulling the directives it cares about over
in one call (`cfg.snapshot_filtered(RELEVANT_DIRECTIVES)`) and rebuilding a
Config guest-side with `build_config_from_snapshot()` — the same approach the
TypeScript plugin takes. Walking `cfg.all_directives_with_context()` on the
host resource directly would instead cost one host call per directive in the
file.

## Known trade-offs vs TypeScript plugins

- Binary size: ~18MB (embedded CPython) vs ~10MB (StarlingMonkey JS engine).
- Cold load compiles the component (mitigated by the plugin compilation
  cache; warm loads are fast).
- No `pip` dependencies are bundled by default; pure-Python deps can be added
  via `componentize-py -p <dir>`.
