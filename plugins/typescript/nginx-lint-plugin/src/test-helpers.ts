/**
 * Test helpers for mocking WIT resources (config, directive).
 *
 * In the real WASM runtime, `config` and `directive` are host-backed resources
 * with methods. For unit testing, we create plain objects that mimic the same
 * interface so we can test plugin logic without the WASM host.
 *
 * Usage:
 *   import { mockConfig, mockDirective } from "nginx-lint-plugin/test-helpers";
 */

import type { Config, ConfigItem, Directive, DirectiveContext, DirectiveData } from "./generated/interfaces/nginx-lint-plugin-config-api.js";

/** Create a mock of a WIT `directive` resource. */
export function mockDirective(opts: {
  name: string;
  args?: string[];
  line?: number;
  column?: number;
}): Directive {
  const name = opts.name;
  const args = opts.args ?? [];
  const line = opts.line ?? 1;
  const column = opts.column ?? 1;

  return {
    data(): DirectiveData {
      return {
        name, args: [], line, column, startOffset: 0, endOffset: 0,
        endLine: line, endColumn: column, leadingWhitespace: "", trailingWhitespace: "",
        spaceBeforeTerminator: "", hasBlock: false, blockIsRaw: false,
        blockRawContent: undefined, closingBraceLeadingWhitespace: undefined,
        blockTrailingWhitespace: undefined, trailingCommentText: undefined,
        nameEndColumn: column, nameEndOffset: 0,
        blockStartLine: undefined, blockStartColumn: undefined, blockStartOffset: undefined,
      };
    },
    is(n: string) { return name === n; },
    name() { return name; },
    firstArg() { return args[0] ?? undefined; },
    firstArgIs(v: string) { return args[0] === v; },
    argAt(i: number) { return args[i] ?? undefined; },
    lastArg() { return args[args.length - 1] ?? undefined; },
    hasArg(v: string) { return args.includes(v); },
    argCount() { return args.length; },
    args() { return []; },
    line() { return line; },
    column() { return column; },
    startOffset() { return 0; },
    endOffset() { return 0; },
    leadingWhitespace() { return ""; },
    trailingWhitespace() { return ""; },
    spaceBeforeTerminator() { return ""; },
    hasBlock() { return false; },
    blockItems(): ConfigItem[] { return []; },
    blockIsRaw() { return false; },
    replaceWith(newText: string) {
      return { line: 0, oldText: undefined, newText, deleteLine: false, insertAfter: false, startOffset: undefined, endOffset: undefined };
    },
    deleteLineFix() {
      return { line: 0, oldText: undefined, newText: "", deleteLine: true, insertAfter: false, startOffset: undefined, endOffset: undefined };
    },
    insertAfter(newText: string) {
      return { line: 0, oldText: undefined, newText, deleteLine: false, insertAfter: true, startOffset: undefined, endOffset: undefined };
    },
    insertBefore(newText: string) {
      return { line: 0, oldText: undefined, newText, deleteLine: false, insertAfter: false, startOffset: undefined, endOffset: undefined };
    },
    insertAfterMany(lines: string[]) {
      return { line: 0, oldText: undefined, newText: lines.join("\n"), deleteLine: false, insertAfter: true, startOffset: undefined, endOffset: undefined };
    },
    insertBeforeMany(lines: string[]) {
      return { line: 0, oldText: undefined, newText: lines.join("\n"), deleteLine: false, insertAfter: false, startOffset: undefined, endOffset: undefined };
    },
  };
}

/** Create a mock of a WIT `config` resource. */
export function mockConfig(
  contexts: DirectiveContext[],
  opts?: { includeContext?: string[] },
): Config {
  const inclCtx = opts?.includeContext ?? [];
  return {
    allDirectivesWithContext() { return contexts; },
    allDirectives() { return contexts.map((c) => c.directive); },
    items(): ConfigItem[] { return []; },
    includeContext() { return inclCtx; },
    isIncludedFrom(context: string) { return inclCtx.includes(context); },
    isIncludedFromHttp() { return inclCtx.includes("http"); },
    isIncludedFromHttpServer() { return inclCtx.includes("http") && inclCtx.includes("server"); },
    isIncludedFromHttpLocation() { return inclCtx.includes("http") && inclCtx.includes("location"); },
    isIncludedFromStream() { return inclCtx.includes("stream"); },
    immediateParentContext() { return inclCtx.length > 0 ? inclCtx[inclCtx.length - 1] : undefined; },
  };
}
