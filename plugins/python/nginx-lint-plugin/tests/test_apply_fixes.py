"""Tests for applying fixes through the linter's own applier.

The fix constructors are pinned field-by-field against the host in
test_config_builder.py, but that compares the Fix records rather than what
they do. These tests compare outcomes, which is the check that was missing
when `insert_before` shipped as a whole-line replacement: it deleted the
directive it meant to insert before, and every unit test stayed green.
"""

import pytest

from nginx_lint_plugin import Fix
from nginx_lint_plugin.testing import apply_fixes, parse_config

NESTED = "http {\n    server_tokens on;\n}\n"


def _directive(source: str, name: str):
    cfg = parse_config(source)
    return next(
        c.directive for c in cfg.all_directives_with_context() if c.directive.is_(name)
    )


# ── The regression this module exists for ───────────────────────────


def test_insert_before_keeps_the_directive_it_inserts_before():
    d = _directive(NESTED, "server_tokens")
    result = apply_fixes(NESTED, [d.insert_before("add_header X-Test 1;")])

    assert result.skipped_invalid == 0
    assert result.content == (
        "http {\n    add_header X-Test 1;\n    server_tokens on;\n}\n"
    )


def test_a_line_based_insert_shape_would_replace_the_line():
    # Why the test above is worth having: the shape the guest constructors
    # emitted before they were matched to the host — a line number, no
    # offsets — normalizes into a whole-line replacement, so the directive
    # disappears. Comparing Fix records cannot see this; comparing output
    # can.
    broken = Fix(
        line=2,
        old_text=None,
        new_text="add_header X-Test 1;",
        delete_line=False,
        insert_after=False,
        start_offset=None,
        end_offset=None,
    )
    result = apply_fixes(NESTED, [broken])

    assert "server_tokens on;" not in result.content
    assert result.content == "http {\nadd_header X-Test 1;\n}\n"


# ── The other constructors, by outcome ──────────────────────────────


def test_replace_with_preserves_indentation():
    d = _directive(NESTED, "server_tokens")
    result = apply_fixes(NESTED, [d.replace_with("server_tokens off;")])

    assert result.applied == 1
    assert result.content == "http {\n    server_tokens off;\n}\n"


def test_insert_after_indents_to_the_directive():
    d = _directive(NESTED, "server_tokens")
    result = apply_fixes(NESTED, [d.insert_after("add_header X-Test 1;")])

    assert result.content == (
        "http {\n    server_tokens on;\n    add_header X-Test 1;\n}\n"
    )


def test_insert_before_many_and_after_many():
    d = _directive(NESTED, "server_tokens")

    before = apply_fixes(NESTED, [d.insert_before_many(["a 1;", "b 2;"])])
    assert before.content == (
        "http {\n    a 1;\n    b 2;\n    server_tokens on;\n}\n"
    )

    after = apply_fixes(NESTED, [d.insert_after_many(["a 1;", "b 2;"])])
    assert after.content == (
        "http {\n    server_tokens on;\n    a 1;\n    b 2;\n}\n"
    )


def test_delete_line_fix_removes_the_directive():
    d = _directive(NESTED, "server_tokens")
    result = apply_fixes(NESTED, [d.delete_line_fix()])

    assert result.applied == 1
    assert "server_tokens" not in result.content


# ── Multiple fixes ──────────────────────────────────────────────────


def test_two_fixes_at_different_positions_both_apply():
    source = "http {\n    server_tokens on;\n    autoindex on;\n}\n"
    cfg = parse_config(source)
    directives = {
        c.directive.name(): c.directive for c in cfg.all_directives_with_context()
    }
    result = apply_fixes(
        source,
        [
            directives["server_tokens"].replace_with("server_tokens off;"),
            directives["autoindex"].replace_with("autoindex off;"),
        ],
    )

    assert result.applied == 2
    assert result.skipped_invalid == 0
    assert result.content == "http {\n    server_tokens off;\n    autoindex off;\n}\n"


def test_overlapping_fixes_skip_rather_than_corrupt():
    # Two fixes over the same directive: the applier keeps one and drops the
    # other instead of splicing both into the same range.
    d = _directive(NESTED, "server_tokens")
    result = apply_fixes(
        NESTED,
        [d.replace_with("server_tokens off;"), d.replace_with("server_tokens build;")],
    )

    assert result.applied == 1
    assert result.content in (
        "http {\n    server_tokens off;\n}\n",
        "http {\n    server_tokens build;\n}\n",
    )


# ── Unapplicable fixes are reported, not swallowed ──────────────────


def test_unapplicable_fix_is_counted_as_skipped():
    # Offsets past the end of the file: the applier cannot use them, and
    # says so rather than silently returning the input unchanged.
    bogus = Fix(
        line=1,
        old_text=None,
        new_text="x",
        delete_line=False,
        insert_after=False,
        start_offset=10_000,
        end_offset=10_001,
    )
    result = apply_fixes(NESTED, [bogus])

    assert result.applied == 0
    assert result.skipped_invalid == 1
    assert result.content == NESTED


def test_apply_fixes_rejects_a_malformed_fix():
    with pytest.raises(ValueError):
        # line and new_text are required; a Fix missing them cannot be a fix
        from nginx_lint_plugin.testing import _native

        _native.apply_fixes_json(NESTED, '[{"old_text": null}]')
