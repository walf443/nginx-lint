/**
 * The `plugin-rules` exports built by defineRules, against the parser-backed
 * Config the test runner uses: which rules run for a given `rules` list, how
 * the config is fetched for them, and what specs() stamps in.
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { parseConfig, PluginTestRunner } from "./plugin-test-runner.js";
import { API_VERSION } from "./api-version.js";
import { defineRules, type Rule } from "./rules.js";
import type { LintError } from "./generated/interfaces/nginx-lint-plugin-types.js";

function finding(rule: string, line: number): LintError {
  return {
    rule,
    category: "test",
    message: `${rule} fired`,
    severity: "warning",
    line,
    column: 1,
    fixes: [],
  };
}

/** Reports every `server_tokens on` it sees. */
const serverTokens: Rule = {
  spec: { name: "server-tokens", category: "test", description: "server_tokens on" },
  relevantDirectives: ["server_tokens"],
  check(cfg) {
    return cfg
      .allDirectives()
      .filter((d) => d.is("server_tokens") && d.firstArgIs("on"))
      .map((d) => finding("server-tokens", d.line()));
  },
};

/** Reports every `autoindex on`, and records what config it was given. */
let autoindexSaw: string[] = [];
const autoindex: Rule = {
  spec: { name: "autoindex", category: "test", description: "autoindex on" },
  relevantDirectives: ["autoindex"],
  check(cfg) {
    autoindexSaw = cfg.allDirectives().map((d) => d.name());
    return cfg
      .allDirectives()
      .filter((d) => d.is("autoindex") && d.firstArgIs("on"))
      .map((d) => finding("autoindex", d.line()));
  },
};

/** Declares no relevant directives: reads comments, so needs the whole file. */
const commentCounter: Rule = {
  spec: { name: "comments", category: "test", description: "counts comments" },
  check(cfg) {
    const comments = cfg.items().filter((item) => item.tag === "comment-item");
    return comments.map((_, i) => finding("comments", i + 1));
  },
};

const source = `\
# a comment
http {
    server_tokens on;
    server {
        location / {
            autoindex on;
        }
    }
}
`;

describe("defineRules", () => {
  it("lists every rule's spec in order, with apiVersion filled in", () => {
    const { specs } = defineRules(serverTokens, autoindex);
    const names = specs().map((s) => s.name);
    assert.deepEqual(names, ["server-tokens", "autoindex"]);
    for (const spec of specs()) {
      assert.equal(spec.apiVersion, API_VERSION);
    }
  });

  it("keeps an apiVersion a rule sets itself, and fills in an explicit undefined", () => {
    const set: Rule = { ...serverTokens, spec: { ...serverTokens.spec, apiVersion: "0.1" } };
    assert.equal(defineRules(set).specs()[0].apiVersion, "0.1");
    // `apiVersion: undefined` is a present key; the host needs a string
    const unset: Rule = {
      ...serverTokens,
      spec: { ...serverTokens.spec, apiVersion: undefined },
    };
    assert.equal(defineRules(unset).specs()[0].apiVersion, API_VERSION);
  });

  it("runs only the rules asked for, in definition order", () => {
    const { check } = defineRules(serverTokens, autoindex);
    const cfg = parseConfig(source);

    const both = check(cfg, "t.conf", ["autoindex", "server-tokens"]);
    assert.deepEqual(
      both.map((e) => e.rule),
      ["server-tokens", "autoindex"],
    );

    const one = check(cfg, "t.conf", ["autoindex"]);
    assert.deepEqual(one.map((e) => e.rule), ["autoindex"]);

    assert.deepEqual(check(cfg, "t.conf", []), []);
    assert.deepEqual(check(cfg, "t.conf", ["no-such-rule"]), []);
  });

  it("prunes the config to the union of the asked rules' directives", () => {
    const { check } = defineRules(serverTokens, autoindex);
    const cfg = parseConfig(source);

    check(cfg, "t.conf", ["autoindex"]);
    // Pruned to autoindex and its ancestors: no server_tokens
    assert.deepEqual(autoindexSaw, ["http", "server", "location", "autoindex"]);

    check(cfg, "t.conf", ["autoindex", "server-tokens"]);
    // The union: server_tokens is kept for the other rule
    assert.deepEqual(autoindexSaw, ["http", "server_tokens", "server", "location", "autoindex"]);
  });

  it("fetches the whole config when an asked rule declares no directives", () => {
    const { check } = defineRules(autoindex, commentCounter);
    const cfg = parseConfig(source);

    const errors = check(cfg, "t.conf", ["autoindex", "comments"]);
    assert.deepEqual(errors.map((e) => e.rule), ["autoindex", "comments"]);
    // Whole file, so autoindex saw everything too
    assert.ok(autoindexSaw.includes("server_tokens"));
  });

  it("refuses an empty, unnamed or duplicated rule list", () => {
    assert.throws(() => defineRules(), /at least one rule/);
    const unnamed: Rule = { ...serverTokens, spec: { ...serverTokens.spec, name: "" } };
    assert.throws(() => defineRules(unnamed), /non-empty spec.name/);
    assert.throws(() => defineRules(serverTokens, serverTokens), /two rules are named/);
  });
});

describe("PluginTestRunner", () => {
  it("runs one rule over the config as it would see it in production", () => {
    const runner = new PluginTestRunner(autoindex);
    runner.assertErrors(source, 1);
    runner.assertErrorOnLine(source, 6);
    assert.deepEqual(autoindexSaw, ["http", "server", "location", "autoindex"]);
  });
});
