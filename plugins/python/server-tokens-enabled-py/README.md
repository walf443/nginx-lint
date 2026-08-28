# server-tokens-enabled-py

A Python implementation of the `server-tokens-enabled` rule, built as a WASM
component with [componentize-py](https://github.com/bytecodealliance/componentize-py).
This is a proof of concept that nginx-lint plugins can be written in Python,
mirroring the TypeScript example at `plugins/typescript/server-tokens-enabled-ts/`.

## Requirements

- Python 3.11+ (the generated bindings use `typing.Self`)
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
# one-time setup: install the SDK (includes the native parser module)
pip install pytest ../nginx-lint-plugin

make test        # pytest
make test-e2e    # componentize + run through the nginx-lint CLI
```

The same `WitWorld` class in `app.py` runs unmodified under pytest and
inside the WASM component.

## Development

`make bindings` generates typed Python bindings under `bindings/` for IDE
completion and type checking. They are not needed for the build itself —
componentize-py regenerates bindings internally — and are gitignored.

The plugin implements the `plugin` world from `wit/nginx-lint-plugin.wit`:
a `WitWorld` class with `spec()` and `check(cfg, path)`. `cfg` is a host-backed
resource; each method call (`all_directives_with_context()`, `directive.line()`,
…) crosses the component boundary. For large configs, the snapshot API
(`cfg.snapshot_filtered([...])` + guest-side reconstruction, as done by the
TypeScript SDK's `buildConfigFromSnapshot`) would reduce host calls; this
prototype keeps the simple per-call style.

## Known trade-offs vs TypeScript plugins

- Binary size: ~18MB (embedded CPython) vs ~10MB (StarlingMonkey JS engine).
- Cold load compiles the component (mitigated by the plugin compilation
  cache; warm loads are fast).
- No `pip` dependencies are bundled by default; pure-Python deps can be added
  via `componentize-py -p <dir>`.
