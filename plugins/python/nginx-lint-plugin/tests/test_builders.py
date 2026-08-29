"""Tests for the WIT record constructors."""

import nginx_lint_plugin
from nginx_lint_plugin import API_VERSION, Severity, error_builder, plugin_spec
from nginx_lint_plugin.testing import parse_config


def _directive(source: str, name: str):
    cfg = parse_config(source)
    return next(
        c.directive for c in cfg.all_directives_with_context() if c.directive.is_(name)
    )


def test_plugin_spec_defaults_optional_fields():
    spec = plugin_spec("my-rule", "style", "desc")
    assert (spec.name, spec.category, spec.description) == ("my-rule", "style", "desc")
    # Defaults to the SDK's version rather than making plugins repeat it
    assert spec.api_version == API_VERSION
    assert spec.severity is None
    assert spec.why is None
    assert spec.bad_example is None
    assert spec.good_example is None
    assert spec.references is None
    assert spec.min_nginx_version is None
    assert spec.max_nginx_version is None


def test_plugin_spec_passes_through_optional_fields():
    spec = plugin_spec(
        "my-rule",
        "style",
        "desc",
        severity="warning",
        why="because",
        references=["https://example.com"],
        min_nginx_version="1.25.1",
    )
    assert spec.severity == "warning"
    assert spec.why == "because"
    assert spec.references == ["https://example.com"]
    assert spec.min_nginx_version == "1.25.1"


def test_error_builder_fills_rule_and_category_from_spec():
    err = error_builder(plugin_spec("my-rule", "style", "desc"))
    e = err.warning("message", line=3, column=5)
    assert (e.rule, e.category) == ("my-rule", "style")
    assert e.severity == Severity.WARNING
    assert (e.line, e.column) == (3, 5)
    assert e.fixes == []


def test_error_builder_severities():
    err = error_builder(plugin_spec("my-rule", "style", "desc"))
    assert err.error("m").severity == Severity.ERROR
    assert err.warning("m").severity == Severity.WARNING


def test_error_builder_at_uses_directive_position():
    d = _directive("http {\n    server_tokens on;\n}", "server_tokens")
    err = error_builder(plugin_spec("my-rule", "security", "desc"))

    warning = err.warning_at("message", d)
    assert (warning.line, warning.column) == (d.line(), d.column()) == (2, 5)
    assert warning.severity == Severity.WARNING

    error = err.error_at("message", d)
    assert (error.line, error.column) == (2, 5)
    assert error.severity == Severity.ERROR


def test_error_builder_attaches_fixes():
    d = _directive("http {\n    server_tokens on;\n}", "server_tokens")
    err = error_builder(plugin_spec("my-rule", "security", "desc"))
    fix = d.replace_with("server_tokens off;")

    assert err.warning_at("m", d, fixes=[fix]).fixes == [fix]
    # Each error gets its own list rather than sharing one
    assert err.warning("m").fixes is not err.warning("m").fixes


def test_sdk_reexports_everything_a_plugin_needs():
    # Plugin code should not have to reach into the generated bindings
    for name in ("Plugin", "Config", "LintError", "PluginSpec", "Fix", "Severity"):
        assert hasattr(nginx_lint_plugin, name), name
