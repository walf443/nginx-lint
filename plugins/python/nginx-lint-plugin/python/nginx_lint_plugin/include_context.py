"""Shared include-context helpers for Config implementations.

Port of the TypeScript SDK's include-context.ts: provides the
include-context portion of the Config interface so config_builder can
reuse a single implementation.
"""

from typing import List, Optional


class IncludeContextMixin:
    """Mixin implementing the include-context methods of the WIT config-api.

    Subclasses must set ``self._include_context`` (a list of parent block
    names) before use.
    """

    _include_context: List[str]

    def include_context(self) -> List[str]:
        return self._include_context

    def is_included_from(self, context: str) -> bool:
        return context in self._include_context

    def is_included_from_http(self) -> bool:
        return "http" in self._include_context

    def is_included_from_http_server(self) -> bool:
        ctx = self._include_context
        return "http" in ctx and "server" in ctx and ctx.index("http") < ctx.index("server")

    def is_included_from_http_location(self) -> bool:
        ctx = self._include_context
        return "http" in ctx and "location" in ctx and ctx.index("http") < ctx.index("location")

    def is_included_from_stream(self) -> bool:
        return "stream" in self._include_context

    def immediate_parent_context(self) -> Optional[str]:
        return self._include_context[-1] if self._include_context else None
