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
│   ├── parser/          # Custom nginx config parser
│   │   ├── mod.rs       # Parser API (parse_config, parse_string)
│   │   ├── ast.rs       # AST type definitions
│   │   ├── lexer.rs     # Tokenizer
│   │   └── error.rs     # Error types
│   ├── linter.rs        # Lint engine & LintRule trait definition
│   ├── reporter.rs      # Output formatting (text/json)
│   └── rules/           # Lint rule implementations
│       ├── mod.rs
│       ├── syntax/      # Syntax checks (brace matching, semicolons, etc.)
│       ├── style/       # Style checks (indentation, etc.)
│       ├── security/    # Security checks (server_tokens, ssl_protocols, etc.)
│       └── best_practices/  # Best practice checks (gzip, error_log, etc.)
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
cargo clippy         # Lint
```

### Adding a New Lint Rule

1. Add the rule to the appropriate file under `src/rules/`
2. Implement the `LintRule` trait:

```rust
use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::Path;

pub struct MyRule;

impl LintRule for MyRule {
    fn name(&self) -> &'static str {
        "my-rule"
    }

    fn description(&self) -> &'static str {
        "Description of what this rule checks"
    }

    fn check(&self, config: &Config, path: &Path) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("some_directive") && directive.first_arg_is("bad_value") {
                errors.push(
                    LintError::new(
                        self.name(),
                        "Error message",
                        Severity::Warning,
                    )
                    .with_location(directive.span.start.line, directive.span.start.column),
                );
            }
        }

        errors
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

- `clap`: CLI framework
- `colored`: Colored output
- `serde`/`serde_json`: JSON output
- `thiserror`: Error type definitions

## Parser

The project uses a custom nginx configuration parser (`src/parser/`) that:
- Accepts any directive name (supports extension modules like ngx_headers_more, lua-nginx-module)
- Provides position tracking for all AST nodes
- Supports round-trip source reconstruction for future autofix functionality

### AST Usage

```rust
// Iterate over all directives recursively
for directive in config.all_directives() {
    // Check directive name
    if directive.is("server_tokens") {
        // Check argument value
        if directive.first_arg_is("on") {
            // Report error
        }
    }
}

// Access directive properties
directive.name          // Directive name (String)
directive.args          // Arguments (Vec<Argument>)
directive.block         // Optional block (Option<Block>)
directive.span          // Source location (Span)
```

## Known Limitations

- `include` directive file expansion is not implemented

## Commit Messages

```
<type>: <summary>

<body>

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
```
