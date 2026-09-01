# nginx-lint-plugin (Go)

Go SDK for writing [nginx-lint](https://github.com/walf443/nginx-lint) plugins.

Plugins are compiled to WebAssembly Component Model binaries and loaded by the
nginx-lint CLI at runtime.

## Install

```bash
go get github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin
go install github.com/bytecodealliance/componentize-go@v0.4.0
```

Requires Go 1.25+. The standard toolchain — TinyGo is not involved.

This is a nested module, so its releases are tagged
`plugins/go/nginx-lint-plugin/vX.Y.Z` — the repository's own `vX.Y.Z` tag does
not version it. The release workflow creates that tag alongside each release;
until the first one, `go get` resolves to a pseudo-version of the default
branch, which works the same.

The version tracks nginx-lint's, so an SDK release always matches the WIT and
the host it was built against. One consequence to know before nginx-lint
reaches 2.0.0: Go requires a major version from v2 on to appear in the module
path itself, so the import path will have to gain a `/v2` at that point.

## Quick start

A plugin implements `Spec` and `Check`, registers itself from `init`, and blank
imports the `export` package so the component's exports get linked.

```go
package main

import (
	_ "embed"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
	_ "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/export"
)

//go:embed examples/bad.conf
var badExample string

//go:embed examples/good.conf
var goodExample string

type myRule struct{}

func (myRule) Spec() nginxlint.Spec {
	return nginxlint.Spec{
		Name:        "my-rule",
		Category:    "best-practices",
		Description: "Describe what this rule checks",
		Severity:    "warning",
		BadExample:  badExample,
		GoodExample: goodExample,
	}
}

func (myRule) Check(cfg nginxlint.Config) []nginxlint.LintError {
	errors := []nginxlint.LintError{}
	cfg.Named("proxy_pass", func(directive nginxlint.Directive) {
		if directive.IsInside("location") && !directive.HasBlock {
			errors = append(errors, nginxlint.Warning(directive, "…").
				WithFix(directive.InsertAfter("proxy_set_header Host $host;")))
		}
	})
	return errors
}

func init() { nginxlint.Register(myRule{}) }

// The host calls the exports directly; main never runs.
func main() {}
```

Build it:

```bash
componentize-go build -o my-rule.wasm
```

No `-d` and no `-w`: this module ships a `componentize-go.toml` naming the WIT
and the world, and componentize-go finds it by scanning your dependencies.

Then run it. `--allow-wasi-plugins` is required for every Go plugin — see
[the plugin sandbox](../../../README.md#the-plugin-sandbox) for why, and for
what the flag does and does not grant:

```bash
nginx-lint --plugins . --allow-wasi-plugins nginx.conf
```

## What a check receives

`Config` is plain Go data. The SDK takes one snapshot of the configuration
when a check starts, so a rule never holds a handle on the host:

```go
type Config struct {
	Path           string
	Directives     []Directive   // top level; each carries its own Block
	Comments       []Comment     // flat, from the whole file
	BlankLines     []BlankLine
	IncludeContext []string
}
```

`Config.All(visit)` walks every directive at any depth in source order, and
`Config.Named(name, visit)` walks the ones with a given name.

Each `Directive` carries its name, arguments, position, byte offsets and the
surrounding whitespace, plus `Parents` — the blocks enclosing it, outermost
first. `Parents` starts from the include context, so `directive.IsInside("http")`
answers the same whether the directive was written inline or pulled in by an
`include`.

Fixes are built from the directive: `ReplaceWith`, `DeleteLine`, `InsertBefore`,
`InsertAfter`, `InsertBeforeMany`, `InsertAfterMany`. They compute the same byte
ranges the host would have, so the rule stays testable without a component
runtime.

## Testing

`nginxlinttest` runs a plugin against the real parser and the real fix
applier, from a plain `go test`:

```go
func TestRule(t *testing.T) {
	runner := nginxlinttest.New(myRule{})

	runner.AssertErrors(t, "http {\n    server_tokens on;\n}\n", 1)
	runner.AssertErrorOnLine(t, "http {\n    server_tokens on;\n}\n", 2)
	runner.AssertNoErrors(t, "http {\n    server_tokens off;\n}\n")

	// Applied by the linter's own applier, so this fails for the same reasons
	// `--fix` would: a bad offset, a range that splits a character, two fixes
	// that overlap.
	runner.AssertFixProduces(t,
		"http {\n    server_tokens on;\n}\n",
		"http {\n    server_tokens off;\n}\n")

	// The pair `nginx-lint why` renders: the bad one has to fire, the good
	// one has to be clean, and --fix has to turn one into the other.
	runner.AssertExamplesWithFix(t, badExample, goodExample)
}
```

`Check(source, includeContext...)` returns the findings if you would rather
assert on them yourself, and `nginxlinttest.Parse` returns the `Config` a check
would receive. The variadic include context is `--context` on the command
line: `runner.Check("server_tokens on;\n", "http", "server")` tests a fragment
as though it were included from inside a `server` block.

The other SDKs' test runners call the linter's parser in process. Go has no
component-model runtime, so this one runs the same two crates compiled as core
wasm modules — no imports, one JSON entry point each — under
[wazero](https://wazero.io), which is pure Go. No cgo, no toolchain beyond Go
itself. The modules are committed under `nginxlinttest/`, and what a rule sees
through them is converted by the same code the component adapter uses, so a
test and a real run cannot disagree about the shape of a configuration.

Struct literals still work for a configuration the parser cannot easily
produce — `Config`, `Directive` and `Argument` are plain data:

```go
cfg := nginxlint.Config{Directives: []nginxlint.Directive{{
	Name:  "server_tokens",
	Args:  []nginxlint.Argument{{Value: "on"}},
	Line:  2,
	Column: 5,
}}}
```

## Layout

- `plugin.go`, `fix.go` — the API a rule uses. Pure Go, no generated bindings,
  which is what makes rules host-testable.
- `exports/export_wit_world/` — the adapter between the bindings and a
  registered plugin. It owns the handle drops and the WIT conversions, so a
  rule never sees either.
- `export/` — the blank import that links the exports in. Empty on the host.
- `nginxlinttest/` — the test runner, and the two wasm modules it runs.
- `internal/snapshot/` — the one conversion from the linter's flat parse
  output into a `Config`, shared by the adapter and the test runner so the two
  cannot drift.
- `bindings/` — generated by `make bindings`, and committed: a Go module is
  consumed as source and cannot run a code generator on the way in.
- `wit/` — a copy of the repository's WIT, committed for the same reason and
  checked against the root in CI.

## Working on the SDK

```bash
make bindings   # regenerate bindings/ after a WIT change; commit the result
make check      # vet + test
```

The wasm modules under `nginxlinttest/` are built from the repository root,
where the Rust crates are:

```bash
make build-testkit-wasm   # refresh the committed copies after changing a crate
make check-testkit-wasm   # are the committed copies still current?
```

`check-testkit-wasm` is what catches a crate change that was never rebuilt. It
builds fresh copies without overwriting the committed ones and requires the
two to agree on every configuration in the repository — a byte diff cannot do
it, because a rustc-built wasm is not reproducible across toolchains. CI runs
it on every change to `plugins/`, `crates/`, `src/` or `wit/`.

## License

MIT
