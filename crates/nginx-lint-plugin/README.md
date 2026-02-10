# nginx-lint-plugin

Plugin SDK for building custom [nginx-lint](https://github.com/walf443/nginx-lint) rules as WASM plugins.

## Overview

This crate provides everything needed to create lint rules for nginx configuration files. Plugins are compiled to WASM and loaded by the nginx-lint host at runtime.

## Quick Start

Create a new plugin project:

```bash
cargo new --lib my-nginx-rule
cd my-nginx-rule
```

Add dependencies to `Cargo.toml`:

```toml
[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
nginx-lint-plugin = "0.2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[features]
default = ["wasm-export"]
wasm-export = []
```

Implement the plugin in `src/lib.rs`:

```rust
use nginx_lint_plugin::prelude::*;

#[derive(Default)]
pub struct NoAutoindexPlugin;

impl Plugin for NoAutoindexPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "no-autoindex",
            "security",
            "Disallow autoindex directive for security",
        )
        .with_severity("warning")
        .with_why("Directory listing can expose sensitive files to attackers.")
        .with_bad_example("location / {\n    autoindex on;\n}")
        .with_good_example("location / {\n    autoindex off;\n}")
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();

        for ctx in config.all_directives_with_context() {
            if ctx.directive.is("autoindex") && ctx.directive.first_arg_is("on") {
                errors.push(
                    err.warning_at("autoindex should be 'off'", ctx.directive)
                       .with_fix(ctx.directive.replace_with("autoindex off;")),
                );
            }
        }

        errors
    }
}

nginx_lint_plugin::export_plugin!(NoAutoindexPlugin);
```

Build for WASM:

```bash
cargo build --target wasm32-unknown-unknown --release
```

## Key Concepts

### Plugin Trait

Every plugin must implement [`Plugin`], which requires two methods:

- **`spec()`** - Returns metadata about the rule (name, category, description, examples)
- **`check()`** - Inspects the parsed nginx config and returns lint errors

### Config Traversal

The SDK provides two ways to iterate over directives:

```rust
// Simple iteration over all directives
for directive in config.all_directives() {
    if directive.is("worker_connections") {
        // ...
    }
}

// Context-aware iteration (knows parent blocks)
for ctx in config.all_directives_with_context() {
    if ctx.is_inside("http") && ctx.directive.is("server_tokens") {
        // This directive is inside an http block
    }

    if ctx.parent_is("server") {
        // Direct child of a server block
    }
}
```

### Error Reporting

Use `ErrorBuilder` (created via `PluginSpec::error_builder()`) to create errors:

```rust
let err = self.spec().error_builder();

// Warning at a directive's location
err.warning_at("message", directive);

// Error at a specific line/column
err.error("message", line, column);
```

### Autofix Support

Attach fixes to errors for automatic correction:

```rust
// Replace a directive
err.warning_at("use 'off'", directive)
    .with_fix(directive.replace_with("autoindex off;"));

// Delete a line
err.warning_at("remove this", directive)
    .with_fix(directive.delete_line());

// Insert after a directive
err.warning_at("missing directive", directive)
    .with_fix(directive.insert_after("add_header X-Frame-Options DENY;"));
```

### Include Context

When nginx-lint processes `include` directives, included files receive context about where they were included from. Use `ConfigExt` methods to check this:

```rust
use nginx_lint_plugin::prelude::*;

// Check if file is included from within http context
if config.is_included_from_http() {
    // This file was included inside an http { } block
}

// Check if inside http > server context
if config.is_included_from_http_server() {
    // Included from within server { } inside http { }
}
```

## Testing

The SDK provides `PluginTestRunner` and `TestCase` for testing plugins:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::{PluginTestRunner, TestCase};

    #[test]
    fn test_detects_bad_config() {
        let runner = PluginTestRunner::new(NoAutoindexPlugin);

        // Assert error count
        runner.assert_errors("http {\n    autoindex on;\n}", 1);

        // Assert no errors for good config
        runner.assert_no_errors("http {\n    autoindex off;\n}");
    }

    #[test]
    fn test_with_builder() {
        TestCase::new("http {\n    autoindex on;\n}")
            .expect_error_count(1)
            .expect_error_on_line(2)
            .expect_message_contains("autoindex")
            .expect_has_fix()
            .run(&NoAutoindexPlugin);
    }

    #[test]
    fn test_with_fixtures() {
        let runner = PluginTestRunner::new(NoAutoindexPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(NoAutoindexPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }
}
```

### Fixture Directory Structure

```
tests/fixtures/
└── 001_basic/
    ├── error/nginx.conf      # Config that should trigger errors
    └── expected/nginx.conf   # Config after applying fixes
```

## Modules

| Module | Description |
|--------|-------------|
| `types` | Core types: `Plugin`, `PluginSpec`, `LintError`, `Fix`, `Config` extensions |
| `helpers` | Utility functions: `is_domain_name()`, `extract_host_from_url()`, etc. |
| `testing` | Test utilities: `PluginTestRunner`, `TestCase`, `fixtures_dir!()` |
| `native` | `NativePluginRule` adapter for running plugins without WASM overhead |
| `prelude` | Convenient re-exports for `use nginx_lint_plugin::prelude::*` |

## License

MIT
