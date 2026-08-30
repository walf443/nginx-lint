/**
 * Builds WIT-compatible Config/Directive objects from parser component output.
 *
 * The parser WASM component returns a ParseOutput with an index-based tree
 * representation (to avoid recursive types in WIT). This module reconstructs
 * the method-based Directive/Config interfaces from that flat representation.
 */

import type {
  Config,
  ConfigItem,
  ConfigSnapshot,
  Directive,
  DirectiveContext,
} from "./generated/interfaces/nginx-lint-plugin-config-api.js";
import type {
  DirectiveData,
  ArgumentInfo,
} from "./generated/interfaces/nginx-lint-plugin-data-types.js";
import type { Fix } from "./generated/interfaces/nginx-lint-plugin-types.js";
import { makeIncludeContextMethods } from "./include-context.js";
import type {
  ParseOutput,
  ConfigItem as ParserConfigItem,
  DirectiveContext as ParserDirectiveContext,
} from "../wasm/parser/interfaces/nginx-lint-plugin-parser-types.js";

// ── Wrap DirectiveData with Directive interface ─────────────────────

/** Mirror of the host's `make_range_fix`: a zero-`line` range-based fix. */
function rangeFix(start: number, end: number, newText: string): Fix {
  return {
    line: 0, oldText: undefined, newText,
    deleteLine: false, insertAfter: false,
    startOffset: start, endOffset: end,
  };
}

/** The indent the host prefixes to each inserted line: `" ".repeat(column - 1)`. */
function indentOf(data: DirectiveData): string {
  return " ".repeat(Math.max(data.column - 1, 0));
}

/** Offset of the start of the directive's line, as the host computes it. */
function lineStartOffset(data: DirectiveData): number {
  return Math.max(data.startOffset - (data.column - 1), 0);
}

/**
 * Duck-typed stand-in for the host-backed `config-api.directive` resource.
 *
 * A class rather than an object literal of closures, so that it matches the
 * shape of the real resource: jco generates that as a class, and the literal
 * made `Object.keys()`, spread and destructured methods behave differently
 * depending on which path a plugin's Config came from.
 *
 * Not a performance change, despite the shape suggesting one. Profiling a
 * 10k-line config found per-check time dominated by the component boundary,
 * not by guest-side allocation: lowering the returned LintErrors accounted
 * for over half of it and lifting the snapshot for most of the rest, while
 * everything this class does — walking the tree, allocating the directive
 * objects — came to a few milliseconds in total.
 */
class BuiltDirective implements Directive {
  readonly #data: DirectiveData;
  readonly #resolveBlockItems: () => ConfigItem[];
  readonly #argValues: string[];

  constructor(data: DirectiveData, resolveBlockItems: () => ConfigItem[]) {
    this.#data = data;
    this.#resolveBlockItems = resolveBlockItems;
    this.#argValues = data.args.map((a) => a.value);
  }

  data(): DirectiveData { return this.#data; }
  name(): string { return this.#data.name; }
  is(name: string): boolean { return this.#data.name === name; }
  firstArg(): string | undefined { return this.#argValues[0] ?? undefined; }
  firstArgIs(value: string): boolean { return this.#argValues[0] === value; }
  argAt(index: number): string | undefined { return this.#argValues[index] ?? undefined; }
  lastArg(): string | undefined {
    return this.#argValues.length > 0 ? this.#argValues[this.#argValues.length - 1] : undefined;
  }
  hasArg(value: string): boolean { return this.#argValues.includes(value); }
  argCount(): number { return this.#argValues.length; }
  args(): ArgumentInfo[] { return this.#data.args; }
  line(): number { return this.#data.line; }
  column(): number { return this.#data.column; }
  startOffset(): number { return this.#data.startOffset; }
  endOffset(): number { return this.#data.endOffset; }
  leadingWhitespace(): string { return this.#data.leadingWhitespace; }
  trailingWhitespace(): string { return this.#data.trailingWhitespace; }
  spaceBeforeTerminator(): string { return this.#data.spaceBeforeTerminator; }
  hasBlock(): boolean { return this.#data.hasBlock; }
  blockItems(): ConfigItem[] { return this.#resolveBlockItems(); }
  blockIsRaw(): boolean { return this.#data.blockIsRaw; }

  // The fix constructors below must produce byte-identical Fix records to
  // the host's implementations in src/plugin/component_rule.rs
  // (replace_with / delete_line_fix / insert_* on DirectiveResource), so a
  // plugin gets the same --fix result whether its Config came from the
  // host resource or from a reconstructed snapshot.
  replaceWith(newText: string): Fix {
    const leading = this.#data.leadingWhitespace;
    return rangeFix(
      this.#data.startOffset - leading.length,
      this.#data.endOffset,
      leading + newText,
    );
  }
  deleteLineFix(): Fix {
    return {
      line: this.#data.line, oldText: undefined, newText: "",
      deleteLine: true, insertAfter: false,
      startOffset: undefined, endOffset: undefined,
    };
  }
  insertAfter(newText: string): Fix {
    const offset = this.#data.endOffset;
    return rangeFix(offset, offset, `\n${indentOf(this.#data)}${newText}`);
  }
  insertBefore(newText: string): Fix {
    const offset = lineStartOffset(this.#data);
    return rangeFix(offset, offset, `${indentOf(this.#data)}${newText}\n`);
  }
  insertAfterMany(lines: string[]): Fix {
    const indent = indentOf(this.#data);
    const text = lines.map((line) => `\n${indent}${line}`).join("");
    const offset = this.#data.endOffset;
    return rangeFix(offset, offset, text);
  }
  insertBeforeMany(lines: string[]): Fix {
    const indent = indentOf(this.#data);
    const text = lines.map((line) => `${indent}${line}\n`).join("");
    const offset = lineStartOffset(this.#data);
    return rangeFix(offset, offset, text);
  }
}

function wrapDirective(
  data: DirectiveData,
  resolveBlockItems: () => ConfigItem[],
): Directive {
  return new BuiltDirective(data, resolveBlockItems);
}

// ── Resolve index-based items to ConfigItem tree ────────────────────

function resolveConfigItem(
  allItems: ParserConfigItem[],
  index: number,
): ConfigItem {
  const item = allItems[index];
  if (item.value.tag === "directive-item") {
    const data = item.value.val;
    const childIndices = item.childIndices;
    return {
      tag: "directive-item",
      val: wrapDirective(data, () =>
        Array.from(childIndices).map((i) => resolveConfigItem(allItems, i)),
      ),
    };
  }
  if (item.value.tag === "comment-item") {
    return { tag: "comment-item", val: item.value.val };
  }
  return { tag: "blank-line-item", val: item.value.val };
}

// ── Build Config from ParseOutput ───────────────────────────────────

export function buildConfigFromParseOutput(output: ParseOutput): Config {
  const inclCtx = output.includeContext;
  const allItems = output.allItems;

  const directiveContexts: DirectiveContext[] = output.directivesWithContext.map(
    (ctx: ParserDirectiveContext) => ({
      directive: wrapDirective(ctx.data, () =>
        Array.from(ctx.blockItemIndices).map((i) => resolveConfigItem(allItems, i)),
      ),
      parentStack: ctx.parentStack,
      depth: ctx.depth,
    }),
  );

  const topLevelItems: ConfigItem[] = Array.from(output.topLevelIndices).map(
    (i) => resolveConfigItem(allItems, i),
  );

  return {
    allDirectivesWithContext() { return directiveContexts; },
    allDirectives() { return directiveContexts.map((c) => c.directive); },
    items() { return topLevelItems; },
    snapshot() {
      return { allItems, topLevelIndices: output.topLevelIndices, includeContext: inclCtx };
    },
    snapshotFiltered(names: string[]) {
      const filteredItems: ParserConfigItem[] = [];
      const filteredTopLevel = filterItemsByName(
        allItems,
        Array.from(output.topLevelIndices),
        names,
        filteredItems,
      );
      return {
        allItems: filteredItems,
        topLevelIndices: Uint32Array.from(filteredTopLevel),
        includeContext: inclCtx,
      };
    },
    ...makeIncludeContextMethods(inclCtx),
  } as Config;
}

// ── Filter a flat item tree by directive name (mirrors the host's
// flatten_item_to_wit_filtered in src/plugin/component_rule.rs) ─────
//
// Keeps a directive if its own name is in `names`, OR it has at least one
// kept descendant (so ancestor context like is_inside("http") stays
// correct); comments/blank-lines are always dropped. Used by the parser-
// output-backed test mock's snapshotFiltered() so plugin tests exercise the
// same semantics the real host applies.

function filterItemsByName(
  allItems: ParserConfigItem[],
  indices: number[],
  names: string[],
  outItems: ParserConfigItem[],
): number[] {
  const kept: number[] = [];
  for (const index of indices) {
    const item = allItems[index];
    if (item.value.tag !== "directive-item") continue;
    const data = item.value.val;
    const keptChildIndices = filterItemsByName(
      allItems,
      Array.from(item.childIndices),
      names,
      outItems,
    );
    if (!names.includes(data.name) && keptChildIndices.length === 0) continue;

    const newIndex = outItems.length;
    outItems.push({
      value: { tag: "directive-item", val: data },
      childIndices: Uint32Array.from(keptChildIndices),
    });
    kept.push(newIndex);
  }
  return kept;
}

// ── Build Config from a (possibly filtered) ConfigSnapshot ──────────
//
// Unlike ParseOutput (produced by the parser component for testing),
// ConfigSnapshot (produced by the host's Config.snapshot()/snapshotFiltered())
// does not come with a pre-computed directives-with-context list — the DFS
// walk with parent-stack tracking has to be done here, seeded with
// includeContext exactly as the host does for allDirectivesWithContext().
// Works for both the full snapshot() and a snapshotFiltered(names) result:
// the walk itself doesn't know or care whether the tree was pruned.

function collectDirectiveContexts(
  allItems: ParserConfigItem[],
  indices: number[],
  parentStack: string[],
  depth: number,
  results: DirectiveContext[],
): void {
  for (const index of indices) {
    const item = allItems[index];
    if (item.value.tag !== "directive-item") continue;
    const data = item.value.val;
    const childIndices = Array.from(item.childIndices);

    results.push({
      directive: wrapDirective(data, () =>
        childIndices.map((i) => resolveConfigItem(allItems, i)),
      ),
      parentStack: [...parentStack],
      depth,
    });

    collectDirectiveContexts(
      allItems,
      childIndices,
      [...parentStack, data.name],
      depth + 1,
      results,
    );
  }
}

/**
 * A {@link Config} reconstructed from a snapshot, minus `snapshot()` and
 * `snapshotFiltered()` themselves: {@link buildConfigFromSnapshot} doesn't
 * implement them (there's no live host resource behind a reconstructed
 * config to re-fetch from), so calling either on the result would throw at
 * runtime. Omitting them from the type makes that a compile-time error
 * instead of a runtime surprise for a plugin that tries to re-filter an
 * already-filtered config.
 */
export type ReconstructedConfig = Omit<Config, "snapshot" | "snapshotFiltered">;

export function buildConfigFromSnapshot(snapshot: ConfigSnapshot): ReconstructedConfig {
  const inclCtx = snapshot.includeContext;
  const allItems = snapshot.allItems;
  const topLevelIndices = Array.from(snapshot.topLevelIndices);

  const directiveContexts: DirectiveContext[] = [];
  collectDirectiveContexts(
    allItems,
    topLevelIndices,
    inclCtx,
    inclCtx.length,
    directiveContexts,
  );

  const topLevelItems: ConfigItem[] = topLevelIndices.map((i) =>
    resolveConfigItem(allItems, i),
  );

  return {
    allDirectivesWithContext() { return directiveContexts; },
    allDirectives() { return directiveContexts.map((c) => c.directive); },
    items() { return topLevelItems; },
    ...makeIncludeContextMethods(inclCtx),
  } as ReconstructedConfig;
}
