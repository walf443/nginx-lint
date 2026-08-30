/**
 * Loader-agnostic testing internals.
 *
 * Given the raw component `parse-config` export, this builds the high-level
 * `parseConfig` (returns a method-based Config) and the `PluginTestRunner`
 * class. It performs NO WASM instantiation itself, so it is shared by:
 *   - `./plugin-test-runner.ts` (the default `nginx-lint-plugin/testing` entry,
 *     which instantiates with the built-in fetch/fs loader), and
 *   - `./plugin-test-runner-custom.ts` (the `nginx-lint-plugin/testing/custom`
 *     entry, which takes a caller-supplied core module — e.g. on Cloudflare
 *     Workers / workerd, where fetch()/node:fs are unavailable).
 */

import { buildConfigFromParseOutput } from "./config-builder.js";
import type { Config } from "./generated/interfaces/nginx-lint-plugin-config-api.js";
import type {
  LintError,
  PluginSpec,
} from "./generated/interfaces/nginx-lint-plugin-types.js";
import type { ParseOutput } from "../wasm/parser/interfaces/nginx-lint-plugin-parser-types.js";
import type { Fix } from "./generated/interfaces/nginx-lint-plugin-types.js";
import type { FixResult } from "../wasm/fixer/interfaces/nginx-lint-plugin-fixer-types.js";

/** The raw component export: parse a config string into the flat ParseOutput. */
export type WasmParseConfig = (
  source: string,
  includeContext: string[],
) => ParseOutput;

/** What the linter's fix applier did with a set of fixes. */
export type { FixResult };

/** Apply fixes the way `nginx-lint --fix` would. */
export type ApplyFixesFn = (content: string, fixes: Fix[]) => FixResult;

/** High-level parse: returns the method-based Config used by plugins. */
export type ParseConfigFn = (
  source: string,
  opts?: { includeContext?: string[] },
) => Config;

type SpecFn = () => PluginSpec;
type CheckFn = (cfg: Config, path: string) => LintError[];

/** Wrap the raw component `parse-config` into the high-level `parseConfig`. */
export function makeParseConfig(parseConfigWasm: WasmParseConfig): ParseConfigFn {
  return function parseConfig(source, opts) {
    const output = parseConfigWasm(source, opts?.includeContext ?? []);
    return buildConfigFromParseOutput(output);
  };
}

/** Instance shape of the runner produced by {@link makePluginTestRunner}. */
export interface PluginTestRunner {
  /** Parse and check a config string, returning only this rule's errors. */
  checkString(content: string, opts?: { includeContext?: string[] }): LintError[];
  /** Assert the config produces exactly `count` errors from this rule. */
  assertErrors(content: string, count: number): void;
  /** Assert the config produces at least one error on `line`. */
  assertErrorOnLine(content: string, line: number): void;
  /** Assert `badConf` produces errors and `goodConf` produces none. */
  testExamples(badConf: string, goodConf: string): void;
  /** Apply every fix this rule reports for `content`. */
  fixString(content: string, opts?: { includeContext?: string[] }): FixResult;
  /** Assert that applying this rule's fixes to `content` yields `expected`. */
  assertFixed(
    content: string,
    expected: string,
    opts?: { includeContext?: string[] },
  ): void;
}

/** Constructor of {@link PluginTestRunner}. */
export interface PluginTestRunnerClass {
  new (spec: SpecFn, check: CheckFn): PluginTestRunner;
}

/** Build the PluginTestRunner class bound to a given `parseConfig`. */
export function makePluginTestRunner(
  parseConfig: ParseConfigFn,
  applyFixes: ApplyFixesFn,
): PluginTestRunnerClass {
  // The explicit PluginTestRunnerClass return type keeps the anonymous class's
  // private members out of re-exporters' declaration files (TS4094).
  return class PluginTestRunner {
    private specFn: SpecFn;
    private checkFn: CheckFn;

    constructor(spec: SpecFn, check: CheckFn) {
      this.specFn = spec;
      this.checkFn = check;
    }

    /**
     * Parse and check a config string, returning only errors from this plugin's rule.
     */
    checkString(
      content: string,
      opts?: { includeContext?: string[] },
    ): LintError[] {
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
     * Apply every fix this rule reports for `content`.
     *
     * All of the rule's fixes go through the applier in one call, as
     * `nginx-lint --fix --rule-only <rule>` does, so overlapping fixes
     * resolve the same way here as in production. One difference: this
     * calls the plugin's `check` directly, so findings the linter would
     * have suppressed via an ignore comment still contribute their fixes.
     */
    fixString(content: string, opts?: { includeContext?: string[] }): FixResult {
      const errors = this.checkString(content, opts);
      const fixes = errors.flatMap((e) => e.fixes);
      return applyFixes(content, fixes);
    }

    /**
     * Assert that applying this rule's fixes to `content` yields `expected`.
     *
     * Fails if any fix was unapplicable rather than comparing the output of
     * a fix that silently did nothing. The applier always ends its output
     * with a newline, so a config written without a trailing one comes back
     * with it added.
     */
    assertFixed(
      content: string,
      expected: string,
      opts?: { includeContext?: string[] },
    ): void {
      const result = this.fixString(content, opts);
      if (result.skippedInvalid > 0) {
        throw new Error(
          `${result.skippedInvalid} fix(es) from "${this.specFn().name}" could not be applied ` +
            `(applied ${result.applied}); the rule is producing fixes the linter rejects`,
        );
      }
      if (result.content !== expected) {
        const hint =
          result.content === `${expected}\n`
            ? "\n(the only difference is the trailing newline the applier always adds)"
            : "";
        throw new Error(
          `Applying ${result.applied} fix(es) from "${this.specFn().name}" gave:\n` +
            `${JSON.stringify(result.content)}\nexpected:\n${JSON.stringify(expected)}${hint}`,
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
  };
}
