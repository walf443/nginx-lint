"""Python SDK for nginx-lint WASM plugins.

Mirrors the TypeScript SDK at plugins/typescript/nginx-lint-plugin/:
provides guest-side Config reconstruction (`build_config_from_snapshot`)
and testing utilities (`nginx_lint_plugin.testing`).
"""

from pathlib import Path

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
    "ReconstructedConfig",
    "build_config_from_parse_output",
    "build_config_from_snapshot",
    "wit_dir",
]
