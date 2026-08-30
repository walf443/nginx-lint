/**
 * Direct SDK-level tests for the snapshot-filtering helpers in
 * config-builder.ts. Previously these were only exercised indirectly
 * through server-tokens-enabled-ts's test suite; a future TS plugin
 * relying on buildConfigFromSnapshot()/the mock's snapshotFiltered()
 * shouldn't have to be the one that first notices a regression here.
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { parseConfig, applyFixes } from "./plugin-test-runner.js";
import { buildConfigFromSnapshot } from "./config-builder.js";
import type { ConfigSnapshot } from "./generated/interfaces/nginx-lint-plugin-config-api.js";

function directiveNames(items: ConfigSnapshot["allItems"]): string[] {
  return items
    .filter((item) => item.value.tag === "directive-item")
    .map((item) => (item.value.tag === "directive-item" ? item.value.val.name : ""));
}

describe("Config.snapshotFiltered (parser-output-backed mock)", () => {
  it("keeps only matching directives plus their ancestors", () => {
    const cfg = parseConfig(`\
http {
  gzip on;
  server {
    listen 80;
    server_tokens off;
    location / {
      proxy_pass http://backend;
    }
  }
}`);

    const snapshot = cfg.snapshotFiltered(["server_tokens"]);
    // "server" and "http" survive as ancestors of the "server_tokens" match;
    // "gzip", "listen", "location", "proxy_pass" have no matching name and
    // no matching descendant, so they're pruned.
    assert.deepEqual(
      new Set(directiveNames(snapshot.allItems)),
      new Set(["http", "server", "server_tokens"]),
    );
  });

  it("drops comments and blank lines even when they'd otherwise survive", () => {
    const cfg = parseConfig(`\
# a comment
http {
  # another comment

  server_tokens off;
}`);

    const snapshot = cfg.snapshotFiltered(["server_tokens", "http"]);
    for (const item of snapshot.allItems) {
      assert.equal(item.value.tag, "directive-item");
    }
  });

  it("returns an empty snapshot when nothing matches", () => {
    const cfg = parseConfig("http {\n  gzip on;\n}");
    const snapshot = cfg.snapshotFiltered(["server_tokens"]);
    assert.equal(snapshot.allItems.length, 0);
    assert.equal(snapshot.topLevelIndices.length, 0);
  });

  it("preserves includeContext regardless of the names filter", () => {
    const cfg = parseConfig("server_tokens on;", { includeContext: ["http", "server"] });
    const snapshot = cfg.snapshotFiltered(["server_tokens"]);
    assert.deepEqual(snapshot.includeContext, ["http", "server"]);
  });
});

describe("buildConfigFromSnapshot", () => {
  it("reconstructs allDirectivesWithContext with correct parentStack", () => {
    const cfg = parseConfig(`\
http {
  server {
    server_tokens off;
  }
}`);

    const rebuilt = buildConfigFromSnapshot(cfg.snapshotFiltered(["server_tokens"]));
    const contexts = rebuilt.allDirectivesWithContext();
    const serverTokensCtx = contexts.find((c) => c.directive.is("server_tokens"));

    assert.ok(serverTokensCtx, "server_tokens directive should be present");
    assert.deepEqual(serverTokensCtx.parentStack, ["http", "server"]);
    assert.equal(serverTokensCtx.depth, 2);
  });

  it("seeds parentStack from includeContext (matches host is_inside semantics)", () => {
    const cfg = parseConfig("server_tokens on;", { includeContext: ["http"] });
    const rebuilt = buildConfigFromSnapshot(cfg.snapshotFiltered(["server_tokens", "http"]));
    const contexts = rebuilt.allDirectivesWithContext();

    assert.equal(contexts.length, 1);
    assert.deepEqual(contexts[0].parentStack, ["http"]);
    assert.equal(contexts[0].depth, 1);
  });

  it("round-trips a full (unfiltered) snapshot losslessly for directive names", () => {
    const cfg = parseConfig(`\
http {
  gzip on;
  server {
    listen 80;
  }
}`);

    const rebuilt = buildConfigFromSnapshot(cfg.snapshot());
    const names = rebuilt.allDirectivesWithContext().map((c) => c.directive.name());
    assert.deepEqual(new Set(names), new Set(["http", "gzip", "server", "listen"]));
  });
});

// ── Fix constructors ────────────────────────────────────────────────
//
// These pin the exact Fix records the SDK emits against the host's
// implementations in src/plugin/component_rule.rs (replace_with /
// delete_line_fix / insert_* on DirectiveResource). They used to differ:
// insertBefore emitted a line-based shape that the applier normalizes into
// a whole-line replacement, so it deleted the directive it meant to insert
// before. Comparing the records catches that; counting findings does not.

const NESTED = `http {
    server_tokens on;
}
`;

function serverTokens() {
  const cfg = parseConfig(NESTED);
  const ctx = cfg
    .allDirectivesWithContext()
    .find((c) => c.directive.is("server_tokens"));
  assert.ok(ctx, "expected a server_tokens directive");
  return ctx.directive;
}

describe("Directive fix constructors", () => {
  it("replaceWith spans the leading whitespace and re-adds it", () => {
    const d = serverTokens();
    const fix = d.replaceWith("server_tokens off;");
    const leading = d.leadingWhitespace();

    assert.equal(fix.line, 0);
    assert.equal(fix.deleteLine, false);
    assert.equal(fix.insertAfter, false);
    assert.equal(fix.startOffset, d.startOffset() - leading.length);
    assert.equal(fix.endOffset, d.endOffset());
    assert.equal(fix.newText, leading + "server_tokens off;");
  });

  it("deleteLineFix stays line-based", () => {
    const d = serverTokens();
    const fix = d.deleteLineFix();

    assert.equal(fix.line, d.line());
    assert.equal(fix.deleteLine, true);
    assert.equal(fix.newText, "");
    assert.equal(fix.startOffset, undefined);
    assert.equal(fix.endOffset, undefined);
  });

  it("insertAfter is a zero-width insert indented to the directive", () => {
    const d = serverTokens();
    const fix = d.insertAfter("add_header X-Test 1;");

    assert.equal(fix.startOffset, d.endOffset());
    assert.equal(fix.endOffset, d.endOffset());
    assert.equal(fix.newText, "\n    add_header X-Test 1;");
    assert.equal(fix.line, 0);
    assert.equal(fix.insertAfter, false);
  });

  it("insertBefore is a zero-width insert at the line start, not a replacement", () => {
    const d = serverTokens();
    const fix = d.insertBefore("add_header X-Test 1;");
    const lineStart = d.startOffset() - (d.column() - 1);

    // The offsets are what make this an insert: without them the applier
    // normalizes the record into a whole-line replacement and the directive
    // is lost.
    assert.equal(fix.startOffset, lineStart);
    assert.equal(fix.endOffset, lineStart);
    assert.equal(fix.newText, "    add_header X-Test 1;\n");
    assert.equal(fix.line, 0);
  });

  it("the *Many variants indent every line", () => {
    const d = serverTokens();
    const lineStart = d.startOffset() - (d.column() - 1);

    const after = d.insertAfterMany(["a 1;", "b 2;"]);
    assert.equal(after.startOffset, d.endOffset());
    assert.equal(after.newText, "\n    a 1;\n    b 2;");

    const before = d.insertBeforeMany(["a 1;", "b 2;"]);
    assert.equal(before.startOffset, lineStart);
    assert.equal(before.newText, "    a 1;\n    b 2;\n");
  });

  it("a top-level directive gets no indent", () => {
    const cfg = parseConfig("server_tokens on;\n");
    const ctx = cfg
      .allDirectivesWithContext()
      .find((c) => c.directive.is("server_tokens"));
    assert.ok(ctx);

    assert.equal(ctx.directive.insertBefore("a 1;").newText, "a 1;\n");
    assert.equal(ctx.directive.insertAfter("a 1;").newText, "\na 1;");
  });
});

// ── Applying fixes ──────────────────────────────────────────────────
//
// The block above pins the Fix records; these check what they do. That is
// the distinction that mattered: insertBefore's old record looked
// plausible and only revealed itself once applied.

describe("applyFixes", () => {
  it("insertBefore keeps the directive it inserts before", () => {
    const d = serverTokens();
    const result = applyFixes(NESTED, [d.insertBefore("add_header X-Test 1;")]);

    assert.equal(result.skippedInvalid, 0);
    assert.equal(
      result.content,
      "http {\n    add_header X-Test 1;\n    server_tokens on;\n}\n",
    );
  });

  it("the old line-based shape would have replaced the line", () => {
    // Why the test above is worth having: this is what insertBefore used to
    // emit. The applier normalizes it into a whole-line replacement, so the
    // directive disappears.
    const result = applyFixes(NESTED, [
      {
        line: 2, oldText: undefined, newText: "add_header X-Test 1;",
        deleteLine: false, insertAfter: false,
        startOffset: undefined, endOffset: undefined,
      },
    ]);

    assert.ok(!result.content.includes("server_tokens on;"));
    assert.equal(result.content, "http {\nadd_header X-Test 1;\n}\n");
  });

  it("replaceWith preserves indentation", () => {
    const d = serverTokens();
    const result = applyFixes(NESTED, [d.replaceWith("server_tokens off;")]);

    assert.equal(result.applied, 1);
    assert.equal(result.content, "http {\n    server_tokens off;\n}\n");
  });

  it("insertAfter indents to the directive", () => {
    const d = serverTokens();
    const result = applyFixes(NESTED, [d.insertAfter("add_header X-Test 1;")]);

    assert.equal(
      result.content,
      "http {\n    server_tokens on;\n    add_header X-Test 1;\n}\n",
    );
  });

  it("applies two fixes at different positions", () => {
    const source = `http {
    server_tokens on;
    autoindex on;
}
`;
    const cfg = parseConfig(source);
    const byName = new Map(
      cfg.allDirectivesWithContext().map((c) => [c.directive.name(), c.directive]),
    );
    const serverTokens = byName.get("server_tokens");
    const autoindex = byName.get("autoindex");
    assert.ok(serverTokens && autoindex);

    const result = applyFixes(source, [
      serverTokens.replaceWith("server_tokens off;"),
      autoindex.replaceWith("autoindex off;"),
    ]);

    assert.equal(result.applied, 2);
    assert.equal(result.skippedInvalid, 0);
    assert.equal(
      result.content,
      "http {\n    server_tokens off;\n    autoindex off;\n}\n",
    );
  });

  it("skips an overlapping fix rather than splicing both", () => {
    // Two fixes over the same range: the applier keeps one and drops the
    // other. Dropping for overlap is not counted as invalid, matching the
    // CLI — so a rule emitting conflicting fixes gets one of them applied,
    // not a corrupted line.
    const d = serverTokens();
    const result = applyFixes(NESTED, [
      d.replaceWith("server_tokens off;"),
      d.replaceWith("server_tokens build;"),
    ]);

    assert.equal(result.applied, 1);
    // The assertion that makes this test about *overlap*: a fix dropped for
    // overlapping is not an invalid one. Without this, the test passes
    // equally if the second fix were rejected as unapplicable, and would
    // not notice assertFixed starting to throw on conflicting fixes.
    assert.equal(result.skippedInvalid, 0);
    assert.ok(
      result.content === "http {\n    server_tokens off;\n}\n" ||
        result.content === "http {\n    server_tokens build;\n}\n",
      `unexpected content: ${JSON.stringify(result.content)}`,
    );
  });

  it("reports fixes it could not apply instead of returning the input", () => {
    const result = applyFixes(NESTED, [
      {
        line: 1, oldText: undefined, newText: "x",
        deleteLine: false, insertAfter: false,
        startOffset: 10000, endOffset: 10001,
      },
    ]);

    assert.equal(result.applied, 0);
    assert.equal(result.skippedInvalid, 1);
    assert.equal(result.content, NESTED);
  });
});
