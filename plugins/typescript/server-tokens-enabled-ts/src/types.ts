/**
 * TypeScript type definitions for the nginx-lint plugin WIT interface.
 *
 * These types correspond to the WIT definition at wit/nginx-lint-plugin.wit.
 * WIT kebab-case names are converted to camelCase by jco.
 */

// --- types interface ---

export type Severity = "error" | "warning";

export interface Fix {
  line: number;
  oldText: string | undefined;
  newText: string;
  deleteLine: boolean;
  insertAfter: boolean;
  startOffset: number | undefined;
  endOffset: number | undefined;
}

export interface LintError {
  rule: string;
  category: string;
  message: string;
  severity: Severity;
  line: number | undefined;
  column: number | undefined;
  fixes: Fix[];
}

export interface PluginSpec {
  name: string;
  category: string;
  description: string;
  apiVersion: string;
  severity: string | undefined;
  why: string | undefined;
  badExample: string | undefined;
  goodExample: string | undefined;
  references: string[] | undefined;
}

// --- config-api interface ---

export type ArgumentType = "literal" | "quoted-string" | "single-quoted-string" | "variable";

export interface ArgumentInfo {
  value: string;
  raw: string;
  argType: ArgumentType;
  line: number;
  column: number;
  startOffset: number;
  endOffset: number;
}

/** WIT `directive` resource — host-backed object with methods. */
export interface Directive {
  name(): string;
  is(name: string): boolean;

  firstArg(): string | undefined;
  firstArgIs(value: string): boolean;
  argAt(index: number): string | undefined;
  lastArg(): string | undefined;
  hasArg(value: string): boolean;
  argCount(): number;
  args(): ArgumentInfo[];

  line(): number;
  column(): number;
  startOffset(): number;
  endOffset(): number;
  leadingWhitespace(): string;
  trailingWhitespace(): string;
  spaceBeforeTerminator(): string;

  hasBlock(): boolean;
  blockIsRaw(): boolean;

  replaceWith(newText: string): Fix;
  deleteLineFix(): Fix;
  insertAfter(newText: string): Fix;
  insertBefore(newText: string): Fix;
  insertAfterMany(lines: string[]): Fix;
  insertBeforeMany(lines: string[]): Fix;
}

export interface DirectiveContext {
  directive: Directive;
  parentStack: string[];
  depth: number;
}

/** WIT `config` resource — host-backed object with methods. */
export interface Config {
  allDirectivesWithContext(): DirectiveContext[];
  allDirectives(): Directive[];

  includeContext(): string[];
  isIncludedFrom(context: string): boolean;
  isIncludedFromHttp(): boolean;
  isIncludedFromHttpServer(): boolean;
  isIncludedFromHttpLocation(): boolean;
  isIncludedFromStream(): boolean;
  immediateParentContext(): string | undefined;
}
