import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { spec, check } from "./plugin.js";
import { parseConfig, PluginTestRunner } from "nginx-lint-plugin/testing";

describe("spec", () => {
  it("returns valid plugin metadata", () => {
    const s = spec();
    assert.equal(s.name, "server-tokens-enabled-ts");
    assert.equal(s.category, "security");
    assert.equal(s.severity, "warning");
    assert.ok(s.description.length > 0);
    assert.ok(s.badExample);
    assert.ok(s.goodExample);
  });
});

describe("check", () => {
  const runner = new PluginTestRunner(spec, check);

  it("detects server_tokens on", () => {
    const errors = runner.checkString("http {\n    server_tokens on;\n}");
    assert.equal(errors.length, 1);
    assert.equal(errors[0].rule, "server-tokens-enabled-ts");
    assert.ok(errors[0].message.includes("should be 'off'"));
    assert.equal(errors[0].line, 2);
    assert.equal(errors[0].fixes.length, 1);
  });

  it("no error when server_tokens off", () => {
    runner.assertErrors("http {\n    server_tokens off;\n}", 0);
  });

  it("no error when server_tokens build", () => {
    runner.assertErrors("http {\n    server_tokens build;\n}", 0);
  });

  it("warns when http block has no server_tokens directive", () => {
    const errors = runner.checkString("http {\n    server {\n        listen 80;\n    }\n}");
    assert.equal(errors.length, 1);
    assert.ok(errors[0].message.includes("defaults to 'on'"));
    assert.equal(errors[0].line, 1);
  });

  it("detects multiple server_tokens on", () => {
    runner.assertErrors(
      "http {\n    server_tokens on;\n    server {\n        server_tokens on;\n    }\n}",
      2,
    );
  });

  it("ignores server_tokens in stream context", () => {
    runner.assertErrors("stream {\n    server_tokens on;\n}", 0);
  });

  it("no warning for config without http block", () => {
    runner.assertErrors("events {\n    worker_connections 1024;\n}", 0);
  });

  it("no warning for file included from http context (parent should set server_tokens)", () => {
    const cfg = parseConfig("server {\n    listen 80;\n}", { includeContext: ["http"] });
    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 0);
  });

  it("detects server_tokens on in file included from http context", () => {
    const cfg = parseConfig("server {\n    server_tokens on;\n}", { includeContext: ["http"] });
    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 1);
    assert.ok(errors[0].message.includes("should be 'off'"));
    assert.equal(errors[0].line, 2);
  });

  it("no error for server_tokens off in file included from http context", () => {
    const cfg = parseConfig("server {\n    server_tokens off;\n}", { includeContext: ["http"] });
    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 0);
  });

  it("no warning for file included from http > server context", () => {
    const cfg = parseConfig("location / {\n    root /var/www;\n}", {
      includeContext: ["http", "server"],
    });
    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 0);
  });

  it("detects server_tokens on in file included from http > server context", () => {
    const cfg = parseConfig("server_tokens on;", { includeContext: ["http", "server"] });
    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 1);
    assert.ok(errors[0].message.includes("should be 'off'"));
  });

  it("ignores file included from stream context", () => {
    const cfg = parseConfig("server {\n    server_tokens on;\n}", { includeContext: ["stream"] });
    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 0);
  });

  it("testExamples with bad/good conf", () => {
    runner.testExamples(
      "http {\n    server_tokens on;\n    server {\n        listen 80;\n    }\n}",
      "http {\n    server_tokens off;\n    server {\n        listen 80;\n    }\n}",
    );
  });
});
