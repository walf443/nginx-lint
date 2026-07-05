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
  DirectiveContext,
  Directive,
  Config,
} from "./generated/interfaces/nginx-lint-plugin-config-api.js";
