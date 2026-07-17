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
    // Literal on purpose: jco componentize cannot resolve runtime imports.
    // Assert equality with the SDK's API_VERSION constant in your tests
    // (which run in Node) to keep it in sync.
    apiVersion: "1.2",
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

## Performance: Reading Only What Your Rule Needs

By default, `check()` gets the entire parsed config: `cfg.allDirectivesWithContext()` makes one host call per directive in the file. For large config files this dominates the per-check cost, even if your rule only ever reads one or two directive names.

If your rule only inspects a fixed, known set of directive names, fetch a filtered snapshot instead and rebuild a `Config` from it with `buildConfigFromSnapshot`:

```typescript
import { buildConfigFromSnapshot } from "nginx-lint-plugin";
import type { Config, LintError } from "nginx-lint-plugin";

const RELEVANT_DIRECTIVES = ["autoindex"];

export function check(rawCfg: Config, path: string): LintError[] {
  const cfg = buildConfigFromSnapshot(rawCfg.snapshotFiltered(RELEVANT_DIRECTIVES));

  const errors: LintError[] = [];
  for (const ctx of cfg.allDirectivesWithContext()) {
    // ... same logic as before, now walking a much smaller tree
  }
  return errors;
}
```

The host sends back a config pruned to just those directive names (plus the ancestor blocks needed for `parentStack`/`is_inside`-style checks to keep working), instead of the whole file. Skipping this (calling `allDirectivesWithContext()` on `rawCfg` directly, as in Quick Start above) behaves exactly as before — this is purely opt-in.

**One important exception**: if your rule warns when a directive is *missing* inside some block (e.g. "this `http` block has no `server_tokens`"), include that enclosing block's own name (`"http"`) in the list, not just the directive you're checking for. Otherwise a block with none of the listed directives inside it has nothing to keep it in the pruned config, and the host drops it entirely — along with the evidence your rule needs to report the block exists but is missing something. If your rule only reports on directives it finds (the common case), you don't need to list ancestor block names — a matched directive's ancestors are always kept automatically.

Don't do this if `check()` reads comments or blank lines (`ConfigItem`'s `comment-item`/`blank-line-item` variants): the pruned snapshot never includes them.

### Bundling required for `jco componentize`

`jco componentize`'s bundler (StarlingMonkey/wizer) does not resolve local module imports at componentize time — not a bare package specifier, not even a relative import to a file in your own plugin's directory. If your `check()` imports a real value from `nginx-lint-plugin` (like `buildConfigFromSnapshot` above — type-only imports are fine, they're erased by `tsc`), bundle it away first with a tool like [esbuild](https://esbuild.github.io/) before handing the output to `jco componentize`:

```json
{
  "scripts": {
    "build": "tsc && npm run bundle && jco componentize dist/plugin.bundle.js -w node_modules/nginx-lint-plugin/wit -n plugin --disable all -o dist/my-plugin.wasm",
    "bundle": "esbuild dist/plugin.js --bundle --format=esm --platform=neutral --outfile=dist/plugin.bundle.js"
  },
  "devDependencies": {
    "esbuild": "^0.28"
  }
}
```

`npm test` can keep running directly against `tsc`'s unbundled output (see Testing below) — bundling only needs to happen on the `.wasm` build path. See `plugins/typescript/server-tokens-enabled-ts` in the nginx-lint repo for a complete worked example.

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

### Custom runtimes (Cloudflare Workers / workerd)

`nginx-lint-plugin/testing` loads the parser WASM with `node:fs`/`fetch`, which
are unavailable in some runtimes — most notably Cloudflare Workers (workerd).
For those, import `nginx-lint-plugin/testing/custom` and supply the core module
yourself. The bundler (e.g. wrangler/esbuild) precompiles a `.wasm` import into
a `WebAssembly.Module`, and since the parser core has no imports it can be
instantiated synchronously:

```ts
import { createTesting } from "nginx-lint-plugin/testing/custom";
import coreModule from "nginx-lint-plugin/wasm/parser/parser.core.wasm";

const { parseConfig, PluginTestRunner } = await createTesting({
  getCoreModule: () => coreModule,
  instantiateCore: (module) => new WebAssembly.Instance(module),
});

// Same API as nginx-lint-plugin/testing from here on:
const runner = new PluginTestRunner(spec, check);
runner.assertErrors("http { server_tokens on; }", 1);
```

`createTesting` returns the same `parseConfig` and `PluginTestRunner` as the
default entry. In Node/browser you don't need this — use `.../testing`.

## Building a Plugin

### package.json

The WIT definition file is bundled with this package, so `jco componentize` can reference it directly from `node_modules`.

```json
{
  "name": "my-plugin",
  "type": "module",
  "scripts": {
    "build": "tsc && jco componentize dist/plugin.js -w node_modules/nginx-lint-plugin/wit -n plugin --disable all -o dist/my-plugin.wasm",
    "test": "tsc && node --test dist/plugin.test.js"
  },
  "dependencies": {
    "nginx-lint-plugin": "^0.18.0"
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
nginx-lint --plugins ./dist path/to/nginx.conf
```

The `--plugins` option takes a directory path. nginx-lint automatically loads all `.wasm` files found in that directory.

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
| `snapshot()` | Entire config as one flat value, for `buildConfigFromSnapshot` |
| `snapshotFiltered(names)` | Like `snapshot()`, pruned to `names` plus ancestor blocks — see Performance above |
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
  apiVersion: string;   // API version — keep in sync with the SDK's exported API_VERSION
  severity?: string;    // Default: "warning". Also accepts "error"
  why?: string;         // Explanation of why this rule matters
  badExample?: string;  // Config that triggers the rule
  goodExample?: string; // Config that passes the rule
  references?: string[];// Links to relevant documentation
}
```

## License

MIT
