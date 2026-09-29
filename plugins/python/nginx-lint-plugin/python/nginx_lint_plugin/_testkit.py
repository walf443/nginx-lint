"""The parser and the fix applier, run as core wasm modules under wasmtime.

These are the Go SDK's test-helper modules (``plugins/go/nginx-lint-plugin/
nginxlinttest/{parser,fixer}.wasm``): ``nginx-lint-parser`` and
``nginx-lint-common`` built with ``--features wasm-json``, committed there and
copied into this package by ``make copy-testkit``. They have no imports and
no canonical ABI — one JSON string in, one out — so a plain wasm runtime
runs them, and wasmtime-py, which has no component-model support, is enough.

Only ``testing`` imports this module. wasmtime is a native extension, so a
module a plugin imports must never reach it: componentize-py executes the
plugin's imports at build time and would fail to load it.
"""

import functools
from pathlib import Path

import wasmtime

_WASM_DIR = Path(__file__).parent / "wasm"


@functools.cache
def _engine() -> wasmtime.Engine:
    return wasmtime.Engine()


@functools.cache
def _module(name: str) -> wasmtime.Module:
    # Compiling is the expensive part and the modules are stateless between
    # calls, so each is compiled once per process and instantiated per call.
    path = _WASM_DIR / f"{name}.wasm"
    if not path.is_file():
        raise RuntimeError(
            f"{path} is missing; inside the repository run `make install` "
            "(or `make copy-testkit`) in plugins/python/nginx-lint-plugin"
        )
    return wasmtime.Module.from_file(_engine(), str(path))


def call(module: str, export: str, *args: bytes) -> bytes:
    """Run one JSON entry point of the ``parser`` or ``fixer`` module.

    Each argument is written into the module's memory and passed as a
    (pointer, length) pair; the export returns its result packed into one
    u64, ``(ptr << 32) | len``.
    """
    # A fresh instance per call, so one parse cannot see what a previous one
    # left in the module's memory.
    store = wasmtime.Store(_engine())
    instance = wasmtime.Instance(store, _module(module), [])
    exports = instance.exports(store)
    memory = exports["memory"]
    alloc = exports["alloc"]
    entry = exports[export]
    assert isinstance(memory, wasmtime.Memory)
    assert isinstance(alloc, wasmtime.Func)
    assert isinstance(entry, wasmtime.Func)

    params: list[int] = []
    for arg in args:
        if not arg:
            params += [0, 0]
            continue
        ptr = alloc(store, len(arg))
        memory.write(store, arg, ptr)
        params += [ptr, len(arg)]

    return _read(store, memory, entry(store, *params))


def version(module: str) -> str:
    """The version of the crate the module was built from."""
    store = wasmtime.Store(_engine())
    instance = wasmtime.Instance(store, _module(module), [])
    exports = instance.exports(store)
    memory = exports["memory"]
    entry = exports["version"]
    assert isinstance(memory, wasmtime.Memory)
    assert isinstance(entry, wasmtime.Func)
    return _read(store, memory, entry(store)).decode()


def _read(store: wasmtime.Store, memory: wasmtime.Memory, packed: int) -> bytes:
    # An i64 comes back signed; the pair is two unsigned 32-bit halves.
    packed &= 0xFFFF_FFFF_FFFF_FFFF
    ptr, length = packed >> 32, packed & 0xFFFF_FFFF
    return memory.read(store, ptr, ptr + length)
