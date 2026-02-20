/**
 * Builds WIT-compatible Config/Directive objects from pre-computed WIT JSON.
 *
 * The nginx-lint-parser WASM module's `parse_to_wit_json` returns data
 * that closely matches the WIT interface types. This module wraps that
 * JSON data with the method-based Directive/Config interfaces.
 */

import type {
  Config,
  ConfigItem,
  Directive,
  DirectiveContext,
  DirectiveData,
  ArgumentInfo,
  CommentInfo,
  BlankLineInfo,
} from "./generated/interfaces/nginx-lint-plugin-config-api.js";
import type { Fix } from "./generated/interfaces/nginx-lint-plugin-types.js";

// ── JSON types from parse_to_wit_json output ────────────────────────

/** Top-level output from parse_to_wit_json */
export interface WitOutput {
  directivesWithContext: WitDirectiveContext[];
  includeContext: string[];
  items: WitConfigItem[];
  error?: string;
}

interface WitDirectiveContext {
  data: DirectiveData;
  blockItems: WitConfigItem[];
  parentStack: string[];
  depth: number;
}

type WitConfigItem =
  | { tag: "directive-item"; data: DirectiveData; blockItems: WitConfigItem[] }
  | { tag: "comment-item"; val: CommentInfo }
  | { tag: "blank-line-item"; val: BlankLineInfo };

// ── Wrap DirectiveData with Directive interface ─────────────────────

function wrapDirective(
  data: DirectiveData,
  blockItems: WitConfigItem[],
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
    blockItems(): ConfigItem[] { return blockItems.map(toConfigItem); },
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

// ── Convert WitConfigItem to ConfigItem ─────────────────────────────

function toConfigItem(item: WitConfigItem): ConfigItem {
  if (item.tag === "directive-item") {
    return { tag: "directive-item", val: wrapDirective(item.data, item.blockItems) };
  }
  if (item.tag === "comment-item") {
    return { tag: "comment-item", val: item.val };
  }
  return { tag: "blank-line-item", val: item.val };
}

// ── Build Config from WIT output ────────────────────────────────────

export function buildConfigFromWit(output: WitOutput): Config {
  const inclCtx = output.includeContext;

  const directiveContexts: DirectiveContext[] = output.directivesWithContext.map((ctx) => ({
    directive: wrapDirective(ctx.data, ctx.blockItems),
    parentStack: ctx.parentStack,
    depth: ctx.depth,
  }));

  return {
    allDirectivesWithContext() { return directiveContexts; },
    allDirectives() { return directiveContexts.map((c) => c.directive); },
    items() { return output.items.map(toConfigItem); },
    includeContext() { return inclCtx; },
    isIncludedFrom(context: string) { return inclCtx.includes(context); },
    isIncludedFromHttp() { return inclCtx.includes("http"); },
    isIncludedFromHttpServer() { return inclCtx.includes("http") && inclCtx.includes("server"); },
    isIncludedFromHttpLocation() { return inclCtx.includes("http") && inclCtx.includes("location"); },
    isIncludedFromStream() { return inclCtx.includes("stream"); },
    immediateParentContext() { return inclCtx.length > 0 ? inclCtx[inclCtx.length - 1] : undefined; },
  } as Config;
}
