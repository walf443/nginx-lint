"""Python SDK for nginx-lint WASM plugins.

Mirrors the TypeScript SDK at plugins/typescript/nginx-lint-plugin/:
provides guest-side Config reconstruction (`build_config_from_snapshot`),
constructors for the WIT record types, and testing utilities
(`nginx_lint_plugin.testing`).

Everything a plugin needs is re-exported here, so plugin code imports from
this package rather than reaching into the generated bindings:

    from nginx_lint_plugin import Rule, define_rules, error_builder, plugin_spec

    class MyRule(Rule):
        spec = plugin_spec("my-rule", "style", "...", severity="warning")
        relevant_directives = ["some_directive"]

        def check(self, cfg, path):
            ...

    WitWorld = define_rules(MyRule())
"""

from pathlib import Path

from wit_world.imports.config_api import (
    Config,
    ConfigItem,
    ConfigSnapshot,
    Directive,
    DirectiveContext,
)
from wit_world.imports.data_types import (
    ArgumentInfo,
    ArgumentType,
    BlankLineInfo,
    CommentInfo,
    DirectiveData,
)
from wit_world.imports.types import Fix, LintError, PluginSpec, Severity

from .builders import ErrorBuilder, error_builder, plugin_spec
from .config_builder import (
    ReconstructedConfig,
    build_config_from_parse_output,
    build_config_from_snapshot,
)
from .rules import Rule, define_rules

# Keep in sync with API_VERSION in crates/nginx-lint-plugin/src/types.rs
API_VERSION = "1.2"


def wit_dir() -> Path:
    """Return the directory holding the bundled `nginx-lint-plugin.wit`.

    componentize-py needs the interface definition to build a plugin, so the
    SDK ships it. Pass this to its `-d` option:

        componentize-py -d "$(python -c 'import nginx_lint_plugin as p; print(p.wit_dir())')" \\
            -w plugin-rules componentize app -o plugin.wasm --stub-wasi

    Raises FileNotFoundError if the installed package has no bundled WIT,
    which happens when it was built without the Makefile's `copy-wit` step
    (the directory is generated, not committed). Failing here names the
    cause; returning the path would surface it as an opaque componentize-py
    error about a missing directory.
    """
    directory = Path(__file__).parent / "wit"
    if not directory.is_dir():
        raise FileNotFoundError(
            f"nginx-lint-plugin was installed without its bundled WIT ({directory} "
            "does not exist). Reinstall it with `make install` from the SDK "
            "directory, which copies the WIT in before building."
        )
    return directory


__all__ = [
    "API_VERSION",
    "ArgumentInfo",
    "ArgumentType",
    "BlankLineInfo",
    "CommentInfo",
    "Config",
    "ConfigItem",
    "ConfigSnapshot",
    "Directive",
    "DirectiveContext",
    "DirectiveData",
    "ErrorBuilder",
    "Fix",
    "LintError",
    "PluginSpec",
    "ReconstructedConfig",
    "Rule",
    "Severity",
    "build_config_from_parse_output",
    "build_config_from_snapshot",
    "define_rules",
    "error_builder",
    "plugin_spec",
    "wit_dir",
]
