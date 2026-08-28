"""Python SDK for nginx-lint WASM plugins.

Mirrors the TypeScript SDK at plugins/typescript/nginx-lint-plugin/:
provides guest-side Config reconstruction (`build_config_from_snapshot`),
constructors for the WIT record types, and testing utilities
(`nginx_lint_plugin.testing`).

Everything a plugin needs is re-exported here, so plugin code imports from
this package rather than reaching into the generated bindings:

    from nginx_lint_plugin import Plugin, error_builder, plugin_spec

    class WitWorld(Plugin):
        def spec(self):
            return plugin_spec("my-rule", "style", "...", severity="warning")
"""

from pathlib import Path

# The generated world protocol, under a name that says what it is. A plugin
# subclasses this; the subclass must still be named WitWorld for
# componentize-py to find it.
from wit_world import WitWorld as Plugin
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

# Keep in sync with API_VERSION in crates/nginx-lint-plugin/src/types.rs
API_VERSION = "1.2"


def wit_dir() -> Path:
    """Return the directory holding the bundled `nginx-lint-plugin.wit`.

    componentize-py needs the interface definition to build a plugin, so the
    SDK ships it. Pass this to its `-d` option:

        componentize-py -d "$(python -c 'import nginx_lint_plugin as p; print(p.wit_dir())')" \\
            -w plugin componentize app -o plugin.wasm --stub-wasi
    """
    return Path(__file__).parent / "wit"


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
    "Plugin",
    "PluginSpec",
    "ReconstructedConfig",
    "Severity",
    "build_config_from_parse_output",
    "build_config_from_snapshot",
    "error_builder",
    "plugin_spec",
    "wit_dir",
]
