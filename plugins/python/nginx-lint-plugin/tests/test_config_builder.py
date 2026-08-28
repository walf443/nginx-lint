"""SDK-level tests for config_builder.

The fix-constructor tests pin the exact Fix records the SDK emits to the
host's implementations in src/plugin/component_rule.rs (replace_with /
delete_line_fix / insert_* on DirectiveResource): a plugin must get the
same --fix result whether its Config came from the host resource or from
a reconstructed snapshot.
"""

from nginx_lint_plugin import build_config_from_snapshot
from nginx_lint_plugin.testing import parse_config

NESTED = "http {\n    server_tokens on;\n}\n"


def _server_tokens(cfg):
    return next(
        ctx.directive
        for ctx in cfg.all_directives_with_context()
        if ctx.directive.is_("server_tokens")
    )


# ── Fix constructors (host parity) ──────────────────────────────────


def test_replace_with_matches_host_shape():
    d = _server_tokens(parse_config(NESTED))
    fix = d.replace_with("server_tokens off;")
    leading = d.leading_whitespace()
    assert fix.line == 0
    assert fix.delete_line is False
    assert fix.insert_after is False
    assert fix.start_offset == d.start_offset() - len(leading)
    assert fix.end_offset == d.end_offset()
    assert fix.new_text == leading + "server_tokens off;"


def test_delete_line_fix_matches_host_shape():
    d = _server_tokens(parse_config(NESTED))
    fix = d.delete_line_fix()
    assert fix.line == d.line()
    assert fix.delete_line is True
    assert fix.new_text == ""
    assert fix.start_offset is None and fix.end_offset is None


def test_insert_after_is_indented_zero_width_insert():
    d = _server_tokens(parse_config(NESTED))
    fix = d.insert_after("add_header X-Test 1;")
    # Zero-width range at the directive's end, each line indented to the
    # directive's column (column 5 -> 4 spaces)
    assert fix.start_offset == fix.end_offset == d.end_offset()
    assert fix.new_text == "\n    add_header X-Test 1;"
    assert fix.line == 0 and fix.delete_line is False and fix.insert_after is False


def test_insert_before_is_indented_zero_width_insert():
    d = _server_tokens(parse_config(NESTED))
    fix = d.insert_before("add_header X-Test 1;")
    # Zero-width range at the start of the directive's line: inserting must
    # never replace the directive itself
    assert fix.start_offset == fix.end_offset == d.start_offset() - (d.column() - 1)
    assert fix.new_text == "    add_header X-Test 1;\n"
    assert fix.line == 0 and fix.delete_line is False and fix.insert_after is False


def test_insert_after_many_indents_each_line():
    d = _server_tokens(parse_config(NESTED))
    fix = d.insert_after_many(["a 1;", "b 2;"])
    assert fix.start_offset == fix.end_offset == d.end_offset()
    assert fix.new_text == "\n    a 1;\n    b 2;"


def test_insert_before_many_indents_each_line():
    d = _server_tokens(parse_config(NESTED))
    fix = d.insert_before_many(["a 1;", "b 2;"])
    assert fix.start_offset == fix.end_offset == d.start_offset() - (d.column() - 1)
    assert fix.new_text == "    a 1;\n    b 2;\n"


def test_fix_constructors_at_top_level_have_no_indent():
    d = _server_tokens(parse_config("server_tokens on;\n"))
    assert d.insert_before("a 1;").new_text == "a 1;\n"
    assert d.insert_after("a 1;").new_text == "\na 1;"


# ── Snapshot reconstruction ─────────────────────────────────────────


def test_build_config_from_snapshot_preserves_contexts():
    cfg = parse_config(NESTED)
    rebuilt = build_config_from_snapshot(cfg.snapshot())
    original = [
        (c.directive.name(), c.parent_stack, c.depth)
        for c in cfg.all_directives_with_context()
    ]
    reconstructed = [
        (c.directive.name(), c.parent_stack, c.depth)
        for c in rebuilt.all_directives_with_context()
    ]
    assert reconstructed == original == [
        ("http", [], 0),
        ("server_tokens", ["http"], 1),
    ]


def test_build_config_from_snapshot_seeds_include_context():
    cfg = parse_config("server {\n    listen 80;\n}", include_context=["http"])
    rebuilt = build_config_from_snapshot(cfg.snapshot())
    contexts = [
        (c.directive.name(), c.parent_stack, c.depth)
        for c in rebuilt.all_directives_with_context()
    ]
    # The include context seeds the parent stack and depth, exactly as the
    # host does for all_directives_with_context()
    assert contexts == [
        ("server", ["http"], 1),
        ("listen", ["http", "server"], 2),
    ]
    assert rebuilt.is_included_from_http()


def test_build_config_from_filtered_snapshot():
    cfg = parse_config(
        "http {\n    gzip on;\n    server {\n        server_tokens on;\n    }\n}"
    )
    rebuilt = build_config_from_snapshot(
        cfg.snapshot_filtered(["http", "server_tokens"])
    )
    names = [c.directive.name() for c in rebuilt.all_directives_with_context()]
    assert names == ["http", "server", "server_tokens"]
    # Reconstructed configs deliberately have no snapshot()/snapshot_filtered()
    assert not hasattr(rebuilt, "snapshot")
