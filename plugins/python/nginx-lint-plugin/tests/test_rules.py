"""The ``plugin-rules`` world class built by define_rules, against the
parser-backed Config the test runner uses: which rules run for a given
``rules`` list, how the config is fetched for them, and what specs() lists.
"""

from typing import List

import pytest

from nginx_lint_plugin import (
    API_VERSION,
    LintError,
    ReconstructedConfig,
    Rule,
    define_rules,
    plugin_spec,
)
from nginx_lint_plugin.testing import PluginTestRunner, parse_config


def finding(rule: str, line: int) -> LintError:
    return LintError(
        rule=rule,
        category="test",
        message=f"{rule} fired",
        severity="warning",
        line=line,
        column=1,
        fixes=[],
    )


class ServerTokens(Rule):
    """Reports every `server_tokens on` it sees."""

    spec = plugin_spec("server-tokens", "test", "server_tokens on")
    relevant_directives = ["server_tokens"]

    def check(self, cfg: ReconstructedConfig, path: str) -> List[LintError]:
        return [
            finding("server-tokens", d.line())
            for d in cfg.all_directives()
            if d.is_("server_tokens") and d.first_arg_is("on")
        ]


class Autoindex(Rule):
    """Reports every `autoindex on`, and records what config it was given."""

    spec = plugin_spec("autoindex", "test", "autoindex on")
    relevant_directives = ["autoindex"]
    saw: List[str] = []

    def check(self, cfg: ReconstructedConfig, path: str) -> List[LintError]:
        Autoindex.saw = [d.name() for d in cfg.all_directives()]
        return [
            finding("autoindex", d.line())
            for d in cfg.all_directives()
            if d.is_("autoindex") and d.first_arg_is("on")
        ]


class CommentCounter(Rule):
    """Declares no relevant directives: reads comments, so needs the whole file."""

    spec = plugin_spec("comments", "test", "counts comments")

    def check(self, cfg: ReconstructedConfig, path: str) -> List[LintError]:
        from wit_world.imports.config_api import ConfigItem_CommentItem

        comments = [i for i in cfg.items() if isinstance(i, ConfigItem_CommentItem)]
        return [finding("comments", n + 1) for n, _ in enumerate(comments)]


SOURCE = """\
# a comment
http {
    server_tokens on;
    server {
        location / {
            autoindex on;
        }
    }
}
"""


def test_specs_lists_every_rule_in_order_with_the_api_version():
    world = define_rules(ServerTokens(), Autoindex())()
    specs = world.specs()
    assert [s.name for s in specs] == ["server-tokens", "autoindex"]
    assert all(s.api_version == API_VERSION for s in specs)


def test_check_runs_only_the_rules_asked_for_in_definition_order():
    world = define_rules(ServerTokens(), Autoindex())()
    cfg = parse_config(SOURCE)

    both = world.check(cfg, "t.conf", ["autoindex", "server-tokens"])
    assert [e.rule for e in both] == ["server-tokens", "autoindex"]

    one = world.check(cfg, "t.conf", ["autoindex"])
    assert [e.rule for e in one] == ["autoindex"]

    assert world.check(cfg, "t.conf", []) == []
    assert world.check(cfg, "t.conf", ["no-such-rule"]) == []


def test_check_prunes_to_the_union_of_the_asked_rules_directives():
    world = define_rules(ServerTokens(), Autoindex())()
    cfg = parse_config(SOURCE)

    world.check(cfg, "t.conf", ["autoindex"])
    # Pruned to autoindex and its ancestors: no server_tokens
    assert Autoindex.saw == ["http", "server", "location", "autoindex"]

    world.check(cfg, "t.conf", ["autoindex", "server-tokens"])
    # The union: server_tokens is kept for the other rule
    assert Autoindex.saw == ["http", "server_tokens", "server", "location", "autoindex"]


def test_check_fetches_the_whole_config_when_an_asked_rule_declares_no_directives():
    world = define_rules(Autoindex(), CommentCounter())()
    cfg = parse_config(SOURCE)

    errors = world.check(cfg, "t.conf", ["autoindex", "comments"])
    assert [e.rule for e in errors] == ["autoindex", "comments"]
    # Whole file, so autoindex saw everything too
    assert "server_tokens" in Autoindex.saw


def test_define_rules_refuses_what_the_host_would():
    with pytest.raises(ValueError, match="at least one rule"):
        define_rules()

    class Unnamed(ServerTokens):
        spec = plugin_spec("", "test", "unnamed")

    with pytest.raises(ValueError, match="non-empty spec.name"):
        define_rules(Unnamed())
    with pytest.raises(ValueError, match="two rules are named"):
        define_rules(ServerTokens(), ServerTokens())

    class Nothing(ServerTokens):
        relevant_directives: List[str] = []

    with pytest.raises(ValueError, match="empty relevant_directives"):
        define_rules(Nothing())


def test_runner_hands_one_rule_the_config_as_in_production():
    runner = PluginTestRunner(Autoindex())
    runner.assert_errors(SOURCE, 1)
    runner.assert_error_on_line(SOURCE, 6)
    assert Autoindex.saw == ["http", "server", "location", "autoindex"]
