# nginx-lint-plugin

TypeScript SDK for writing [nginx-lint](https://github.com/walf443/nginx-lint) plugins.

Plugins are compiled to WebAssembly Component Model modules and loaded by the nginx-lint CLI at runtime.

## Install

```bash
npm install nginx-lint-plugin
```

## Quick Start

A plugin exports two functions: `spec` (metadata) and `check` (lint logic).

```typescript
// src/plugin.ts
import type { Config, LintError, PluginSpec } from "nginx-lint-plugin";

export function spec(): PluginSpec {
  return {
    name: "my-rule",
    category: "best-practices",
    description: "Describe what this rule checks",
    apiVersion: "1.0",
    severity: "warning",
  };
}

export function check(cfg: Config, path: string): LintError[] {
  const errors: LintError[] = [];

  for (const ctx of cfg.allDirectivesWithContext()) {
    const directive = ctx.directive;

    if (directive.is("proxy_pass") && !directive.hasBlock()) {
      errors.push({
        rule: "my-rule",
        category: "best-practices",
        message: "proxy_pass should ...",
        severity: "warning",
        line: directive.line(),
        column: directive.column(),
        fixes: [directive.replaceWith("proxy_pass http://upstream;")],
      });
    }
  }

  return errors;
}
```

## Building a Plugin

### package.json

The WIT definition file is bundled with this package, so `jco componentize` can reference it directly from `node_modules`.

```json
{
  "name": "my-plugin",
  "type": "module",
  "scripts": {
    "build": "tsc && jco componentize dist/plugin.js -w node_modules/nginx-lint-plugin/wit -n plugin --disable all -o my-plugin.wasm",
    "test": "tsc && node --test dist/plugin.test.js"
  },
  "dependencies": {
    "nginx-lint-plugin": "^0.8.0"
  },
  "devDependencies": {
    "@bytecodealliance/componentize-js": "^0.19",
    "@bytecodealliance/jco": "^1",
    "typescript": "^5"
  }
}
```

### tsconfig.json

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ES2022",
    "moduleResolution": "bundler",
    "outDir": "dist",
    "strict": true,
    "skipLibCheck": true,
    "declaration": true
  },
  "include": ["src"]
}
```

### Build and run

```bash
npm run build
nginx-lint --plugins ./my-plugin.wasm path/to/nginx.conf
```

## API Reference

### Types

```typescript
import type {
  // Core types
  Severity,        // "error" | "warning"
  Fix,             // Autofix descriptor
  LintError,       // Lint error with rule, message, line, column, fixes
  PluginSpec,      // Plugin metadata

  // Directive data
  ArgumentType,    // "literal" | "quoted-string" | "single-quoted-string" | "variable"
  ArgumentInfo,    // Argument with value, raw text, type, position
  DirectiveData,   // Flat directive properties

  // Config tree
  Config,          // Parsed nginx configuration
  Directive,       // A single directive with methods
  DirectiveContext,// Directive with parent stack and depth
  ConfigItem,      // Directive | Comment | BlankLine
} from "nginx-lint-plugin";
```

### Config

| Method | Description |
|--------|-------------|
| `allDirectivesWithContext()` | All directives with parent context (DFS order) |
| `allDirectives()` | All directives without context |
| `items()` | Top-level config items (directives, comments, blank lines) |
| `includeContext()` | Parent block names from `include` directives |
| `isIncludedFrom(context)` | Check if included from a specific block |
| `isIncludedFromHttp()` | Check if included from `http` block |
| `isIncludedFromHttpServer()` | Check if included from `http > server` |
| `isIncludedFromHttpLocation()` | Check if included from `http > ... > location` |
| `isIncludedFromStream()` | Check if included from `stream` block |
| `immediateParentContext()` | Immediate parent block name |

### Directive

| Method | Description |
|--------|-------------|
| `name()` | Directive name (e.g. `"server_tokens"`) |
| `is(name)` | Check directive name |
| `firstArg()` | First argument value |
| `firstArgIs(value)` | Check first argument |
| `argAt(index)` | Argument at index |
| `lastArg()` | Last argument value |
| `hasArg(value)` | Check if argument exists |
| `argCount()` | Number of arguments |
| `args()` | All arguments as `ArgumentInfo[]` |
| `line()` / `column()` | Source position |
| `hasBlock()` | Whether directive has a `{ }` block |
| `blockItems()` | Child items inside the block |
| `blockIsRaw()` | Whether block content is raw (e.g. `map`) |

### Directive Fix Builders

| Method | Description |
|--------|-------------|
| `replaceWith(newText)` | Replace the entire directive |
| `deleteLineFix()` | Delete the directive's line |
| `insertAfter(newText)` | Insert text after the directive |
| `insertBefore(newText)` | Insert text before the directive |
| `insertAfterMany(lines)` | Insert multiple lines after |
| `insertBeforeMany(lines)` | Insert multiple lines before |

### DirectiveContext

| Field | Description |
|-------|-------------|
| `directive` | The directive |
| `parentStack` | Parent block names (e.g. `["http", "server"]`) |
| `depth` | Nesting depth |

### PluginSpec

```typescript
{
  name: string;         // Rule identifier (e.g. "my-rule")
  category: string;     // Category (e.g. "security", "best-practices", "style", "syntax")
  description: string;  // Human-readable description
  apiVersion: string;   // API version ("1.0")
  severity?: string;    // Default: "warning". Also accepts "error"
  why?: string;         // Explanation of why this rule matters
  badExample?: string;  // Config that triggers the rule
  goodExample?: string; // Config that passes the rule
  references?: string[];// Links to relevant documentation
}
```

## Testing

The `nginx-lint-plugin/testing` entry provides parser-based testing utilities. Tests parse real nginx configuration strings using the same Rust parser that powers the production linter.

```typescript
import { describe, it } from "node:test";
import { spec, check } from "./plugin.js";
import { parseConfig, PluginTestRunner } from "nginx-lint-plugin/testing";

describe("my-rule", () => {
  const runner = new PluginTestRunner(spec, check);

  it("detects the issue", () => {
    runner.assertErrors("http {\n    proxy_pass http://bad;\n}", 1);
  });

  it("passes valid config", () => {
    runner.assertErrors("http {\n    proxy_pass http://good;\n}", 0);
  });

  it("checks error on specific line", () => {
    runner.assertErrorOnLine("http {\n    proxy_pass http://bad;\n}", 2);
  });

  it("validates bad/good examples", () => {
    runner.testExamples(
      "http {\n    proxy_pass http://bad;\n}",
      "http {\n    proxy_pass http://good;\n}",
    );
  });
});
```

### Testing with include context

Use `parseConfig` directly to simulate files included from specific blocks:

```typescript
it("handles included files", () => {
  const cfg = parseConfig("server_tokens off;", {
    includeContext: ["http", "server"],
  });
  const errors = check(cfg, "test.conf");
  assert.equal(errors.length, 0);
});
```

### PluginTestRunner

| Method | Description |
|--------|-------------|
| `checkString(content, opts?)` | Parse and check, returning errors from this rule |
| `assertErrors(content, count)` | Assert exactly N errors |
| `assertErrorOnLine(content, line)` | Assert error on a specific line |
| `testExamples(badConf, goodConf)` | Validate bad config produces errors, good config does not |

## License

See the [nginx-lint repository](https://github.com/walf443/nginx-lint) for license information.
