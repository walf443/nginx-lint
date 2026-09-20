"""The ``plugin-rules`` world from Python: a component carrying one or more
rules.

A plugin module defines each rule as a :class:`Rule` and binds the world's
class to the rules with :func:`define_rules`::

    class ServerTokens(Rule):
        spec = plugin_spec("server-tokens", "security", "...")
        relevant_directives = ["http", "server_tokens"]

        def check(self, cfg, path):
            ...

    WitWorld = define_rules(ServerTokens())

componentize-py looks the class up by that name in the app module, so the
assignment has to be to ``WitWorld``. The host loads the component as one
rule per entry, each with its own name, documentation and configuration;
``check`` reconstructs the config once and runs the rules the host asked
for over it.
"""

from abc import ABC, abstractmethod
from typing import ClassVar, List, Optional, Sequence, Type

from wit_world import WitWorld
from wit_world.imports.config_api import Config
from wit_world.imports.types import LintError, PluginSpec

from .config_builder import ReconstructedConfig, build_config_from_snapshot


class Rule(ABC):
    """One lint rule.

    A subclass sets ``spec`` (a :class:`PluginSpec`, usually from
    :func:`~nginx_lint_plugin.plugin_spec`, which fills in the SDK's API
    version) and implements :meth:`check`. ``relevant_directives`` names the
    directives the rule reads, if it reads a fixed, known set: the config it
    gets is then pruned to those directives plus the ancestor blocks needed
    for context queries, which is far cheaper to transfer and rebuild than
    the whole file. A rule that warns when a directive is *missing* inside a
    block has to list that block's name too: with none of the listed names
    inside it, the block is pruned away with the evidence. Leave it ``None``
    to get the whole config, which is also the only way to see comments and
    blank lines.

    The list is a floor, not a ceiling: when the host asks for several rules
    at once, the config is pruned to the union of their lists, so a rule can
    see directives it did not ask for. Match by name; do not read anything
    into a block being empty or a list having a certain length.
    """

    spec: ClassVar[PluginSpec]
    relevant_directives: ClassVar[Optional[List[str]]] = None

    @abstractmethod
    def check(self, cfg: ReconstructedConfig, path: str) -> List[LintError]:
        """Inspect the config and report findings under ``spec.name``.

        When the host asks for several rules at once they share one config,
        so treat it as read-only.
        """
        raise NotImplementedError


def define_rules(*rules: Rule) -> Type[WitWorld]:
    """Build the ``plugin-rules`` world class for these rules, in this order.

    Its ``check`` runs only the rules named in its ``rules`` argument (the
    host asks only for enabled ones), ignores names it does not carry, and
    returns nothing for an empty list. The config is fetched once: pruned to
    the union of the asked rules' ``relevant_directives`` when every asked
    rule declares them, and whole otherwise.

    Raises :class:`ValueError` on an empty list, a rule without a name, two
    rules with one name, or an empty ``relevant_directives`` (which would
    fetch an empty config) — the host would refuse the component for the
    first three, and a plugin's own tests are the earlier place to hear it.
    """
    if not rules:
        raise ValueError("define_rules needs at least one rule")
    seen = set()
    for rule in rules:
        name = rule.spec.name
        if not name:
            raise ValueError("every rule needs a non-empty spec.name")
        if name in seen:
            raise ValueError(f"two rules are named {name!r}")
        seen.add(name)
        if rule.relevant_directives is not None and not rule.relevant_directives:
            raise ValueError(
                f"rule {name!r} has an empty relevant_directives; "
                "set it to None to read the whole config"
            )
    ordered: List[Rule] = list(rules)

    class Rules(WitWorld):
        def specs(self) -> List[PluginSpec]:
            return [rule.spec for rule in ordered]

        def check(self, cfg: Config, path: str, rules: List[str]) -> List[LintError]:
            asked = [rule for rule in ordered if rule.spec.name in rules]
            if not asked:
                return []
            config = reconstruct_for(asked, cfg)
            errors: List[LintError] = []
            for rule in asked:
                errors.extend(rule.check(config, path))
            return errors

    return Rules


def reconstruct_for(rules: Sequence[Rule], cfg: Config) -> ReconstructedConfig:
    """Fetch the config once for these rules: pruned to the union of their
    ``relevant_directives`` when every one declares them, whole otherwise."""
    relevant = set()
    for rule in rules:
        if rule.relevant_directives is None:
            return build_config_from_snapshot(cfg.snapshot())
        relevant.update(rule.relevant_directives)
    return build_config_from_snapshot(cfg.snapshot_filtered(sorted(relevant)))
