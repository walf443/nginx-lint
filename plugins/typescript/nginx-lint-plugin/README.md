# nginx-lint-plugin

TypeScript SDK for writing [nginx-lint](https://github.com/walf443/nginx-lint) plugins.

Plugins are compiled to WebAssembly Component Model modules and loaded by the nginx-lint CLI at runtime.

## Install

```bash
npm install nginx-lint-plugin
```

## Quick Start

A plugin is one or more rules. Each rule is a `Rule`: its metadata (`spec`),
the directive names it reads (`relevantDirectives`), and a `check`. The
module exports the two functions the host calls, built by `defineRules`:

```typescript
// src/plugin.ts
import { defineRules } from "nginx-lint-plugin";
import type { LintError, ReconstructedConfig, Rule } from "nginx-lint-plugin";

export const myRule: Rule = {
  spec: {
    name: "my-rule",
    category: "best-practices",
    description: "Describe what this rule checks",
    severity: "warning",
    // apiVersion is filled in by defineRules
  },
  relevantDirectives: ["proxy_pass"],
  check(cfg: ReconstructedConfig, path: string): LintError[] {
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
  },
};

// The component's exports. A plugin with several rules lists them all
// here; the host loads each as its own rule, with its own name,
// documentation and configuration.
export const { specs, check } = defineRules(myRule);
```

The component targets the `plugin-rules` world, which the host loads from
the same release of nginx-lint as this SDK onwards (the two share a version
number). A component built with this SDK does not load on an older
nginx-lint.

## Migrating from the `plugin` world

A plugin written against an earlier SDK exported `spec()` and
`check(cfg, path)` directly. Moving it to `defineRules` is mechanical:

1. **Spec**: turn `function spec(): PluginSpec { return { ... } }` into the
   rule's `spec: { ... }` property. Drop `apiVersion`; `defineRules` fills
   it in. Anything the spec references (example strings, say) has to be
   defined above the rule, since the spec is now a value.
2. **Check**: delete the `buildConfigFromSnapshot(cfg.snapshotFiltered(names))`
   line at the top of `check` and put `names` in the rule's
   `relevantDirectives` instead. The parameter type becomes
   `ReconstructedConfig`. A `check` that walked the raw config with
   `allDirectivesWithContext()` needs no change beyond the type.
3. **Exports**: `export const { specs, check } = defineRules(rule)`. If the
   old `check` function is still exported under that name, rename it.
4. **Build and tests**: `-n plugin-rules` in the `jco componentize` command,
   with the bundle step in place; `new PluginTestRunner(rule)` instead of
   `new PluginTestRunner(spec, check)`; a test that called `check(cfg, path)`
   directly now calls the component's `check(cfg, path, [rule.spec.name])`.

## Performance: Reading Only What Your Rule Needs

`check` gets the config already fetched from the host and rebuilt. What is
fetched depends on `relevantDirectives`:

- Set it to the directive names your rule reads, and the config is pruned to
  those directives plus the ancestor blocks needed for `parentStack` and
  include-context checks to keep working. One host call, proportional to
  what is relevant rather than to the file.
- Leave it undefined, and the whole file is fetched. This is the only way to
  see comments and blank lines (`ConfigItem`'s `comment-item` /
  `blank-line-item` variants): the pruned config never includes them.

When the host asks for several rules of one component at once, the config is
fetched once, pruned to the union of their `relevantDirectives` if every
asked rule declares them.

**One important exception**: if your rule warns when a directive is
*missing* inside some block (e.g. "this `http` block has no
`server_tokens`"), include that enclosing block's own name (`"http"`) in the
list, not just the directive you're checking for. Otherwise a block with none
of the listed directives inside it has nothing to keep it in the pruned
config, and the host drops it entirely — along with the evidence your rule
needs to report the block exists but is missing something. If your rule only
reports on directives it finds (the common case), you don't need to list
ancestor block names — a matched directive's ancestors are always kept
automatically.

### Bundling required for `jco componentize`

`jco componentize`'s bundler (StarlingMonkey/wizer) does not resolve local module imports at componentize time — not a bare package specifier, not even a relative import to a file in your own plugin's directory. `defineRules` is a real value imported from `nginx-lint-plugin` (type-only imports are fine, they're erased by `tsc`), so bundle it away first with a tool like [esbuild](https://esbuild.github.io/) before handing the output to `jco componentize`:

```json
{
  "scripts": {
    "build": "tsc && npm run bundle && jco componentize dist/plugin.bundle.js -w node_modules/nginx-lint-plugin/wit -n plugin-rules --disable all -o dist/my-plugin.wasm",
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
import { myRule } from "./plugin.js";
import { parseConfig, PluginTestRunner } from "nginx-lint-plugin/testing";

describe("my-rule", () => {
  // The runner hands the rule its config the way the host does: pruned to
  // its relevantDirectives when it declares them
  const runner = new PluginTestRunner(myRule);

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

Use `parseConfig` directly to simulate files included from specific blocks,
and call the component's `check` the way the host does — naming the rules to
run:

```typescript
import { check } from "./plugin.js";

it("handles included files", () => {
  const cfg = parseConfig("server_tokens off;", {
    includeContext: ["http", "server"],
  });
  const errors = check(cfg, "test.conf", ["my-rule"]);
  assert.equal(errors.length, 0);
});
```

`runner.checkString(content, { includeContext })` does the same for one rule.

### PluginTestRunner

| Method | Description |
|--------|-------------|
| `checkString(content, opts?)` | Parse and check, returning errors from this rule |
| `assertErrors(content, count)` | Assert exactly N errors |
| `assertErrorOnLine(content, line)` | Assert error on a specific line |
| `testExamples(badConf, goodConf)` | Validate bad config produces errors, good config does not |
| `fixString(content, opts?)` | Apply every fix this rule reports, returning `{ content, applied, skippedInvalid }` |
| `assertFixed(content, expected, opts?)` | Assert applying this rule's fixes yields `expected` |

`fixString`/`assertFixed` run the fixes through the linter's own applier, so a
fix the CLI would normalize into a different operation than intended shows up
as the wrong output rather than passing unnoticed. `applyFixes(content, fixes)`
is exported for checking a single fix without a plugin. Note the applier always
ends its output with a newline.

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
// Only needed for fixString()/assertFixed(); omit it and just those throw.
import fixerCore from "nginx-lint-plugin/wasm/fixer/fixer.core.wasm";

const { parseConfig, PluginTestRunner } = await createTesting({
  getCoreModule: () => coreModule,
  getFixerCoreModule: () => fixerCore,
  instantiateCore: (module) => new WebAssembly.Instance(module),
});

// Same API as nginx-lint-plugin/testing from here on:
const runner = new PluginTestRunner(myRule);
runner.assertErrors("http { server_tokens on; }", 1);
```

`createTesting` returns the same `parseConfig` and `PluginTestRunner` as the
default entry. In Node/browser you don't need this — use `.../testing`.

## Building a Plugin

### package.json

The WIT definition file is bundled with this package, so `jco componentize` can reference it directly from `node_modules`. The bundle step is required: `defineRules` is a runtime import (see above).

```json
{
  "name": "my-plugin",
  "type": "module",
  "scripts": {
    "build": "tsc && npm run bundle && jco componentize dist/plugin.bundle.js -w node_modules/nginx-lint-plugin/wit -n plugin-rules --disable all -o dist/my-plugin.wasm",
    "bundle": "esbuild dist/plugin.js --bundle --format=esm --platform=neutral --outfile=dist/plugin.bundle.js",
    "test": "tsc && node --test dist/plugin.test.js"
  },
  "dependencies": {
    "nginx-lint-plugin": "^0.23.0"
  },
  "devDependencies": {
    "@bytecodealliance/componentize-js": "^0.19",
    "@bytecodealliance/jco": "^1",
    "esbuild": "^0.28",
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

### Rules

```typescript
import { defineRules } from "nginx-lint-plugin";
import type { Rule, RuleSpec, RulesExports } from "nginx-lint-plugin";

// A rule: metadata, the directives it reads, and its check
interface Rule {
  spec: RuleSpec;                  // PluginSpec with apiVersion optional
  relevantDirectives?: string[];   // undefined: the whole config
  check(cfg: ReconstructedConfig, path: string): LintError[];
}

// The component's exports, for one or more rules
function defineRules(...rules: Rule[]): RulesExports;
interface RulesExports {
  specs(): PluginSpec[];
  check(cfg: Config, path: string, rules: string[]): LintError[];
}
```

`defineRules` throws on an empty list, a rule without a name, or two rules
with one name — the host would refuse the component for any of these.

### Types

```typescript
import type {
  // Core types
  Severity,        // "error" | "warning"
  Fix,             // Autofix descriptor
  LintError,       // Lint error with rule, message, line, column, fixes
  PluginSpec,      // Plugin metadata, as the host receives it

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
  apiVersion: string;   // API version; defineRules fills in the SDK's API_VERSION when a rule omits it
  severity?: string;    // Default: "warning". Also accepts "error"
  why?: string;         // Explanation of why this rule matters
  badExample?: string;  // Config that triggers the rule
  goodExample?: string; // Config that passes the rule
  references?: string[];// Links to relevant documentation
}
```

## Developing this SDK

The package imports two WASM components — the parser and the fix applier —
so build both before `npm test` or `npm run build` in a fresh checkout.
They are gitignored, and TypeScript reports them as missing modules if
either is absent:

```bash
# from the repository root
make build-parser-wasm
make build-fixer-wasm
```

The applier is a separate component because it lives in
`nginx-lint-common`, which depends on `nginx-lint-parser` — the parser
component cannot export it without a dependency cycle.

## Checking the built plugin

The CLI can check a built plugin end to end from the examples in its
spec — the bad one has to be reported, the good one clean, and the fixes
have to resolve the bad one:

```bash
nginx-lint test-plugins --plugins <dir>
```

## License

MIT
