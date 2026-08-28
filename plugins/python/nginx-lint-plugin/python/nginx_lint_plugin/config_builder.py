"""Builds WIT-compatible Config/Directive objects from parser output.

Port of the TypeScript SDK's config-builder.ts. The parser returns a
ParseOutput with an index-based tree representation (to avoid recursive
types in WIT); this module reconstructs the method-based Directive/Config
interfaces from that flat representation, with the exact method names the
componentize-py generated bindings use (``is_``, ``first_arg_is``, ...),
so a plugin's ``check()`` runs unmodified against the result.
"""

from typing import Callable, List, Optional

from wit_world.imports import config_api, parser_types
from wit_world.imports.config_api import ConfigSnapshot
from wit_world.imports.data_types import ArgumentInfo, DirectiveData
from wit_world.imports.parser_types import ParseOutput
from wit_world.imports.types import Fix

from .include_context import IncludeContextMixin

# ── Directive wrapper ───────────────────────────────────────────────


class BuiltDirective:
    """Duck-typed stand-in for the host-backed ``config-api.directive`` resource."""

    def __init__(
        self,
        data: DirectiveData,
        resolve_block_items: Callable[[], List[config_api.ConfigItem]],
    ):
        self._data = data
        self._resolve_block_items = resolve_block_items
        self._arg_values = [a.value for a in data.args]

    def data(self) -> DirectiveData:
        return self._data

    def name(self) -> str:
        return self._data.name

    def is_(self, name: str) -> bool:
        return self._data.name == name

    def first_arg(self) -> Optional[str]:
        return self._arg_values[0] if self._arg_values else None

    def first_arg_is(self, value: str) -> bool:
        return bool(self._arg_values) and self._arg_values[0] == value

    def arg_at(self, index: int) -> Optional[str]:
        return self._arg_values[index] if 0 <= index < len(self._arg_values) else None

    def last_arg(self) -> Optional[str]:
        return self._arg_values[-1] if self._arg_values else None

    def has_arg(self, value: str) -> bool:
        return value in self._arg_values

    def arg_count(self) -> int:
        return len(self._arg_values)

    def args(self) -> List[ArgumentInfo]:
        return self._data.args

    def line(self) -> int:
        return self._data.line

    def column(self) -> int:
        return self._data.column

    def start_offset(self) -> int:
        return self._data.start_offset

    def end_offset(self) -> int:
        return self._data.end_offset

    def leading_whitespace(self) -> str:
        return self._data.leading_whitespace

    def trailing_whitespace(self) -> str:
        return self._data.trailing_whitespace

    def space_before_terminator(self) -> str:
        return self._data.space_before_terminator

    def has_block(self) -> bool:
        return self._data.has_block

    def block_items(self) -> List[config_api.ConfigItem]:
        return self._resolve_block_items()

    def block_is_raw(self) -> bool:
        return self._data.block_is_raw

    def replace_with(self, new_text: str) -> Fix:
        return Fix(
            line=self._data.line,
            old_text=None,
            new_text=new_text,
            delete_line=False,
            insert_after=False,
            start_offset=self._data.start_offset,
            end_offset=self._data.end_offset,
        )

    def delete_line_fix(self) -> Fix:
        return Fix(
            line=self._data.line,
            old_text=None,
            new_text="",
            delete_line=True,
            insert_after=False,
            start_offset=None,
            end_offset=None,
        )

    def insert_after(self, new_text: str) -> Fix:
        return Fix(
            line=self._data.line,
            old_text=None,
            new_text=new_text,
            delete_line=False,
            insert_after=True,
            start_offset=None,
            end_offset=None,
        )

    def insert_before(self, new_text: str) -> Fix:
        return Fix(
            line=self._data.line,
            old_text=None,
            new_text=new_text,
            delete_line=False,
            insert_after=False,
            start_offset=None,
            end_offset=None,
        )

    def insert_after_many(self, lines: List[str]) -> Fix:
        return Fix(
            line=self._data.line,
            old_text=None,
            new_text="\n".join(lines),
            delete_line=False,
            insert_after=True,
            start_offset=None,
            end_offset=None,
        )

    def insert_before_many(self, lines: List[str]) -> Fix:
        return Fix(
            line=self._data.line,
            old_text=None,
            new_text="\n".join(lines),
            delete_line=False,
            insert_after=False,
            start_offset=None,
            end_offset=None,
        )


# ── Resolve index-based items to ConfigItem tree ────────────────────


def _resolve_config_item(
    all_items: List[parser_types.ConfigItem], index: int
) -> config_api.ConfigItem:
    item = all_items[index]
    value = item.value
    if isinstance(value, parser_types.ConfigItemValue_DirectiveItem):
        child_indices = list(item.child_indices)
        return config_api.ConfigItem_DirectiveItem(
            value=BuiltDirective(
                value.value,
                lambda: [_resolve_config_item(all_items, i) for i in child_indices],
            )
        )
    if isinstance(value, parser_types.ConfigItemValue_CommentItem):
        return config_api.ConfigItem_CommentItem(value=value.value)
    return config_api.ConfigItem_BlankLineItem(value=value.value)


# ── Config implementations ──────────────────────────────────────────


class _BaseConfig(IncludeContextMixin):
    def __init__(
        self,
        directive_contexts: List[config_api.DirectiveContext],
        top_level_items: List[config_api.ConfigItem],
        include_context: List[str],
    ):
        self._directive_contexts = directive_contexts
        self._top_level_items = top_level_items
        self._include_context = include_context

    def all_directives_with_context(self) -> List[config_api.DirectiveContext]:
        return self._directive_contexts

    def all_directives(self) -> List[BuiltDirective]:
        return [c.directive for c in self._directive_contexts]

    def items(self) -> List[config_api.ConfigItem]:
        return self._top_level_items


class BuiltConfig(_BaseConfig):
    """Duck-typed stand-in for the host-backed ``config-api.config`` resource."""

    def __init__(
        self,
        directive_contexts: List[config_api.DirectiveContext],
        top_level_items: List[config_api.ConfigItem],
        output: ParseOutput,
    ):
        super().__init__(directive_contexts, top_level_items, output.include_context)
        self._output = output

    def snapshot(self) -> ConfigSnapshot:
        return ConfigSnapshot(
            all_items=self._output.all_items,
            top_level_indices=self._output.top_level_indices,
            include_context=self._output.include_context,
        )

    def snapshot_filtered(self, names: List[str]) -> ConfigSnapshot:
        filtered_items: List[parser_types.ConfigItem] = []
        filtered_top_level = _filter_items_by_name(
            self._output.all_items,
            list(self._output.top_level_indices),
            names,
            filtered_items,
        )
        return ConfigSnapshot(
            all_items=filtered_items,
            top_level_indices=filtered_top_level,
            include_context=self._output.include_context,
        )


class ReconstructedConfig(_BaseConfig):
    """A Config reconstructed from a snapshot.

    Unlike :class:`BuiltConfig` it has no ``snapshot()``/``snapshot_filtered()``:
    there's no live host resource behind a reconstructed config to re-fetch
    from, so a plugin that tries to re-filter an already-filtered config gets
    an AttributeError instead of silently wrong data.
    """


# ── Build Config from ParseOutput ───────────────────────────────────


def build_config_from_parse_output(output: ParseOutput) -> BuiltConfig:
    all_items = output.all_items

    directive_contexts = [
        config_api.DirectiveContext(
            directive=BuiltDirective(
                ctx.data,
                (lambda indices: lambda: [
                    _resolve_config_item(all_items, i) for i in indices
                ])(list(ctx.block_item_indices)),
            ),
            parent_stack=list(ctx.parent_stack),
            depth=ctx.depth,
        )
        for ctx in output.directives_with_context
    ]

    top_level_items = [
        _resolve_config_item(all_items, i) for i in output.top_level_indices
    ]

    return BuiltConfig(directive_contexts, top_level_items, output)


# ── Filter a flat item tree by directive name (mirrors the host's
# flatten_item_to_wit_filtered in src/plugin/component_rule.rs) ─────
#
# Keeps a directive if its own name is in `names`, OR it has at least one
# kept descendant (so ancestor context like is_inside("http") stays
# correct); comments/blank-lines are always dropped.


def _filter_items_by_name(
    all_items: List[parser_types.ConfigItem],
    indices: List[int],
    names: List[str],
    out_items: List[parser_types.ConfigItem],
) -> List[int]:
    kept: List[int] = []
    for index in indices:
        item = all_items[index]
        if not isinstance(item.value, parser_types.ConfigItemValue_DirectiveItem):
            continue
        data = item.value.value
        kept_child_indices = _filter_items_by_name(
            all_items, list(item.child_indices), names, out_items
        )
        if data.name not in names and not kept_child_indices:
            continue

        new_index = len(out_items)
        out_items.append(
            parser_types.ConfigItem(
                value=parser_types.ConfigItemValue_DirectiveItem(value=data),
                child_indices=kept_child_indices,
            )
        )
        kept.append(new_index)
    return kept


# ── Build Config from a (possibly filtered) ConfigSnapshot ──────────
#
# ConfigSnapshot does not come with a pre-computed directives-with-context
# list — the DFS walk with parent-stack tracking is done here, seeded with
# include_context exactly as the host does for all_directives_with_context().


def _collect_directive_contexts(
    all_items: List[parser_types.ConfigItem],
    indices: List[int],
    parent_stack: List[str],
    depth: int,
    results: List[config_api.DirectiveContext],
) -> None:
    for index in indices:
        item = all_items[index]
        if not isinstance(item.value, parser_types.ConfigItemValue_DirectiveItem):
            continue
        data = item.value.value
        child_indices = list(item.child_indices)

        results.append(
            config_api.DirectiveContext(
                directive=BuiltDirective(
                    data,
                    (lambda ci: lambda: [
                        _resolve_config_item(all_items, i) for i in ci
                    ])(child_indices),
                ),
                parent_stack=list(parent_stack),
                depth=depth,
            )
        )

        _collect_directive_contexts(
            all_items,
            child_indices,
            parent_stack + [data.name],
            depth + 1,
            results,
        )


def build_config_from_snapshot(snapshot: ConfigSnapshot) -> ReconstructedConfig:
    incl_ctx = list(snapshot.include_context)
    all_items = snapshot.all_items
    top_level_indices = list(snapshot.top_level_indices)

    directive_contexts: List[config_api.DirectiveContext] = []
    _collect_directive_contexts(
        all_items,
        top_level_indices,
        incl_ctx,
        len(incl_ctx),
        directive_contexts,
    )

    top_level_items = [_resolve_config_item(all_items, i) for i in top_level_indices]

    return ReconstructedConfig(directive_contexts, top_level_items, incl_ctx)
