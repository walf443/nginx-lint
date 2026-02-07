# nginx-lint

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A linter for nginx configuration files with WASM plugin support, autofix, and a browser-based Web UI.

## Features

- **30+ built-in rules** covering security, best practices, style, syntax, and deprecation
- **Autofix** — automatically fix problems with `--fix`
- **WASM plugin system** — extend with custom rules written in Rust and compiled to WebAssembly
- **Web UI** — lint interactively in the browser with real-time feedback
- **Ignore comments** — suppress specific warnings with inline annotations
- **Configurable** — customize rules, severity, and options via `.nginx-lint.toml`
- **JSON output** — machine-readable output for CI integration

## Quick Start

```bash
# Lint a configuration file
nginx-lint /etc/nginx/nginx.conf

# Automatically fix problems
nginx-lint --fix /etc/nginx/nginx.conf

# Show why a rule exists
nginx-lint why server-tokens-enabled

# List all available rules
nginx-lint why --list
```

## Usage

```
nginx-lint [OPTIONS] [FILE]...
nginx-lint <COMMAND>
```

### Options

| Flag | Description |
|------|-------------|
| `-o, --format <FORMAT>` | Output format: `text` (default) or `json` |
| `--fix` | Automatically fix problems |
| `-c, --config <FILE>` | Path to configuration file |
| `--context <CONTEXT>` | Parent context for partial configs (e.g., `http,server`) |
| `--plugins <DIR>` | Directory containing custom WASM plugins |
| `--color` / `--no-color` | Force or disable colored output |
| `--no-fail-on-warnings` | Only fail on errors, not warnings |
| `-v, --verbose` | Show verbose output |
| `--profile` | Show time spent per rule |

### Subcommands

**`config`** — Configuration file management

```bash
nginx-lint config init                  # Generate default .nginx-lint.toml
nginx-lint config init -o custom.toml   # Custom output path
nginx-lint config validate              # Validate configuration
```

**`why`** — Show detailed documentation for a rule

```bash
nginx-lint why server-tokens-enabled    # Explain a rule
nginx-lint why --list                   # List all rules
```

## Configuration

Generate a default configuration file with:

```bash
nginx-lint config init
```

This creates `.nginx-lint.toml`:

```toml
[color]
ui = "auto"       # "auto", "always", or "never"
error = "red"
warning = "yellow"

[rules.server-tokens-enabled]
enabled = true

[rules.indent]
indent_size = "auto"   # or a number like 4

[rules.deprecated-ssl-protocol]
allowed_protocols = ["TLSv1.2", "TLSv1.3"]

# Support non-standard directives from extension modules
[rules.invalid-directive-context]
additional_contexts = { server = ["rtmp"], upstream = ["rtmp"] }

[parser]
block_directives = ["rtmp", "application"]
```

## Rules

See the [rules list](https://walf443.github.io/nginx-lint/rules.html) for all available rules, or run `nginx-lint why --list` locally.

## Ignore Comments

Suppress warnings using `nginx-lint:ignore` comments. Both a rule name and a reason are required.

**Comment on the line before:**

```nginx
# nginx-lint:ignore server-tokens-enabled required by monitoring system
server_tokens on;
```

**Inline comment:**

```nginx
server_tokens on; # nginx-lint:ignore server-tokens-enabled required by monitoring system
```

### Context Comments

When linting partial configuration files (e.g., included snippets), specify the parent context:

```nginx
# nginx-lint:context http,server
location /api {
    proxy_pass http://backend;
}
```

This is equivalent to `--context http,server` on the command line.

## Web UI

Start the browser-based linting interface:

```bash
nginx-lint web --open
```

The Web UI provides:

- Real-time linting as you type
- Interactive fix buttons for each issue
- "Fix All" to apply all fixes at once
- Rule documentation with bad/good examples
- In-browser configuration editing
- Runs entirely client-side via WebAssembly

## Custom Plugins

Load custom WASM plugins from a directory:

```bash
nginx-lint --plugins ./my-plugins /etc/nginx/nginx.conf
```

Each `.wasm` file in the directory is loaded as a plugin. See the `plugins/builtin/` directory for examples of how to write plugins using the `nginx-lint-plugin` SDK.

## Installation

### From source

```bash
# Default build (CLI + builtin plugins)
cargo install --path .

# Build WASM plugins first, then build with them embedded
make build-plugins
cargo install --path . --features builtin-plugins

# With web server support
cargo install --path . --features web-server
```

### Cargo features

| Feature | Description |
|---------|-------------|
| `cli` | Command-line interface (default) |
| `builtin-plugins` | Embed builtin WASM plugins in the binary (default) |
| `plugins` | Support loading external WASM plugins |
| `web-server` | Built-in web server for browser UI |
| `wasm` | WebAssembly target support |

## License

[MIT](LICENSE)
