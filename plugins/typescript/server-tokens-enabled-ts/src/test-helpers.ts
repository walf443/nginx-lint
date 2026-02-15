/**
 * Test helpers for mocking WIT resources (config, directive).
 *
 * In the real WASM runtime, `config` and `directive` are host-backed resources
 * with methods. For unit testing, we create plain objects that mimic the same
 * interface so we can test plugin logic without the WASM host.
 */

import type { Config, Directive, DirectiveContext } from "./types.js";

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
export function mockConfig(contexts: DirectiveContext[]): Config {
  return {
    allDirectivesWithContext() { return contexts; },
    allDirectives() { return contexts.map((c) => c.directive); },
    includeContext() { return []; },
    isIncludedFrom() { return false; },
    isIncludedFromHttp() { return false; },
    isIncludedFromHttpServer() { return false; },
    isIncludedFromHttpLocation() { return false; },
    isIncludedFromStream() { return false; },
    immediateParentContext() { return undefined; },
  };
}
