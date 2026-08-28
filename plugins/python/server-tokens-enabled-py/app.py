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

from nginx_lint_plugin import (
    Config,
    LintError,
    Plugin,
    PluginSpec,
    build_config_from_snapshot,
    error_builder,
    plugin_spec,
)

RULE_NAME = "server-tokens-enabled-py"

# Directive names this plugin reads. "http" must be included alongside
# "server_tokens" even though the plugin only reports on server_tokens:
# the warning for a MISSING server_tokens is keyed on "an http block
# exists but none of its children matched" — if "http" weren't in this
# list, an http block with no server_tokens inside would have no name
# match and no kept descendant, so the host would prune it away entirely,
# silently losing the exact case this plugin needs to detect. (See the
# Rust port's relevant_directives() for the same reasoning.)
RELEVANT_DIRECTIVES = ["http", "server_tokens"]

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


class WitWorld(Plugin):
    def spec(self) -> PluginSpec:
        return plugin_spec(
            RULE_NAME,
            "security",
            "Detects when server_tokens is enabled (exposes nginx version) [Python]",
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
        )

    def check(self, raw_cfg: Config, path: str) -> List[LintError]:
        # Fetch only "http"/"server_tokens" (plus their ancestors) instead of
        # the whole file: raw_cfg.all_directives_with_context() makes one host
        # call per directive in the file, while snapshot_filtered() transfers
        # everything needed in a single call proportional to what's actually
        # relevant.
        cfg = build_config_from_snapshot(raw_cfg.snapshot_filtered(RELEVANT_DIRECTIVES))
        err = error_builder(self.spec())

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
                        err.warning_at(
                            "server_tokens should be 'off' to hide nginx version",
                            directive,
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
                err.warning(
                    "server_tokens defaults to 'on', consider adding "
                    "'server_tokens off;' in http context",
                    line=http_block_line,
                    column=1,
                )
            )

        return errors
