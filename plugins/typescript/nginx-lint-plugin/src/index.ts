/**
 * nginx-lint-plugin — shared TypeScript library for nginx-lint WASM plugins.
 *
 * Types are auto-generated from the WIT definition (wit/nginx-lint-plugin.wit)
 * by `jco types` during the build step.
 *
 * Usage:
 *   import type { Config, LintError, PluginSpec } from "nginx-lint-plugin";
 */

/**
 * Current API version of the plugin interface.
 *
 * Use this for `PluginSpec.apiVersion` so plugins track the SDK version
 * automatically. Informational only: compatibility is enforced structurally
 * by WIT import resolution (a plugin built against a newer SDK fails to
 * instantiate on an older host). Kept in sync with the Rust SDK's
 * `API_VERSION` in crates/nginx-lint-plugin/src/types.rs.
 */
export const API_VERSION = "1.2";

// --- types interface (severity, fix, lint-error, plugin-spec) ---
export type {
  Severity,
  Fix,
  LintError,
  PluginSpec,
} from "./generated/interfaces/nginx-lint-plugin-types.js";

// --- data-types interface (shared record types) ---
export type {
  ArgumentType,
  ArgumentInfo,
  CommentInfo,
  BlankLineInfo,
  DirectiveData,
} from "./generated/interfaces/nginx-lint-plugin-data-types.js";

// --- config-api interface (resource-based directive, config, etc.) ---
export type {
  ConfigItem,
  ConfigItemDirectiveItem,
  ConfigItemCommentItem,
  ConfigItemBlankLineItem,
  ConfigSnapshot,
  DirectiveContext,
  Directive,
  Config,
} from "./generated/interfaces/nginx-lint-plugin-config-api.js";

/**
 * Reconstruct a method-based {@link Config} from a (possibly filtered)
 * {@link ConfigSnapshot} — the result of `cfg.snapshot()` or
 * `cfg.snapshotFiltered(names)`. Use this when a plugin only reads a fixed,
 * known set of directive names: fetching a filtered snapshot and walking a
 * pruned tree is faster than `cfg.allDirectivesWithContext()`, which walks
 * the whole file via one host call per directive.
 *
 * @example
 * ```ignore
 * export function check(cfg: Config, path: string): LintError[] {
 *   const filtered = buildConfigFromSnapshot(cfg.snapshotFiltered(["http", "gzip"]));
 *   for (const ctx of filtered.allDirectivesWithContext()) { ... }
 * }
 * ```
 *
 * IMPORTANT for `jco componentize`-built plugins: its bundler
 * (StarlingMonkey/wizer) requires a single, fully self-contained JS file —
 * it does not resolve ANY local module import at componentize time (not
 * even a relative path to a file in the plugin's own directory, let alone
 * a package import), failing with "No such file or directory" even though
 * tsc and plain Node resolve the same import fine. A real runtime import of
 * this function must be bundled away before that step: run a bundler (e.g.
 * `esbuild --bundle --format=esm --platform=neutral`) on the tsc output to
 * produce one self-contained file, then pass THAT to `jco componentize`
 * instead of the raw per-file tsc output. See
 * plugins/typescript/server-tokens-enabled-ts/package.json's `build` script
 * for a worked example.
 */
export { buildConfigFromSnapshot } from "./config-builder.js";
