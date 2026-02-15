/**
 * Test helpers for mocking WIT resources (config, directive).
 *
 * In the real WASM runtime, `config` and `directive` are host-backed resources
 * with methods. For unit testing, we create plain objects that mimic the same
 * interface so we can test plugin logic without the WASM host.
 */

/** Minimal mock of a WIT `directive` resource. */
export function mockDirective(opts: {
  name: string;
  args?: string[];
  line?: number;
  column?: number;
}) {
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
    line() { return line; },
    column() { return column; },
    replaceWith(newText: string) {
      return { line: 0, oldText: undefined, newText, deleteLine: false, insertAfter: false };
    },
  };
}

/** Minimal mock of a WIT `config` resource. */
export function mockConfig(
  contexts: { directive: ReturnType<typeof mockDirective>; parentStack: string[]; depth: number }[]
) {
  return {
    allDirectivesWithContext() { return contexts; },
  };
}
