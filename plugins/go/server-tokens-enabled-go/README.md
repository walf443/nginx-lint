# server-tokens-enabled-go

The Go counterpart of `plugins/builtin/security/server_tokens_enabled`: it
reports every `server_tokens on`, which leaks the exact nginx version in
response headers. It exists to show what writing an nginx-lint plugin in Go
involves, and to keep that path building.

It is also the example for the Go SDK in `../nginx-lint-plugin`, which it uses
through a `replace` to the working tree.

## Prerequisites

- Go 1.25+ (the standard toolchain; TinyGo is not involved)
- [componentize-go](https://github.com/bytecodealliance/componentize-go) 0.4.0:

  ```bash
  go install github.com/bytecodealliance/componentize-go@v0.4.0
  ```

## Build and run

```bash
make build      # componentize, no WIT path or world needed
make check      # vet + unit tests, on the host, no wasm
make test-e2e   # needs ../../../target/debug/nginx-lint
```

The component this produces needs `--allow-wasi-plugins`:

```bash
nginx-lint --plugins . --allow-wasi-plugins examples/bad.conf
```

componentize-go builds a wasip1 module and adapts it, so the Go runtime
imports `wasi:cli/*`, `wasi:clocks/*`, `wasi:filesystem/*`, `wasi:io/*` and
`wasi:random/random` even though this plugin only walks the configuration it
is handed. The linter links no WASI by default, so without that flag the
plugin does not load at all. See the sandbox section in the root README for
what the flag does and does not grant.

## Layout

Three files, one package:

- `main.go` — the rule. It implements the SDK's `Plugin` interface, registers
  itself from `init`, and blank imports the SDK's `export` package so the
  component's exports get linked.
- `main_test.go` — the rule's tests, run against the real parser.
- `examples/` — the bad and good configurations, embedded with `go:embed` so
  `nginx-lint why` can render them.

There is no generated code here and no WIT path in the Makefile: the SDK
carries the bindings and a `componentize-go.toml`, so `componentize-go build`
finds the world by scanning this module's dependencies.

## Testing

`make check` runs on the host with no wasm toolchain and no component runtime.
The tests use the SDK's `nginxlinttest`, which runs the **real parser** and the
**real fix applier** — the same two crates the CLI uses, compiled as core wasm
modules and run under wazero, which is pure Go:

```go
runner().AssertErrorOnLine(t, source, 2)
runner().AssertFixProduces(t,
	"http {\n    server_tokens on;\n}\n",
	"http {\n    server_tokens off;\n}\n")
runner().AssertExamplesWithFix(t, badExample, goodExample)
```

So a test that passes is a test against the positions and offsets the linter
will actually produce, not against a fixture someone typed. One test here
still builds a `Config` by hand, to show that style works for a shape the
parser cannot easily produce.

`make test-e2e` covers what remains: the generated bindings, handle ownership,
the WASI-less default, and the host boundary. It runs the built component
through the CLI against `examples/`, and applies `--fix` to `examples/bad.conf`
and diffs the result against `examples/good.conf`.
