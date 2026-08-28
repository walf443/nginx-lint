"""Pytest path setup: make app.py importable.

The SDK (nginx-lint-plugin, including the wit_world bindings and the
native parser module) must be installed into the active environment:

    pip install ../nginx-lint-plugin
    # or, for SDK development: cd ../nginx-lint-plugin && maturin develop
"""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
