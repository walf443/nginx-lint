"""Constructors for the WIT record types.

The generated dataclasses have no defaults, so building a `PluginSpec`
means spelling out all eleven fields (five of them usually `None`) and
every `LintError` repeats the plugin's own rule and category. These
helpers mirror the Rust SDK's `PluginSpec::new()` / `with_*()` and
`spec().error_builder()` so plugin code reads the same in both languages.
"""

from typing import List, Optional, Protocol

from wit_world.imports.types import Fix, LintError, PluginSpec, Severity


class _HasPosition(Protocol):
    """The part of the `directive` resource the error builder needs."""

    def line(self) -> int: ...

    def column(self) -> int: ...


def plugin_spec(
    name: str,
    category: str,
    description: str,
    *,
    api_version: Optional[str] = None,
    severity: Optional[str] = None,
    why: Optional[str] = None,
    bad_example: Optional[str] = None,
    good_example: Optional[str] = None,
    references: Optional[List[str]] = None,
    min_nginx_version: Optional[str] = None,
    max_nginx_version: Optional[str] = None,
) -> PluginSpec:
    """Build a :class:`PluginSpec`, defaulting the optional fields.

    ``api_version`` defaults to the SDK's :data:`API_VERSION`, so a plugin
    does not have to repeat (and cannot drift from) the version it was
    built against.
    """
    from . import API_VERSION

    return PluginSpec(
        name=name,
        category=category,
        description=description,
        api_version=api_version if api_version is not None else API_VERSION,
        severity=severity,
        why=why,
        bad_example=bad_example,
        good_example=good_example,
        references=references,
        min_nginx_version=min_nginx_version,
        max_nginx_version=max_nginx_version,
    )


class ErrorBuilder:
    """Creates :class:`LintError`s with the rule and category pre-filled.

    Obtain one with :func:`error_builder`, mirroring the Rust SDK's
    ``spec().error_builder()``::

        err = error_builder(self.spec())
        errors.append(err.warning_at("message", ctx.directive))
    """

    def __init__(self, rule: str, category: str):
        self._rule = rule
        self._category = category

    def _make(
        self,
        severity: Severity,
        message: str,
        line: Optional[int],
        column: Optional[int],
        fixes: Optional[List[Fix]],
    ) -> LintError:
        return LintError(
            rule=self._rule,
            category=self._category,
            message=message,
            severity=severity,
            line=line,
            column=column,
            fixes=list(fixes) if fixes else [],
        )

    def error(
        self,
        message: str,
        line: Optional[int] = None,
        column: Optional[int] = None,
        fixes: Optional[List[Fix]] = None,
    ) -> LintError:
        """Create an error at an explicit position."""
        return self._make(Severity.ERROR, message, line, column, fixes)

    def warning(
        self,
        message: str,
        line: Optional[int] = None,
        column: Optional[int] = None,
        fixes: Optional[List[Fix]] = None,
    ) -> LintError:
        """Create a warning at an explicit position."""
        return self._make(Severity.WARNING, message, line, column, fixes)

    def error_at(
        self,
        message: str,
        directive: _HasPosition,
        fixes: Optional[List[Fix]] = None,
    ) -> LintError:
        """Create an error at a directive's position."""
        return self.error(message, directive.line(), directive.column(), fixes)

    def warning_at(
        self,
        message: str,
        directive: _HasPosition,
        fixes: Optional[List[Fix]] = None,
    ) -> LintError:
        """Create a warning at a directive's position."""
        return self.warning(message, directive.line(), directive.column(), fixes)


def error_builder(spec: PluginSpec) -> ErrorBuilder:
    """Return an :class:`ErrorBuilder` keyed on a spec's rule and category."""
    return ErrorBuilder(spec.name, spec.category)
