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
 * // Optional: only needed to test what a rule's fixes produce.
 * import fixerCore from "nginx-lint-plugin/wasm/fixer/fixer.core.wasm";
 *
 * const { parseConfig, PluginTestRunner } = await createTesting({
 *   getCoreModule: () => coreModule,
 *   getFixerCoreModule: () => fixerCore,
 *   // The core modules have no imports, so they can be instantiated synchronously.
 *   instantiateCore: (module) => new WebAssembly.Instance(module),
 * });
 * ```
 */

import { instantiate } from "../wasm/parser/parser.js";
import { instantiate as instantiateFixer } from "../wasm/fixer/fixer.js";
import {
  makeParseConfig,
  makePluginTestRunner,
  type ApplyFixesFn,
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
  /**
   * Compile the fix applier's core WASM module ("fixer.core.wasm"), enabling
   * `PluginTestRunner.fixString()` / `assertFixed()`. Optional: omit it and
   * those two throw with instructions, leaving everything else working — so
   * a caller that only parses need not bundle a second module.
   */
  getFixerCoreModule?: (
    path: string,
  ) => WebAssembly.Module | Promise<WebAssembly.Module>;
}

export interface Testing {
  parseConfig: ParseConfigFn;
  PluginTestRunner: PluginTestRunnerClass;
  /**
   * Apply fixes without going through a runner. Throws unless
   * {@link CreateTestingOptions.getFixerCoreModule} was supplied.
   */
  applyFixes: ApplyFixesFn;
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

  let applyFixes: ApplyFixesFn;
  if (options.getFixerCoreModule) {
    const fixer = await instantiateFixer(
      options.getFixerCoreModule,
      {} as never,
      options.instantiateCore,
    );
    applyFixes = fixer.applyFixes;
  } else {
    applyFixes = () => {
      throw new Error(
        "Testing fixes needs the applier component: pass getFixerCoreModule " +
          '(e.g. import coreModule from "nginx-lint-plugin/wasm/fixer/fixer.core.wasm") ' +
          "to createTesting().",
      );
    };
  }

  const PluginTestRunner = makePluginTestRunner(parseConfig, applyFixes);
  return { parseConfig, PluginTestRunner, applyFixes };
}
