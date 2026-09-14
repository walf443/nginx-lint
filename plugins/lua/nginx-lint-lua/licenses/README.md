# Third-party notices

Verbatim license texts of the code compiled into `runtime/runtime.core.wasm`,
which is embedded in the `nginx-lint-lua` binary and in every plugin it
builds. `nginx-lint-lua license` prints them. Sources:

| Directory | Project | Revision |
|---|---|---|
| `lua/` | Lua 5.4.8, `src/lua.h` copyright notice | lua-5.4.8 |
| `wasi-libc/` | wasi-libc (`LICENSE`, `LICENSE-MIT`, `libc-top-half/musl/COPYRIGHT`, `libc-bottom-half/cloudlibc/LICENSE`) | 2e6fb9d8ee0c (wasi-sdk 34) |
| `llvm/` | LLVM compiler-rt builtins, `compiler-rt/LICENSE.TXT` | 895aa2c896ad (wasi-sdk 34) |

wasi-libc is multi-licensed; it is used here under its MIT option, so only
that text is included alongside the overview `LICENSE`. Its dlmalloc is
CC0 and carries no notice. Refresh these when `runtime/Makefile` moves to a
new wasi-sdk or Lua release (the revisions are in the wasi-sdk `VERSION`
file).
