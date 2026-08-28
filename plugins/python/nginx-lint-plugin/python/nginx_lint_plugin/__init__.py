"""Python SDK for nginx-lint WASM plugins.

Mirrors the TypeScript SDK at plugins/typescript/nginx-lint-plugin/:
provides guest-side Config reconstruction (`build_config_from_snapshot`)
and testing utilities (`nginx_lint_plugin.testing`).
"""

from .config_builder import (
    ReconstructedConfig,
    build_config_from_parse_output,
    build_config_from_snapshot,
)

# Keep in sync with API_VERSION in crates/nginx-lint-plugin/src/types.rs
API_VERSION = "1.2"

__all__ = [
    "API_VERSION",
    "ReconstructedConfig",
    "build_config_from_parse_output",
    "build_config_from_snapshot",
]
