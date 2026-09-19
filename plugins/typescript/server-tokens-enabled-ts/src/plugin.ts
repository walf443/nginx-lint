/**
 * server-tokens-enabled plugin (TypeScript version)
 *
 * Detects when server_tokens is enabled, which exposes nginx version
 * information in response headers and error pages.
 *
 * server_tokens defaults to 'on', so this plugin also warns when no
 * server_tokens directive is found in the http context.
 *
 * This is a TypeScript implementation of the Rust plugin at:
 *   plugins/builtin/security/server_tokens_enabled/
 */

import { defineRules } from "nginx-lint-plugin";
import type { LintError, ReconstructedConfig, Rule } from "nginx-lint-plugin";

/**
 * Directive names this plugin reads. "http" must be included alongside
 * "server_tokens" even though the plugin only reports on server_tokens:
 * the warning for a MISSING server_tokens is keyed on "an http block
 * exists but none of its children matched" — if "http" weren't in this
 * list, an http block with no server_tokens inside would have no name
 * match and no kept descendant, so the host would prune it away entirely,
 * silently losing the exact case this plugin needs to detect. (See the
 * Rust port's relevant_directives() for the same reasoning.)
 */
const RELEVANT_DIRECTIVES = ["http", "server_tokens"];

const BAD_EXAMPLE = `http {
  server_tokens on;
  server {
    listen 80;
  }
}`;

const GOOD_EXAMPLE = `http {
  server_tokens off;
  server {
    listen 80;
  }
}`;

/**
 * The rule: metadata, the directives it reads, and the check.
 */
export const serverTokensEnabled: Rule = {
  spec: {
    name: "server-tokens-enabled-ts",
    category: "security",
    description:
      "Detects when server_tokens is enabled (exposes nginx version) [TypeScript]",
    severity: "warning",
    why: "When server_tokens is 'on' (the default), nginx includes its version number in " +
      "the Server response header and on default error pages. This information can help " +
      "attackers identify specific vulnerabilities associated with your nginx version.",
    badExample: BAD_EXAMPLE,
    goodExample: GOOD_EXAMPLE,
    references: [
      "https://nginx.org/en/docs/http/ngx_http_core_module.html#server_tokens",
    ],
  },
  relevantDirectives: RELEVANT_DIRECTIVES,
  check: checkServerTokens,
};

/**
 * Check the nginx config and return lint errors. The config arrives
 * already pruned to RELEVANT_DIRECTIVES (plus their ancestors).
 */
function checkServerTokens(cfg: ReconstructedConfig, _path: string): LintError[] {
  const errors: LintError[] = [];
  let hasServerTokensOff = false;
  let hasServerTokensOn = false;
  let httpBlockLine: number | undefined;

  // Check if this file is included from an http context.
  // When included from http (or http > server, etc.), directives are
  // implicitly inside the http context even without an explicit http block.
  const includeCtx = cfg.includeContext();
  const includedFromHttp = includeCtx.includes("http");

  const contexts = cfg.allDirectivesWithContext();

  for (const ctx of contexts) {
    const directive = ctx.directive;
    const parentStack: string[] = ctx.parentStack;

    // Track if we have an http block and remember its line
    if (directive.is("http")) {
      httpBlockLine = directive.line();
    }

    // Only check server_tokens in http context.
    // include_context means the file is included from within http,
    // so all top-level directives are effectively inside http.
    const insideHttp =
      parentStack.includes("http") ||
      directive.is("http") ||
      includedFromHttp;
    if (!insideHttp) {
      continue;
    }

    if (directive.is("server_tokens")) {
      if (
        directive.firstArgIs("off") ||
        directive.firstArgIs("build")
      ) {
        // 'off' or 'build' both hide the version number
        hasServerTokensOff = true;
      } else if (directive.firstArgIs("on")) {
        hasServerTokensOn = true;
        // Explicit 'on' — warn with fix
        errors.push({
          rule: "server-tokens-enabled-ts",
          category: "security",
          message: "server_tokens should be 'off' to hide nginx version",
          severity: "warning",
          line: directive.line(),
          column: directive.column(),
          fixes: [directive.replaceWith("server_tokens off;")],
        });
      }
    }
  }

  // If we have an http block in THIS file but no server_tokens off/build, warn
  // about the default. Skip if we already warned about explicit 'on'.
  // Don't warn if this file is included from another config — the parent
  // config's http block should set server_tokens.
  if (
    httpBlockLine !== undefined &&
    !includedFromHttp &&
    !hasServerTokensOff &&
    !hasServerTokensOn
  ) {
    errors.push({
      rule: "server-tokens-enabled-ts",
      category: "security",
      message:
        "server_tokens defaults to 'on', consider adding 'server_tokens off;' in http context",
      severity: "warning",
      line: httpBlockLine,
      column: 1,
      fixes: [],
    });
  }

  return errors;
}

/**
 * The component's exports: the `plugin-rules` world, carrying this one
 * rule. A plugin with several rules lists them all here.
 */
export const { specs, check } = defineRules(serverTokensEnabled);
