/**
 * The `plugin-rules` world from TypeScript: a component carrying one or
 * more rules.
 *
 * A plugin module defines each rule as a {@link Rule} and exports the
 * world's two functions from {@link defineRules}:
 *
 * ```typescript
 * export const { specs, check } = defineRules(serverTokens, autoindex);
 * ```
 *
 * The host loads the component as one rule per entry, each with its own
 * name, documentation and configuration. `check` reconstructs the config
 * once and runs the rules the host asked for over it.
 */

import { API_VERSION } from "./api-version.js";
import {
  buildConfigFromSnapshot,
  type ReconstructedConfig,
} from "./config-builder.js";
import type { Config } from "./generated/interfaces/nginx-lint-plugin-config-api.js";
import type {
  LintError,
  PluginSpec,
} from "./generated/interfaces/nginx-lint-plugin-types.js";

/**
 * A rule's metadata. {@link PluginSpec} with `apiVersion` optional: when
 * omitted, {@link defineRules} fills in the SDK's {@link API_VERSION}, so
 * a plugin need not repeat it.
 */
export type RuleSpec = Omit<PluginSpec, "apiVersion"> & { apiVersion?: string };

/** One lint rule. */
export interface Rule {
  /** The rule's metadata; `name` is what findings and configuration use. */
  spec: RuleSpec;
  /**
   * Directive names the rule reads, if it reads a fixed, known set. The
   * config it gets is then pruned to those directives plus the ancestor
   * blocks needed for context queries, which is far cheaper to transfer
   * and rebuild than the whole file. A rule that warns when a directive
   * is *missing* inside a block has to list that block's name too: with
   * none of the listed names inside it, the block is pruned away with the
   * evidence. Leave undefined to get the whole config, which is also the
   * only way to see comments and blank lines.
   *
   * This is a floor, not a ceiling: when the host asks for several rules
   * at once, the config is pruned to the union of their lists, so a rule
   * can see directives it did not ask for. Match by name; do not read
   * anything into a block being empty or a list having a certain length.
   */
  relevantDirectives?: string[];
  /**
   * Inspect the config and report findings under `spec.name`. When the
   * host asks for several rules at once they share one config, so treat
   * what it returns as read-only: its arrays are fresh per call, the
   * directives and parent stacks inside them are not.
   */
  check(cfg: ReconstructedConfig, path: string): LintError[];
}

/** What {@link defineRules} returns: the `plugin-rules` world's exports. */
export interface RulesExports {
  specs(): PluginSpec[];
  check(cfg: Config, path: string, rules: string[]): LintError[];
}

/**
 * Build the `plugin-rules` exports for these rules, in this order.
 *
 * `check` runs only the rules named in its `rules` argument (the host asks
 * only for enabled ones), ignores names it does not carry, and returns
 * nothing for an empty list. The config is fetched once: pruned to the
 * union of the asked rules' {@link Rule.relevantDirectives} when every
 * asked rule declares them, and whole otherwise.
 *
 * Throws at definition time on an empty list, a rule without a name, or
 * two rules with one name — the host would refuse the component for any
 * of these, and a plugin's own tests are the earlier place to hear it.
 */
export function defineRules(...rules: Rule[]): RulesExports {
  if (rules.length === 0) {
    throw new Error("defineRules needs at least one rule");
  }
  const seen = new Set<string>();
  for (const rule of rules) {
    const name = rule.spec.name;
    if (!name) {
      throw new Error("every rule needs a non-empty spec.name");
    }
    if (seen.has(name)) {
      throw new Error(`two rules are named ${JSON.stringify(name)}`);
    }
    seen.add(name);
  }

  return {
    specs(): PluginSpec[] {
      return rules.map((rule) => specOf(rule));
    },
    check(cfg: Config, path: string, names: string[]): LintError[] {
      const asked = rules.filter((rule) => names.includes(rule.spec.name));
      if (asked.length === 0) {
        return [];
      }
      const config = reconstructFor(asked, cfg);
      return asked.flatMap((rule) => rule.check(config, path));
    },
  };
}

/**
 * A rule's spec as the host receives it, `apiVersion` filled in — also
 * when the rule set it to `undefined` explicitly, which a spread would
 * have kept and the host cannot lower as a string.
 */
export function specOf(rule: Rule): PluginSpec {
  return { ...rule.spec, apiVersion: rule.spec.apiVersion ?? API_VERSION };
}

/**
 * Fetch the config once for these rules: pruned to the union of their
 * `relevantDirectives` when every one declares them, whole otherwise.
 */
export function reconstructFor(rules: Rule[], cfg: Config): ReconstructedConfig {
  const relevant = new Set<string>();
  for (const rule of rules) {
    if (rule.relevantDirectives === undefined) {
      return buildConfigFromSnapshot(cfg.snapshot());
    }
    for (const name of rule.relevantDirectives) {
      relevant.add(name);
    }
  }
  return buildConfigFromSnapshot(cfg.snapshotFiltered([...relevant].sort()));
}
