/**
 * Parser-based testing utilities for TypeScript nginx-lint plugins.
 *
 * Provides `parseConfig()` to parse real nginx configuration strings
 * into WIT-compatible Config objects, and `PluginTestRunner` for
 * assertion-based testing.
 *
 * Usage:
 *   import { parseConfig, PluginTestRunner } from "nginx-lint-plugin/testing";
 */

import { parse_string_to_json } from "../wasm/nginx_lint_parser.js";
import { buildConfig } from "./config-builder.js";
import type { ParsedAst } from "./config-builder.js";
import type { Config } from "./generated/interfaces/nginx-lint-plugin-config-api.js";
import type { LintError, PluginSpec } from "./generated/interfaces/nginx-lint-plugin-types.js";

/**
 * Parse an nginx configuration string into a WIT-compatible Config object.
 *
 * Uses the nginx-lint-parser WASM module for accurate parsing identical
 * to the production Rust parser.
 */
export function parseConfig(
  source: string,
  opts?: { includeContext?: string[] },
): Config {
  const json = parse_string_to_json(source);
  const parsed = JSON.parse(json);
  if (parsed.error) {
    throw new Error(`Failed to parse nginx config: ${parsed.error}`);
  }
  return buildConfig(parsed as ParsedAst, opts);
}

type SpecFn = () => PluginSpec;
type CheckFn = (cfg: Config, path: string) => LintError[];

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
export class PluginTestRunner {
  private specFn: SpecFn;
  private checkFn: CheckFn;

  constructor(spec: SpecFn, check: CheckFn) {
    this.specFn = spec;
    this.checkFn = check;
  }

  /**
   * Parse and check a config string, returning only errors from this plugin's rule.
   */
  checkString(content: string, opts?: { includeContext?: string[] }): LintError[] {
    const cfg = parseConfig(content, opts);
    const errors = this.checkFn(cfg, "test.conf");
    const ruleName = this.specFn().name;
    return errors.filter((e) => e.rule === ruleName);
  }

  /**
   * Assert that parsing and checking a config produces exactly `count` errors
   * from this plugin's rule.
   */
  assertErrors(content: string, count: number): void {
    const errors = this.checkString(content);
    if (errors.length !== count) {
      throw new Error(
        `Expected ${count} error(s) from "${this.specFn().name}", got ${errors.length}: ${JSON.stringify(errors, null, 2)}`,
      );
    }
  }

  /**
   * Assert that a config produces at least one error on the given line.
   */
  assertErrorOnLine(content: string, line: number): void {
    const errors = this.checkString(content);
    const hasLine = errors.some((e) => e.line === line);
    if (!hasLine) {
      const lines = errors.map((e) => e.line);
      throw new Error(
        `Expected error on line ${line} from "${this.specFn().name}", got errors on lines: ${JSON.stringify(lines)}`,
      );
    }
  }

  /**
   * Test bad/good config content.
   * - `badConf` must produce at least one error.
   * - `goodConf` must produce zero errors.
   */
  testExamples(badConf: string, goodConf: string): void {
    const ruleName = this.specFn().name;

    const badErrors = this.checkString(badConf);
    if (badErrors.length === 0) {
      throw new Error(
        `bad.conf should produce at least one "${ruleName}" error, got none`,
      );
    }

    const goodErrors = this.checkString(goodConf);
    if (goodErrors.length > 0) {
      throw new Error(
        `good.conf should produce no "${ruleName}" errors, got: ${JSON.stringify(goodErrors, null, 2)}`,
      );
    }
  }
}
