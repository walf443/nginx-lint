/**
 * Builds WIT-compatible Config/Directive objects from parsed JSON AST.
 *
 * The nginx-lint-parser WASM module returns the AST as JSON. This module
 * converts that JSON into objects that implement the same interface as
 * the host-backed WIT resources, so plugins can be tested without the
 * full WASM runtime.
 */

import type {
  Config,
  ConfigItem,
  Directive,
  DirectiveContext,
  DirectiveData,
  ArgumentInfo,
  ArgumentType,
  CommentInfo,
  BlankLineInfo,
} from "./generated/interfaces/nginx-lint-plugin-config-api.js";
import type { Fix } from "./generated/interfaces/nginx-lint-plugin-types.js";

// ── JSON AST types (matches nginx-lint-parser serde output) ──────────

interface ParsedPosition {
  line: number;
  column: number;
  offset: number;
}

interface ParsedSpan {
  start: ParsedPosition;
  end: ParsedPosition;
}

interface ParsedArgumentValue {
  Literal?: string;
  QuotedString?: string;
  SingleQuotedString?: string;
  Variable?: string;
}

interface ParsedArgument {
  value: ParsedArgumentValue;
  span: ParsedSpan;
  raw: string;
}

interface ParsedComment {
  text: string;
  span: ParsedSpan;
  leading_whitespace: string;
  trailing_whitespace: string;
}

interface ParsedBlankLine {
  span: ParsedSpan;
  content: string;
}

interface ParsedBlock {
  items: ParsedConfigItem[];
  span: ParsedSpan;
  raw_content: string | null;
  closing_brace_leading_whitespace: string;
  trailing_whitespace: string;
}

interface ParsedDirective {
  name: string;
  name_span: ParsedSpan;
  args: ParsedArgument[];
  block: ParsedBlock | null;
  span: ParsedSpan;
  trailing_comment: ParsedComment | null;
  leading_whitespace: string;
  space_before_terminator: string;
  trailing_whitespace: string;
}

type ParsedConfigItem =
  | { Directive: ParsedDirective }
  | { Comment: ParsedComment }
  | { BlankLine: ParsedBlankLine };

export interface ParsedAst {
  items: ParsedConfigItem[];
  include_context?: string[];
}

// ── Helpers ──────────────────────────────────────────────────────────

function getArgValue(arg: ParsedArgument): string {
  const v = arg.value;
  if (v.Literal !== undefined) return v.Literal;
  if (v.QuotedString !== undefined) return v.QuotedString;
  if (v.SingleQuotedString !== undefined) return v.SingleQuotedString;
  if (v.Variable !== undefined) return v.Variable;
  return "";
}

function getArgType(arg: ParsedArgument): ArgumentType {
  const v = arg.value;
  if (v.QuotedString !== undefined) return "quoted-string";
  if (v.SingleQuotedString !== undefined) return "single-quoted-string";
  if (v.Variable !== undefined) return "variable";
  return "literal";
}

function toArgumentInfo(arg: ParsedArgument): ArgumentInfo {
  return {
    value: getArgValue(arg),
    raw: arg.raw,
    argType: getArgType(arg),
    line: arg.span.start.line,
    column: arg.span.start.column,
    startOffset: arg.span.start.offset,
    endOffset: arg.span.end.offset,
  };
}

function toConfigItem(item: ParsedConfigItem): ConfigItem {
  if ("Directive" in item) {
    return { tag: "directive-item", val: buildDirective(item.Directive) };
  }
  if ("Comment" in item) {
    const c = item.Comment;
    const info: CommentInfo = {
      text: c.text,
      line: c.span.start.line,
      column: c.span.start.column,
      leadingWhitespace: c.leading_whitespace,
      trailingWhitespace: c.trailing_whitespace,
      startOffset: c.span.start.offset,
      endOffset: c.span.end.offset,
    };
    return { tag: "comment-item", val: info };
  }
  // BlankLine
  const b = (item as { BlankLine: ParsedBlankLine }).BlankLine;
  const info: BlankLineInfo = {
    line: b.span.start.line,
    content: b.content,
    startOffset: b.span.start.offset,
  };
  return { tag: "blank-line-item", val: info };
}

// ── buildDirective ───────────────────────────────────────────────────

function buildDirective(d: ParsedDirective): Directive {
  const argValues = d.args.map(getArgValue);
  const argInfos = d.args.map(toArgumentInfo);

  const directiveData: DirectiveData = {
    name: d.name,
    args: argInfos,
    line: d.span.start.line,
    column: d.span.start.column,
    startOffset: d.span.start.offset,
    endOffset: d.span.end.offset,
    endLine: d.span.end.line,
    endColumn: d.span.end.column,
    leadingWhitespace: d.leading_whitespace,
    trailingWhitespace: d.trailing_whitespace,
    spaceBeforeTerminator: d.space_before_terminator,
    hasBlock: d.block !== null,
    blockIsRaw: d.block?.raw_content !== null && d.block?.raw_content !== undefined,
    blockRawContent: d.block?.raw_content ?? undefined,
    closingBraceLeadingWhitespace: d.block?.closing_brace_leading_whitespace ?? undefined,
    blockTrailingWhitespace: d.block?.trailing_whitespace ?? undefined,
    trailingCommentText: d.trailing_comment?.text ?? undefined,
    nameEndColumn: d.name_span.end.column,
    nameEndOffset: d.name_span.end.offset,
    blockStartLine: d.block?.span.start.line ?? undefined,
    blockStartColumn: d.block?.span.start.column ?? undefined,
    blockStartOffset: d.block?.span.start.offset ?? undefined,
  };

  return {
    data() { return directiveData; },
    name() { return d.name; },
    is(name: string) { return d.name === name; },
    firstArg() { return argValues[0] ?? undefined; },
    firstArgIs(value: string) { return argValues[0] === value; },
    argAt(index: number) { return argValues[index] ?? undefined; },
    lastArg() { return argValues.length > 0 ? argValues[argValues.length - 1] : undefined; },
    hasArg(value: string) { return argValues.includes(value); },
    argCount() { return argValues.length; },
    args() { return argInfos; },
    line() { return d.span.start.line; },
    column() { return d.span.start.column; },
    startOffset() { return d.span.start.offset; },
    endOffset() { return d.span.end.offset; },
    leadingWhitespace() { return d.leading_whitespace; },
    trailingWhitespace() { return d.trailing_whitespace; },
    spaceBeforeTerminator() { return d.space_before_terminator; },
    hasBlock() { return d.block !== null; },
    blockItems(): ConfigItem[] {
      if (!d.block) return [];
      return d.block.items.map(toConfigItem);
    },
    blockIsRaw() { return d.block?.raw_content != null; },
    replaceWith(newText: string): Fix {
      return {
        line: d.span.start.line,
        oldText: undefined,
        newText,
        deleteLine: false,
        insertAfter: false,
        startOffset: d.span.start.offset,
        endOffset: d.span.end.offset,
      };
    },
    deleteLineFix(): Fix {
      return {
        line: d.span.start.line,
        oldText: undefined,
        newText: "",
        deleteLine: true,
        insertAfter: false,
        startOffset: undefined,
        endOffset: undefined,
      };
    },
    insertAfter(newText: string): Fix {
      return {
        line: d.span.start.line,
        oldText: undefined,
        newText,
        deleteLine: false,
        insertAfter: true,
        startOffset: undefined,
        endOffset: undefined,
      };
    },
    insertBefore(newText: string): Fix {
      return {
        line: d.span.start.line,
        oldText: undefined,
        newText,
        deleteLine: false,
        insertAfter: false,
        startOffset: undefined,
        endOffset: undefined,
      };
    },
    insertAfterMany(lines: string[]): Fix {
      return {
        line: d.span.start.line,
        oldText: undefined,
        newText: lines.join("\n"),
        deleteLine: false,
        insertAfter: true,
        startOffset: undefined,
        endOffset: undefined,
      };
    },
    insertBeforeMany(lines: string[]): Fix {
      return {
        line: d.span.start.line,
        oldText: undefined,
        newText: lines.join("\n"),
        deleteLine: false,
        insertAfter: false,
        startOffset: undefined,
        endOffset: undefined,
      };
    },
  } as Directive;
}

// ── allDirectivesWithContext (DFS) ───────────────────────────────────

function collectDirectivesWithContext(
  items: ParsedConfigItem[],
  initialParents: string[],
): DirectiveContext[] {
  const results: DirectiveContext[] = [];
  const stack: Array<{ iter: ParsedConfigItem[]; index: number; parentName: string | null }> = [
    { iter: items, index: 0, parentName: null },
  ];
  const currentParents = [...initialParents];

  while (stack.length > 0) {
    const top = stack[stack.length - 1];
    if (top.index >= top.iter.length) {
      stack.pop();
      if (top.parentName !== null) {
        currentParents.pop();
      }
      continue;
    }

    const item = top.iter[top.index];
    top.index++;

    if ("Directive" in item) {
      const d = item.Directive;
      results.push({
        directive: buildDirective(d),
        parentStack: [...currentParents],
        depth: currentParents.length,
      });

      if (d.block !== null) {
        currentParents.push(d.name);
        stack.push({ iter: d.block.items, index: 0, parentName: d.name });
      }
    }
  }

  return results;
}

// ── buildConfig ──────────────────────────────────────────────────────

export function buildConfig(
  ast: ParsedAst,
  opts?: { includeContext?: string[] },
): Config {
  const inclCtx = opts?.includeContext ?? ast.include_context ?? [];

  return {
    allDirectivesWithContext() {
      return collectDirectivesWithContext(ast.items, inclCtx);
    },
    allDirectives() {
      return collectDirectivesWithContext(ast.items, inclCtx).map((c) => c.directive);
    },
    items() {
      return ast.items.map(toConfigItem);
    },
    includeContext() { return inclCtx; },
    isIncludedFrom(context: string) { return inclCtx.includes(context); },
    isIncludedFromHttp() { return inclCtx.includes("http"); },
    isIncludedFromHttpServer() { return inclCtx.includes("http") && inclCtx.includes("server"); },
    isIncludedFromHttpLocation() { return inclCtx.includes("http") && inclCtx.includes("location"); },
    isIncludedFromStream() { return inclCtx.includes("stream"); },
    immediateParentContext() { return inclCtx.length > 0 ? inclCtx[inclCtx.length - 1] : undefined; },
  } as Config;
}
