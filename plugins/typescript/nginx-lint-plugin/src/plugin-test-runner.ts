/**
 * Parser-based testing utilities for TypeScript nginx-lint plugins.
 *
 * Provides `parseConfig()` to parse real nginx configuration strings
 * into WIT-compatible Config objects, and `PluginTestRunner` for
 * assertion-based testing.
 *
 * Usage:
 *   import { parseConfig, PluginTestRunner } from "nginx-lint-plugin/testing";
 *
 * This entry instantiates the parser WASM with a built-in loader (node:fs in
 * Node, fetch in the browser). In runtimes without either — e.g. Cloudflare
 * Workers / workerd — import `nginx-lint-plugin/testing/custom` and supply the
 * core module yourself.
 */

import { instantiate } from "../wasm/parser/parser.js";
import { makeParseConfig, makePluginTestRunner } from "./testing-core.js";

// When `getCoreModule` is omitted, jco's generated glue falls back to its
// built-in loader (node:fs in Node, fetch in the browser) — the original
// behavior. Top-level await keeps the exported `parseConfig` synchronous for
// callers. The component declares only type-only imports (data-types/
// parser-types) that are never invoked, so `{}` is a safe import object.
const { parseConfig: parseConfigWasm } = await instantiate(
  undefined as never,
  {} as never,
);

/**
 * Parse an nginx configuration string into a WIT-compatible Config object.
 *
 * Uses the nginx-lint-parser WASM component for accurate parsing identical
 * to the production Rust parser. The DFS traversal (allDirectivesWithContext)
 * is computed on the Rust side.
 */
export const parseConfig = makeParseConfig(parseConfigWasm);

/**
 * Test runner for TypeScript nginx-lint plugins.
 *
 * Mirrors the Rust `PluginTestRunner` API from `nginx-lint-plugin/testing`.
 *
 * Example:
 * ```typescript
 * import { spec, check } from "./plugin.js";
 * import { PluginTestRunner } from "nginx-lint-plugin/testing";
 *
 * const runner = new PluginTestRunner(spec, check);
 * runner.assertErrors("http { server_tokens on; }", 1);
 * runner.assertErrors("http { server_tokens off; }", 0);
 * ```
 */
export const PluginTestRunner = makePluginTestRunner(parseConfig);
export type PluginTestRunner = InstanceType<typeof PluginTestRunner>;
