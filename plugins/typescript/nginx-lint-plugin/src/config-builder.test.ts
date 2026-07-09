/**
 * Direct SDK-level tests for the snapshot-filtering helpers in
 * config-builder.ts. Previously these were only exercised indirectly
 * through server-tokens-enabled-ts's test suite; a future TS plugin
 * relying on buildConfigFromSnapshot()/the mock's snapshotFiltered()
 * shouldn't have to be the one that first notices a regression here.
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { parseConfig } from "./plugin-test-runner.js";
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
