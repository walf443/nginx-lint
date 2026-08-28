"""server-tokens-enabled plugin (Python version)

Detects when server_tokens is enabled, which exposes nginx version
information in response headers and error pages.

server_tokens defaults to 'on', so this plugin also warns when no
server_tokens directive is found in the http context.

This is a Python implementation of the TypeScript plugin at:
  plugins/typescript/server-tokens-enabled-ts/

Built as a WASM component with componentize-py:
  componentize-py -d wit -w plugin componentize app -o server-tokens-enabled-py.wasm --stub-wasi
"""

from typing import List

import wit_world
from wit_world.imports import config_api
from wit_world.imports.types import LintError, PluginSpec, Severity

RULE_NAME = "server-tokens-enabled-py"

BAD_EXAMPLE = """http {
  server_tokens on;
  server {
    listen 80;
  }
}"""

GOOD_EXAMPLE = """http {
  server_tokens off;
  server {
    listen 80;
  }
}"""


class WitWorld(wit_world.WitWorld):
    def spec(self) -> PluginSpec:
        return PluginSpec(
            name=RULE_NAME,
            category="security",
            description="Detects when server_tokens is enabled (exposes nginx version) [Python]",
            # Keep in sync with API_VERSION in crates/nginx-lint-plugin
            # (informational only; nothing compares it at runtime)
            api_version="1.2",
            severity="warning",
            why=(
                "When server_tokens is 'on' (the default), nginx includes its version "
                "number in the Server response header and on default error pages. This "
                "information can help attackers identify specific vulnerabilities "
                "associated with your nginx version."
            ),
            bad_example=BAD_EXAMPLE,
            good_example=GOOD_EXAMPLE,
            references=[
                "https://nginx.org/en/docs/http/ngx_http_core_module.html#server_tokens",
            ],
            min_nginx_version=None,
            max_nginx_version=None,
        )

    def check(self, cfg: config_api.Config, path: str) -> List[LintError]:
        errors: List[LintError] = []
        has_server_tokens_off = False
        has_server_tokens_on = False
        http_block_line = None

        # When included from http (or http > server, etc.), directives are
        # implicitly inside the http context even without an explicit http block.
        included_from_http = "http" in cfg.include_context()

        for ctx in cfg.all_directives_with_context():
            directive = ctx.directive

            if directive.is_("http"):
                http_block_line = directive.line()

            inside_http = (
                "http" in ctx.parent_stack
                or directive.is_("http")
                or included_from_http
            )
            if not inside_http:
                continue

            if directive.is_("server_tokens"):
                if directive.first_arg_is("off") or directive.first_arg_is("build"):
                    # 'off' or 'build' both hide the version number
                    has_server_tokens_off = True
                elif directive.first_arg_is("on"):
                    has_server_tokens_on = True
                    errors.append(
                        LintError(
                            rule=RULE_NAME,
                            category="security",
                            message="server_tokens should be 'off' to hide nginx version",
                            severity=Severity.WARNING,
                            line=directive.line(),
                            column=directive.column(),
                            fixes=[directive.replace_with("server_tokens off;")],
                        )
                    )

        # If we have an http block in THIS file but no server_tokens off/build,
        # warn about the default. Skip if we already warned about explicit 'on'.
        # Don't warn if this file is included from another config — the parent
        # config's http block should set server_tokens.
        if (
            http_block_line is not None
            and not included_from_http
            and not has_server_tokens_off
            and not has_server_tokens_on
        ):
            errors.append(
                LintError(
                    rule=RULE_NAME,
                    category="security",
                    message=(
                        "server_tokens defaults to 'on', consider adding "
                        "'server_tokens off;' in http context"
                    ),
                    severity=Severity.WARNING,
                    line=http_block_line,
                    column=1,
                    fixes=[],
                )
            )

        return errors
