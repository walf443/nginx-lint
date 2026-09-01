# AGENTS.md

This document provides guidelines for AI agents (such as Claude Code) working on this project.

## Project Overview

nginx-lint is a Rust CLI tool for linting nginx configuration files with WASM plugin support.

## Directory Structure

```
nginx-lint/
├── crates/
│   ├── nginx-lint-parser/     # nginx config parser (AST, lexer)
│   ├── nginx-lint-common/     # Common types (LintRule trait, config, ignore)
│   └── nginx-lint-plugin/     # Plugin SDK for WASM plugins
├── src/
│   ├── main.rs                # CLI entry point
│   ├── lib.rs                 # Library root (re-exports)
│   ├── linter.rs              # Lint engine
│   ├── reporter.rs            # Output formatting (text/json)
│   ├── include.rs             # Include directive handling
│   ├── plugin/                # WASM plugin system
│   │   ├── mod.rs
│   │   ├── loader.rs          # Plugin loader
│   │   ├── component_rule.rs   # Component model rule wrapper
│   │   ├── builtin.rs         # Embedded builtin plugins
│   │   └── error.rs           # Plugin errors
│   ├── rules/                 # Native lint rules
│   │   ├── syntax/            # Syntax checks (braces, semicolons, quotes)
│   │   └── style/             # Style checks (indentation)
│   ├── wasm.rs                # WASM target support
│   └── docs.rs                # Documentation generation
├── plugins/builtin/           # WASM plugins (compiled to .wasm)
│   ├── security/              # server_tokens, autoindex, ssl, etc.
│   ├── best_practices/        # proxy, gzip, error_log, etc.
│   ├── style/                 # trailing_whitespace, space_before_semicolon
│   └── syntax/                # duplicate_directive, invalid_directive_context
├── tests/
│   ├── integration_test.rs
│   └── fixtures/              # Test nginx configuration files
└── Cargo.toml                 # Workspace root
```

## Crate Dependencies

```
nginx-lint-parser (AST, parsing)
      ↓
nginx-lint-common (LintRule trait, config)
      ↓
   ┌──┴──┐
   ↓     ↓
nginx-lint-plugin    nginx-lint (CLI)
(Plugin SDK)         (native rules, WASM host)
```

## Development Guidelines

### Build & Test

```bash
cargo build                    # Build
cargo test                     # Run tests
cargo run -- <file>            # Run CLI
cargo clippy                   # Lint
cargo fmt                      # format

# Build with embedded WASM plugins
make build-plugins             # Build all WASM plugins
cargo build --features wasm-builtin-plugins
```

### Crate-Specific Testing

When modifying a specific crate, always run build and test for that crate:

```bash
# nginx-lint-parser
cd crates/nginx-lint-parser && cargo build && cargo test

# nginx-lint-common
cd crates/nginx-lint-common && cargo build && cargo test

# nginx-lint-plugin
cd crates/nginx-lint-plugin && cargo build && cargo test
```

The Python SDK's crate (`plugins/python/nginx-lint-plugin`) must be built
from its own directory, never with `--manifest-path` from elsewhere. It
depends on `nginx-lint-parser` and `nginx-lint-common` by published version,
and `plugins/python/.cargo/config.toml` patches them back to the working
tree; cargo discovers that config by walking up from the current directory,
so building from anywhere else silently compiles against the released crates
instead (and fails outright between a version bump and its crates.io
publish).

For the same reason Renovate is disabled for that manifest (it runs cargo
from the repository root), so its `pyo3` and `serde` dependencies are bumped
by hand. `pyo3` in particular has to stay new enough for the newest released
CPython: an older one hard-errors on a newer interpreter even with abi3.

The Go SDK (`plugins/go/nginx-lint-plugin`) commits both its generated
bindings and its copy of the WIT, unlike the TypeScript and Python SDKs which
regenerate theirs. A Go module is consumed as source and cannot run a code
generator on the way in, and its `componentize-go.toml` points a consuming
plugin at the vendored WIT. So after any change to `wit/`, run `make copy-wit`
at the root and `make bindings` in the SDK, and commit the result — CI diffs
the WIT copy against the root.

```bash
# Go SDK
cd plugins/go/nginx-lint-plugin && make check     # vet + test, on the host
# Go example plugin
cd plugins/go/server-tokens-enabled-go && make check test-e2e
```

The Go SDK's `nginxlinttest` package runs the real parser and the real fix
applier from a plain `go test`, by embedding them as core wasm modules built
from `nginx-lint-parser --features wasm-json` and `nginx-lint-common --features
wasm-json` and running them under wazero (Go has no component-model runtime).
Those modules are committed. After changing either crate, rebuild and commit
them with `make build-testkit-wasm` at the root. `make check-testkit-wasm`
answers whether the committed copies are still current: it builds fresh ones
without overwriting them and requires the two to agree on every configuration
in the tree, which is how a forgotten rebuild is caught — a byte diff cannot
do it, because rustc does not produce the same wasm across toolchains. CI runs
that check, not the rebuild.

Only the SDK's root package builds for the host; everything under `bindings/`
and `exports/` is wasm-only, so `go test ./...` fails there by design and the
Makefiles target the packages that do build.

### Adding a Native Lint Rule

Native rules are implemented in Rust under `src/rules/`. Use for rules that need access to file system or complex logic.

1. Add the rule to the appropriate file under `src/rules/`
2. Implement the `LintRule` trait
3. Register in `with_default_rules()` in `src/linter.rs`

### Adding a WASM Plugin

WASM plugins are self-contained and easier to develop. Recommended for most new rules.

1. Create a new plugin directory under `plugins/builtin/<category>/<rule-name>/`
2. Implement the `Plugin` trait using `nginx-lint-plugin`:

```rust
use nginx_lint_plugin::prelude::*;

#[derive(Default)]
pub struct MyPlugin;

impl Plugin for MyPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new("my-rule", "category", "Description")
            .with_severity("warning")
            .with_bad_example(include_str!("../examples/bad.conf").trim())
            .with_good_example(include_str!("../examples/good.conf").trim())
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();

        for ctx in config.all_directives_with_context() {
            if ctx.directive.is("some_directive") {
                errors.push(err.warning_at("message", ctx.directive));
            }
        }

        errors
    }
}

nginx_lint_plugin::export_component_plugin!(MyPlugin);
```

3. Add `examples/bad.conf` and `examples/good.conf`
4. Add `tests/fixtures/` with test cases
5. Build with `cargo build --target wasm32-unknown-unknown --release`
6. Register in `src/plugin/builtin.rs`

Check a built plugin end to end with the CLI, which needs nothing but the
directory — the examples travel inside the component:

```bash
nginx-lint test-plugins --plugins <dir> [--fixtures tests/fixtures]
```

It requires the bad example to be reported, the good example to be clean, and
the fixes to resolve the bad example, matching findings by the plugin's own
rule name. This is what the language plugins' `make test-e2e` and the CI
integration job run; do not reach for `--rule-only`, which cannot select a
rule that is disabled by default.

### Severity Levels

- `Error`: Configuration will not work, or critical security issue
- `Warning`: Discouraged settings, potential problems, improvement suggestions

### Test File Organization

```
# Plugin tests (in each plugin directory)
plugins/builtin/<category>/<rule>/
├── src/lib.rs                 # Plugin implementation with unit tests
├── examples/
│   ├── bad.conf               # Example that triggers the rule
│   └── good.conf              # Example after fix
└── tests/fixtures/
    └── 001_basic/
        ├── error/nginx.conf   # Config with errors
        └── expected/nginx.conf # Config after fix

# Native rule tests
tests/fixtures/rules/
├── style/indent/              # Indentation tests
├── syntax/                    # Syntax rule tests
└── ignore/                    # Ignore comment tests

# Parser tests
crates/nginx-lint-parser/tests/fixtures/
└── test_generated/            # Various nginx configs for parse testing
```

## Parser (nginx-lint-parser)

The parser crate provides:
- AST types (`Config`, `Directive`, `Block`, `Argument`)
- `parse_config(path)` - Parse from file
- `parse_string(content)` - Parse from string

### AST Usage

```rust
use nginx_lint_parser::{parse_string, Config};

let config = parse_string("server { listen 80; }").unwrap();

// Iterate over all directives recursively
for directive in config.all_directives() {
    if directive.is("listen") {
        println!("Port: {}", directive.first_arg().unwrap_or("?"));
    }
}

// Argument helpers
let arg = &directive.args[0];
arg.as_str()           // Get string value
arg.is_variable()      // Check if $variable
arg.is_quoted()        // Check if quoted string
arg.is_literal()       // Check if unquoted literal
```

## Plugin SDK (nginx-lint-plugin)

The plugin SDK provides:
- `Plugin` trait for implementing rules
- `PluginSpec` for metadata
- `LintError` for reporting issues
- `Fix` for autofix support
- Testing utilities (`PluginTestRunner`, `TestCase`)

### Context-Aware Iteration

```rust
for ctx in config.all_directives_with_context() {
    // Check if inside a specific block
    if ctx.is_inside("http") && ctx.directive.is("server_tokens") {
        // ...
    }

    // Get parent contexts
    let parents = ctx.parents();  // e.g., ["http", "server"]
}
```

## Commit Messages

```
<type>: <summary>

<body>

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
```
