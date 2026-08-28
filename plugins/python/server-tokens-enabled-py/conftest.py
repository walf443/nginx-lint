"""Pytest path setup: make app.py, the SDK, and the generated bindings importable.

The SDK directory provides both `nginx_lint_plugin` and the committed
componentize-py bindings (`wit_world`, `componentize_py_types`).
The native parser module (`nginx_lint_parser_py`) must be installed into
the active environment: cd crates/nginx-lint-parser-py && maturin develop
"""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT.parent / "nginx-lint-plugin"))
