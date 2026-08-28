"""Unit tests for the server-tokens-enabled-py plugin.

Python port of plugins/typescript/server-tokens-enabled-ts/src/plugin.test.ts —
same cases, same assertions, run with plain pytest against the real Rust
parser (via the SDK's nginx_lint_plugin._native module).
"""

from pathlib import Path

from app import RULE_NAME, WitWorld
from nginx_lint_plugin import API_VERSION
from nginx_lint_plugin.testing import PluginTestRunner, parse_config
from wit_world.imports.parser_types import ConfigItemValue_DirectiveItem

EXAMPLES_DIR = Path(__file__).resolve().parent / "examples"

plugin = WitWorld()
runner = PluginTestRunner(plugin.spec, plugin.check)


# ── spec ────────────────────────────────────────────────────────────


def test_spec_returns_valid_plugin_metadata():
    s = plugin.spec()
    assert s.name == "server-tokens-enabled-py"
    assert s.category == "security"
    # The plugin declares api_version as a literal; this assertion keeps it
    # in sync with the SDK
    assert s.api_version == API_VERSION
    assert s.severity == "warning"
    assert len(s.description) > 0
    assert s.bad_example
    assert s.good_example


# ── check ───────────────────────────────────────────────────────────


def test_detects_server_tokens_on():
    errors = runner.check_string("http {\n    server_tokens on;\n}")
    assert len(errors) == 1
    assert errors[0].rule == RULE_NAME
    assert "should be 'off'" in errors[0].message
    assert errors[0].line == 2
    assert len(errors[0].fixes) == 1


def test_no_error_when_server_tokens_off():
    runner.assert_errors("http {\n    server_tokens off;\n}", 0)


def test_no_error_when_server_tokens_build():
    runner.assert_errors("http {\n    server_tokens build;\n}", 0)


def test_warns_when_http_block_has_no_server_tokens_directive():
    errors = runner.check_string("http {\n    server {\n        listen 80;\n    }\n}")
    assert len(errors) == 1
    assert "defaults to 'on'" in errors[0].message
    assert errors[0].line == 1


def test_detects_multiple_server_tokens_on():
    runner.assert_errors(
        "http {\n    server_tokens on;\n    server {\n        server_tokens on;\n    }\n}",
        2,
    )


def test_ignores_server_tokens_in_stream_context():
    runner.assert_errors("stream {\n    server_tokens on;\n}", 0)


def test_no_warning_for_config_without_http_block():
    runner.assert_errors("events {\n    worker_connections 1024;\n}", 0)


# ── include-context ─────────────────────────────────────────────────


def test_no_warning_for_file_included_from_http_context():
    # Parent should set server_tokens
    cfg = parse_config("server {\n    listen 80;\n}", include_context=["http"])
    errors = plugin.check(cfg, "test.conf")
    assert len(errors) == 0


def test_detects_server_tokens_on_in_file_included_from_http_context():
    cfg = parse_config("server {\n    server_tokens on;\n}", include_context=["http"])
    errors = plugin.check(cfg, "test.conf")
    assert len(errors) == 1
    assert "should be 'off'" in errors[0].message
    assert errors[0].line == 2


def test_no_error_for_server_tokens_off_in_file_included_from_http_context():
    cfg = parse_config("server {\n    server_tokens off;\n}", include_context=["http"])
    errors = plugin.check(cfg, "test.conf")
    assert len(errors) == 0


def test_no_warning_for_file_included_from_http_server_context():
    cfg = parse_config(
        "location / {\n    root /var/www;\n}", include_context=["http", "server"]
    )
    errors = plugin.check(cfg, "test.conf")
    assert len(errors) == 0


def test_detects_server_tokens_on_in_file_included_from_http_server_context():
    cfg = parse_config("server_tokens on;", include_context=["http", "server"])
    errors = plugin.check(cfg, "test.conf")
    assert len(errors) == 1
    assert "should be 'off'" in errors[0].message


def test_ignores_file_included_from_stream_context():
    cfg = parse_config(
        "server {\n    server_tokens on;\n}", include_context=["stream"]
    )
    errors = plugin.check(cfg, "test.conf")
    assert len(errors) == 0


def test_examples_bad_good_conf():
    bad_conf = (EXAMPLES_DIR / "bad.conf").read_text()
    good_conf = (EXAMPLES_DIR / "good.conf").read_text()
    runner.test_examples(bad_conf, good_conf)


# ── snapshot_filtered mechanism ─────────────────────────────────────
# The check tests above exercise the plugin end-to-end; these verify that
# cfg.snapshot_filtered() itself prunes unrelated directives rather than
# silently behaving like the unfiltered path.


def test_snapshot_filtered_prunes_directives_not_in_the_requested_names():
    cfg = parse_config(
        "http {\n"
        "  gzip on;\n"
        "  server {\n"
        "    listen 80;\n"
        "    server_tokens off;\n"
        "    location / {\n"
        "      proxy_pass http://backend;\n"
        "    }\n"
        "  }\n"
        "}"
    )
    snapshot = cfg.snapshot_filtered(["http", "server_tokens"])
    names = [
        item.value.value.name
        for item in snapshot.all_items
        if isinstance(item.value, ConfigItemValue_DirectiveItem)
    ]

    # "server" survives even though it's not in `names`: it's an ancestor
    # of the kept "server_tokens" match. "gzip", "listen", "location",
    # "proxy_pass" are unrelated siblings/descendants and must be gone.
    assert set(names) == {"http", "server", "server_tokens"}


def test_snapshot_filtered_keeps_include_context():
    cfg = parse_config("server_tokens on;", include_context=["http"])
    snapshot = cfg.snapshot_filtered(["http", "server_tokens"])
    assert snapshot.include_context == ["http"]
