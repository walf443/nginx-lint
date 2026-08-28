"""Testing utilities for Python nginx-lint plugins.

Port of the TypeScript SDK's testing entry: provides ``parse_config()``
to parse real nginx configuration strings into WIT-compatible Config
objects, and ``PluginTestRunner`` for assertion-based testing.

Parsing uses the ``nginx_lint_plugin._native`` module (the Rust parser
compiled into this wheel by maturin), so tests exercise the same parser
the production linter uses. Usage:

    from nginx_lint_plugin.testing import parse_config, PluginTestRunner

    plugin = WitWorld()
    runner = PluginTestRunner(plugin.spec, plugin.check)
    runner.assert_errors("http { server_tokens on; }", 1)
"""

import json
from typing import Callable, List, Optional, cast

from wit_world.imports.config_api import Config
from wit_world.imports import parser_types
from wit_world.imports.data_types import (
    ArgumentInfo,
    ArgumentType,
    BlankLineInfo,
    CommentInfo,
    DirectiveData,
)
from wit_world.imports.parser_types import ParseOutput
from wit_world.imports.types import LintError, PluginSpec

from . import _native
from .config_builder import build_config_from_parse_output

# ── JSON → generated dataclasses ────────────────────────────────────

_ARGUMENT_TYPES = {
    "literal": ArgumentType.LITERAL,
    "quoted-string": ArgumentType.QUOTED_STRING,
    "single-quoted-string": ArgumentType.SINGLE_QUOTED_STRING,
    "variable": ArgumentType.VARIABLE,
}


def _argument_info_from_json(d: dict) -> ArgumentInfo:
    return ArgumentInfo(
        value=d["value"],
        raw=d["raw"],
        arg_type=_ARGUMENT_TYPES[d["arg_type"]],
        line=d["line"],
        column=d["column"],
        start_offset=d["start_offset"],
        end_offset=d["end_offset"],
    )


def _directive_data_from_json(d: dict) -> DirectiveData:
    return DirectiveData(
        name=d["name"],
        args=[_argument_info_from_json(a) for a in d["args"]],
        line=d["line"],
        column=d["column"],
        start_offset=d["start_offset"],
        end_offset=d["end_offset"],
        end_line=d["end_line"],
        end_column=d["end_column"],
        leading_whitespace=d["leading_whitespace"],
        trailing_whitespace=d["trailing_whitespace"],
        space_before_terminator=d["space_before_terminator"],
        has_block=d["has_block"],
        block_is_raw=d["block_is_raw"],
        block_raw_content=d["block_raw_content"],
        closing_brace_leading_whitespace=d["closing_brace_leading_whitespace"],
        block_trailing_whitespace=d["block_trailing_whitespace"],
        trailing_comment_text=d["trailing_comment_text"],
        name_end_column=d["name_end_column"],
        name_end_offset=d["name_end_offset"],
        block_start_line=d["block_start_line"],
        block_start_column=d["block_start_column"],
        block_start_offset=d["block_start_offset"],
    )


def _config_item_from_json(d: dict) -> parser_types.ConfigItem:
    value = d["value"]
    tag = value["tag"]
    val = value["val"]
    if tag == "directive-item":
        item_value = parser_types.ConfigItemValue_DirectiveItem(
            value=_directive_data_from_json(val)
        )
    elif tag == "comment-item":
        item_value = parser_types.ConfigItemValue_CommentItem(
            value=CommentInfo(
                text=val["text"],
                line=val["line"],
                column=val["column"],
                leading_whitespace=val["leading_whitespace"],
                trailing_whitespace=val["trailing_whitespace"],
                start_offset=val["start_offset"],
                end_offset=val["end_offset"],
            )
        )
    else:
        item_value = parser_types.ConfigItemValue_BlankLineItem(
            value=BlankLineInfo(
                line=val["line"],
                content=val["content"],
                start_offset=val["start_offset"],
            )
        )
    return parser_types.ConfigItem(value=item_value, child_indices=d["child_indices"])


def _parse_output_from_json(d: dict) -> ParseOutput:
    return ParseOutput(
        directives_with_context=[
            parser_types.DirectiveContext(
                data=_directive_data_from_json(c["data"]),
                block_item_indices=c["block_item_indices"],
                parent_stack=c["parent_stack"],
                depth=c["depth"],
            )
            for c in d["directives_with_context"]
        ],
        include_context=d["include_context"],
        all_items=[_config_item_from_json(i) for i in d["all_items"]],
        top_level_indices=d["top_level_indices"],
    )


# ── High-level parse ────────────────────────────────────────────────


def parse_config(
    source: str, include_context: Optional[List[str]] = None
) -> Config:
    """Parse an nginx configuration string into a WIT-compatible Config.

    Uses the native nginx-lint-parser module for parsing identical to the
    production Rust parser. Raises ValueError on parse errors.

    The result is a :class:`BuiltConfig`, which reproduces the host-backed
    `config` resource's methods without inheriting from it (the generated
    class is a stub whose bodies raise). It is annotated as ``Config`` so
    that a plugin's ``check(cfg: Config, ...)`` type-checks when called
    with a parsed config; the substitution is contained here.
    """
    raw = _native.parse_config_json(source, include_context or [])
    built = build_config_from_parse_output(_parse_output_from_json(json.loads(raw)))
    return cast(Config, built)


# ── Test runner ─────────────────────────────────────────────────────

SpecFn = Callable[[], PluginSpec]
CheckFn = Callable[..., List[LintError]]


class PluginTestRunner:
    """Test runner for Python nginx-lint plugins.

    Mirrors the Rust and TypeScript ``PluginTestRunner`` APIs:

        plugin = WitWorld()
        runner = PluginTestRunner(plugin.spec, plugin.check)
        runner.assert_errors("http { server_tokens on; }", 1)
        runner.assert_errors("http { server_tokens off; }", 0)
    """

    def __init__(self, spec: SpecFn, check: CheckFn):
        self._spec = spec
        self._check = check

    def check_string(
        self, content: str, include_context: Optional[List[str]] = None
    ) -> List[LintError]:
        """Parse and check a config string, returning only this rule's errors."""
        cfg = parse_config(content, include_context)
        errors = self._check(cfg, "test.conf")
        rule_name = self._spec().name
        return [e for e in errors if e.rule == rule_name]

    def assert_errors(self, content: str, count: int) -> None:
        """Assert the config produces exactly `count` errors from this rule."""
        errors = self.check_string(content)
        if len(errors) != count:
            raise AssertionError(
                f'Expected {count} error(s) from "{self._spec().name}", '
                f"got {len(errors)}: {errors!r}"
            )

    def assert_error_on_line(self, content: str, line: int) -> None:
        """Assert the config produces at least one error on `line`."""
        errors = self.check_string(content)
        if not any(e.line == line for e in errors):
            lines = [e.line for e in errors]
            raise AssertionError(
                f'Expected error on line {line} from "{self._spec().name}", '
                f"got errors on lines: {lines!r}"
            )

    def test_examples(self, bad_conf: str, good_conf: str) -> None:
        """Assert `bad_conf` produces errors and `good_conf` produces none."""
        rule_name = self._spec().name

        bad_errors = self.check_string(bad_conf)
        if not bad_errors:
            raise AssertionError(
                f'bad.conf should produce at least one "{rule_name}" error, got none'
            )

        good_errors = self.check_string(good_conf)
        if good_errors:
            raise AssertionError(
                f'good.conf should produce no "{rule_name}" errors, got: {good_errors!r}'
            )
