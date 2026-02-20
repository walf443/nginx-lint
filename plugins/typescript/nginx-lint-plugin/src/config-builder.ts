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

function wrapDirective(
  data: DirectiveData,
  resolveBlockItems: () => ConfigItem[],
): Directive {
  const argValues = data.args.map((a) => a.value);

  return {
    data() { return data; },
    name() { return data.name; },
    is(name: string) { return data.name === name; },
    firstArg() { return argValues[0] ?? undefined; },
    firstArgIs(value: string) { return argValues[0] === value; },
    argAt(index: number) { return argValues[index] ?? undefined; },
    lastArg() { return argValues.length > 0 ? argValues[argValues.length - 1] : undefined; },
    hasArg(value: string) { return argValues.includes(value); },
    argCount() { return argValues.length; },
    args(): ArgumentInfo[] { return data.args; },
    line() { return data.line; },
    column() { return data.column; },
    startOffset() { return data.startOffset; },
    endOffset() { return data.endOffset; },
    leadingWhitespace() { return data.leadingWhitespace; },
    trailingWhitespace() { return data.trailingWhitespace; },
    spaceBeforeTerminator() { return data.spaceBeforeTerminator; },
    hasBlock() { return data.hasBlock; },
    blockItems(): ConfigItem[] { return resolveBlockItems(); },
    blockIsRaw() { return data.blockIsRaw; },
    replaceWith(newText: string): Fix {
      return {
        line: data.line, oldText: undefined, newText,
        deleteLine: false, insertAfter: false,
        startOffset: data.startOffset, endOffset: data.endOffset,
      };
    },
    deleteLineFix(): Fix {
      return {
        line: data.line, oldText: undefined, newText: "",
        deleteLine: true, insertAfter: false,
        startOffset: undefined, endOffset: undefined,
      };
    },
    insertAfter(newText: string): Fix {
      return {
        line: data.line, oldText: undefined, newText,
        deleteLine: false, insertAfter: true,
        startOffset: undefined, endOffset: undefined,
      };
    },
    insertBefore(newText: string): Fix {
      return {
        line: data.line, oldText: undefined, newText,
        deleteLine: false, insertAfter: false,
        startOffset: undefined, endOffset: undefined,
      };
    },
    insertAfterMany(lines: string[]): Fix {
      return {
        line: data.line, oldText: undefined, newText: lines.join("\n"),
        deleteLine: false, insertAfter: true,
        startOffset: undefined, endOffset: undefined,
      };
    },
    insertBeforeMany(lines: string[]): Fix {
      return {
        line: data.line, oldText: undefined, newText: lines.join("\n"),
        deleteLine: false, insertAfter: false,
        startOffset: undefined, endOffset: undefined,
      };
    },
  } as Directive;
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
    ...makeIncludeContextMethods(inclCtx),
  } as Config;
}
