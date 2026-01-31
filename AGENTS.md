# AGENTS.md

This document provides guidelines for AI agents (such as Claude Code) working on this project.

## Project Overview

nginx-lint is a Rust CLI tool for linting nginx configuration files.

## Directory Structure

```
nginx-lint/
├── src/
│   ├── main.rs          # CLI entry point
│   ├── lib.rs           # Library root
│   ├── parser.rs        # Wrapper for nginx-config crate
│   ├── linter.rs        # Lint engine & LintRule trait definition
│   ├── reporter.rs      # Output formatting (text/json)
│   └── rules/           # Lint rule implementations
│       ├── mod.rs
│       ├── syntax.rs    # Syntax checks (brace matching, etc.)
│       ├── style.rs     # Style checks (indentation, etc.)
│       ├── security.rs  # Security checks
│       └── best_practices.rs
├── tests/
│   ├── integration_test.rs
│   └── fixtures/        # Test nginx configuration files
└── Cargo.toml
```

## Development Guidelines

### Build & Test

```bash
cargo build          # Build
cargo test           # Run tests
cargo run -- <file>  # Run
```

### Adding a New Lint Rule

1. Add the rule to the appropriate file under `src/rules/`
2. Implement the `LintRule` trait:

```rust
pub struct MyRule;

impl LintRule for MyRule {
    fn name(&self) -> &'static str {
        "my-rule"
    }

    fn description(&self) -> &'static str {
        "Description of what this rule checks"
    }

    fn check(&self, config: &Main, path: &Path) -> Vec<LintError> {
        // Implementation
    }
}
```

3. Register the rule in `with_default_rules()` in `src/linter.rs`
4. Add test fixtures under `tests/fixtures/`
5. Add integration tests in `tests/integration_test.rs`

### Severity Levels

- `Error`: Configuration will not work, or critical security issue
- `Warning`: Discouraged settings, potential problems
- `Info`: Improvement suggestions, best practices

### Test File Organization

Each test case is organized in separate directories:
```
tests/fixtures/
├── valid/           # Valid configuration (no issues)
├── warnings/        # Configuration with warnings
├── bad_indentation/ # Indentation errors
└── unmatched_braces/ # Brace mismatch errors
```

## Dependencies

- `nginx-config`: nginx configuration file parser (some directives unsupported)
- `clap`: CLI framework
- `colored`: Colored output
- `serde`/`serde_json`: JSON output
- `thiserror`: Error type definitions

## Known Limitations

- The `nginx-config` crate does not support some directives:
  - `ssl_protocols`, `gzip_types`, `autoindex`, etc.
- `include` directive file expansion is not implemented

## Commit Messages

```
<type>: <summary>

<body>

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
```
