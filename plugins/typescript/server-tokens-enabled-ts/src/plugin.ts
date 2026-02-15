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

// WIT types are provided by jco componentize from the WIT definition.
// - `config` and `directive` are WIT resources, passed as objects with methods.
// - WIT kebab-case names are converted to camelCase by jco.

/**
 * Return plugin metadata.
 * Exported as `spec` per the WIT world definition.
 */
export function spec() {
  return {
    name: "server-tokens-enabled-ts",
    category: "security",
    description:
      "Detects when server_tokens is enabled (exposes nginx version) [TypeScript]",
    apiVersion: "3.0.0",
    severity: "warning",
    why: "When server_tokens is 'on' (the default), nginx includes its version number in " +
      "the Server response header and on default error pages. This information can help " +
      "attackers identify specific vulnerabilities associated with your nginx version.",
    badExample: BAD_EXAMPLE,
    goodExample: GOOD_EXAMPLE,
    references: [
      "https://nginx.org/en/docs/http/ngx_http_core_module.html#server_tokens",
    ],
  };
}

/**
 * Check the nginx config and return lint errors.
 * Exported as `check` per the WIT world definition.
 *
 * @param cfg - A WIT `config` resource (host-backed). Has methods like
 *              `allDirectivesWithContext()`, `allDirectives()`, etc.
 * @param path - The file path being checked.
 */
export function check(cfg: any, path: string) {
  const errors: any[] = [];
  let hasServerTokensOff = false;
  let hasServerTokensOn = false;
  let httpBlockLine: number | undefined;

  const contexts = cfg.allDirectivesWithContext();

  for (const ctx of contexts) {
    const directive = ctx.directive;
    const parentStack: string[] = ctx.parentStack;

    // Track if we have an http block and remember its line
    if (directive.is("http")) {
      httpBlockLine = directive.line();
    }

    // Only check server_tokens in http context
    const insideHttp = parentStack.includes("http") || directive.is("http");
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
  if (
    httpBlockLine !== undefined &&
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
