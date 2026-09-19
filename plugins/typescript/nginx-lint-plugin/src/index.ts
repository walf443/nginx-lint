/**
 * nginx-lint-plugin — shared TypeScript library for nginx-lint WASM plugins.
 *
 * Types are auto-generated from the WIT definition (wit/nginx-lint-plugin.wit)
 * by `jco types` during the build step.
 *
 * Usage:
 *   import { defineRules } from "nginx-lint-plugin";
 *   import type { Rule, LintError } from "nginx-lint-plugin";
 *
 *   const serverTokens: Rule = { spec: { ... }, check(cfg, path) { ... } };
 *   export const { specs, check } = defineRules(serverTokens);
 */

export { API_VERSION } from "./api-version.js";

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
 * Reconstruct a method-based config from a {@link ConfigSnapshot} — the
 * result of `cfg.snapshot()` or `cfg.snapshotFiltered(names)`.
 *
 * A {@link Rule} does not call this: `defineRules` fetches and rebuilds the
 * config for the rules the host asked for, pruned to their
 * `relevantDirectives`, and hands each `check` the result. It is exported
 * for code that holds a raw host `Config` itself — a test, or a custom
 * export written against the generated bindings.
 *
 * `defineRules` is a runtime import, so a plugin's build has to bundle
 * before `jco componentize`: its bundler (StarlingMonkey/wizer) requires a
 * single, fully self-contained JS file and does not resolve any module
 * import at componentize time, not even a relative one, failing with "No
 * such file or directory" even though tsc and Node resolve it fine. Run a
 * bundler (e.g. `esbuild --bundle --format=esm --platform=neutral`) on the
 * tsc output and pass that file to `jco componentize`. See
 * plugins/typescript/server-tokens-enabled-ts/package.json's `build`
 * script for a worked example.
 */
export { buildConfigFromSnapshot } from "./config-builder.js";
export type { ReconstructedConfig } from "./config-builder.js";

// --- the plugin-rules world: a component of one or more rules ---
export { defineRules } from "./rules.js";
export type { Rule, RuleSpec, RulesExports } from "./rules.js";
