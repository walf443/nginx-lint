"""The wasm parser and fix applier behind ``nginx_lint_plugin.testing``."""

from importlib.metadata import version

import pytest

from nginx_lint_plugin import ArgumentType
from nginx_lint_plugin import _testkit
from nginx_lint_plugin.testing import parse_config


@pytest.mark.parametrize("module", ["parser", "fixer"])
def test_modules_match_the_package_version(module):
    # The modules carry the version of the crates they were built from, and
    # the package shares the repository's version: a wheel built after a
    # version bump without `make build-testkit-wasm` fails here instead of
    # testing plugins against the previous release's parser.
    assert _testkit.version(module) == version("nginx-lint-plugin")


def test_a_parse_error_raises_value_error():
    with pytest.raises(ValueError):
        parse_config("http {")


def test_every_argument_type_converts():
    cfg = parse_config("""set $a "x" 'y' z;\n""")
    (ctx,) = cfg.all_directives_with_context()
    assert [a.arg_type for a in ctx.directive.args()] == [
        ArgumentType.VARIABLE,
        ArgumentType.QUOTED_STRING,
        ArgumentType.SINGLE_QUOTED_STRING,
        ArgumentType.LITERAL,
    ]


def test_every_item_kind_converts():
    cfg = parse_config("# a comment\n\nhttp {\n}\n")
    kinds = [type(item).__name__ for item in cfg.items()]
    assert kinds == [
        "ConfigItem_CommentItem",
        "ConfigItem_BlankLineItem",
        "ConfigItem_DirectiveItem",
    ]


def test_include_context_reaches_the_parser():
    cfg = parse_config("server_tokens on;", include_context=["http", "server"])
    assert cfg.include_context() == ["http", "server"]
    (ctx,) = cfg.all_directives_with_context()
    assert ctx.parent_stack == ["http", "server"]
