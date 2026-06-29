/**
 * Custom-runtime testing entry: `nginx-lint-plugin/testing/custom`.
 *
 * Same API as `nginx-lint-plugin/testing`, but the caller supplies how to load
 * the parser core WASM module. Use this where the built-in fetch/node:fs loader
 * does not work — most notably Cloudflare Workers / workerd, where the core
 * module is imported (and precompiled by the bundler) instead of fetched.
 *
 * Example (Cloudflare Workers):
 * ```ts
 * import { createTesting } from "nginx-lint-plugin/testing/custom";
 * // wrangler/esbuild compiles a .wasm import to a WebAssembly.Module:
 * import coreModule from "nginx-lint-plugin/wasm/parser/parser.core.wasm";
 *
 * const { parseConfig, PluginTestRunner } = await createTesting({
 *   getCoreModule: () => coreModule,
 *   // The core module has no imports, so it can be instantiated synchronously.
 *   instantiateCore: (module) => new WebAssembly.Instance(module),
 * });
 * ```
 */

import { instantiate } from "../wasm/parser/parser.js";
import {
  makeParseConfig,
  makePluginTestRunner,
  type ParseConfigFn,
  type PluginTestRunnerClass,
} from "./testing-core.js";

export interface CreateTestingOptions {
  /**
   * Compile a parser core WASM module by file name (e.g. "parser.core.wasm").
   * Return a `WebAssembly.Module`, or a promise of one.
   */
  getCoreModule: (
    path: string,
  ) => WebAssembly.Module | Promise<WebAssembly.Module>;
  /**
   * Optionally override how the core module is instantiated. Defaults to
   * `WebAssembly.instantiate`. The core module has no imports, so a synchronous
   * `(m) => new WebAssembly.Instance(m)` is valid and keeps `parseConfig` sync.
   */
  instantiateCore?: (
    module: WebAssembly.Module,
    imports: Record<string, unknown>,
  ) => WebAssembly.Instance | Promise<WebAssembly.Instance>;
}

export interface Testing {
  parseConfig: ParseConfigFn;
  PluginTestRunner: PluginTestRunnerClass;
}

/**
 * Instantiate the parser with a caller-provided core-module loader and return
 * the `parseConfig` + `PluginTestRunner` API.
 */
export async function createTesting(
  options: CreateTestingOptions,
): Promise<Testing> {
  // The component declares only type-only imports (data-types/parser-types)
  // that are never invoked at runtime, so `{}` is a safe import object.
  const root = await instantiate(
    options.getCoreModule,
    {} as never,
    options.instantiateCore,
  );
  const parseConfig = makeParseConfig(root.parseConfig);
  const PluginTestRunner = makePluginTestRunner(parseConfig);
  return { parseConfig, PluginTestRunner };
}
