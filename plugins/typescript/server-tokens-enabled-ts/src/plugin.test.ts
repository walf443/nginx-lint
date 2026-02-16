import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { spec, check } from "./plugin.js";
import { mockConfig, mockDirective } from "./test-helpers.js";

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
  it("detects server_tokens on", () => {
    const cfg = mockConfig([
      { directive: mockDirective({ name: "http", line: 1 }), parentStack: [], depth: 0 },
      {
        directive: mockDirective({ name: "server_tokens", args: ["on"], line: 2, column: 3 }),
        parentStack: ["http"],
        depth: 1,
      },
    ]);

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 1);
    assert.equal(errors[0].rule, "server-tokens-enabled-ts");
    assert.ok(errors[0].message.includes("should be 'off'"));
    assert.equal(errors[0].line, 2);
    assert.equal(errors[0].column, 3);
    assert.equal(errors[0].fixes.length, 1);
  });

  it("no error when server_tokens off", () => {
    const cfg = mockConfig([
      { directive: mockDirective({ name: "http", line: 1 }), parentStack: [], depth: 0 },
      {
        directive: mockDirective({ name: "server_tokens", args: ["off"], line: 2 }),
        parentStack: ["http"],
        depth: 1,
      },
    ]);

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 0);
  });

  it("no error when server_tokens build", () => {
    const cfg = mockConfig([
      { directive: mockDirective({ name: "http", line: 1 }), parentStack: [], depth: 0 },
      {
        directive: mockDirective({ name: "server_tokens", args: ["build"], line: 2 }),
        parentStack: ["http"],
        depth: 1,
      },
    ]);

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 0);
  });

  it("warns when http block has no server_tokens directive", () => {
    const cfg = mockConfig([
      { directive: mockDirective({ name: "http", line: 1 }), parentStack: [], depth: 0 },
      {
        directive: mockDirective({ name: "server", line: 2 }),
        parentStack: ["http"],
        depth: 1,
      },
      {
        directive: mockDirective({ name: "listen", args: ["80"], line: 3 }),
        parentStack: ["http", "server"],
        depth: 2,
      },
    ]);

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 1);
    assert.ok(errors[0].message.includes("defaults to 'on'"));
    assert.equal(errors[0].line, 1);
  });

  it("detects multiple server_tokens on", () => {
    const cfg = mockConfig([
      { directive: mockDirective({ name: "http", line: 1 }), parentStack: [], depth: 0 },
      {
        directive: mockDirective({ name: "server_tokens", args: ["on"], line: 2 }),
        parentStack: ["http"],
        depth: 1,
      },
      {
        directive: mockDirective({ name: "server_tokens", args: ["on"], line: 4 }),
        parentStack: ["http", "server"],
        depth: 2,
      },
    ]);

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 2);
  });

  it("ignores server_tokens in stream context", () => {
    const cfg = mockConfig([
      { directive: mockDirective({ name: "stream", line: 1 }), parentStack: [], depth: 0 },
      {
        directive: mockDirective({ name: "server_tokens", args: ["on"], line: 2 }),
        parentStack: ["stream"],
        depth: 1,
      },
    ]);

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 0);
  });

  it("no warning for config without http block", () => {
    const cfg = mockConfig([
      { directive: mockDirective({ name: "events", line: 1 }), parentStack: [], depth: 0 },
      {
        directive: mockDirective({ name: "worker_connections", args: ["1024"], line: 2 }),
        parentStack: ["events"],
        depth: 1,
      },
    ]);

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 0);
  });

  it("no warning for file included from http context (parent should set server_tokens)", () => {
    const cfg = mockConfig(
      [
        {
          directive: mockDirective({ name: "server", line: 1 }),
          parentStack: [],
          depth: 0,
        },
        {
          directive: mockDirective({ name: "listen", args: ["80"], line: 2 }),
          parentStack: ["server"],
          depth: 1,
        },
      ],
      { includeContext: ["http"] },
    );

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 0);
  });

  it("detects server_tokens on in file included from http context", () => {
    const cfg = mockConfig(
      [
        {
          directive: mockDirective({ name: "server", line: 1 }),
          parentStack: [],
          depth: 0,
        },
        {
          directive: mockDirective({ name: "server_tokens", args: ["on"], line: 2, column: 5 }),
          parentStack: ["server"],
          depth: 1,
        },
      ],
      { includeContext: ["http"] },
    );

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 1);
    assert.ok(errors[0].message.includes("should be 'off'"));
    assert.equal(errors[0].line, 2);
  });

  it("no error for server_tokens off in file included from http context", () => {
    const cfg = mockConfig(
      [
        {
          directive: mockDirective({ name: "server", line: 1 }),
          parentStack: [],
          depth: 0,
        },
        {
          directive: mockDirective({ name: "server_tokens", args: ["off"], line: 2 }),
          parentStack: ["server"],
          depth: 1,
        },
      ],
      { includeContext: ["http"] },
    );

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 0);
  });

  it("no warning for file included from http > server context", () => {
    const cfg = mockConfig(
      [
        {
          directive: mockDirective({ name: "location", line: 1 }),
          parentStack: [],
          depth: 0,
        },
        {
          directive: mockDirective({ name: "root", args: ["/var/www"], line: 2 }),
          parentStack: ["location"],
          depth: 1,
        },
      ],
      { includeContext: ["http", "server"] },
    );

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 0);
  });

  it("detects server_tokens on in file included from http > server context", () => {
    const cfg = mockConfig(
      [
        {
          directive: mockDirective({ name: "server_tokens", args: ["on"], line: 1 }),
          parentStack: [],
          depth: 0,
        },
      ],
      { includeContext: ["http", "server"] },
    );

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 1);
    assert.ok(errors[0].message.includes("should be 'off'"));
  });

  it("ignores file included from stream context", () => {
    const cfg = mockConfig(
      [
        {
          directive: mockDirective({ name: "server", line: 1 }),
          parentStack: [],
          depth: 0,
        },
        {
          directive: mockDirective({ name: "server_tokens", args: ["on"], line: 2 }),
          parentStack: ["server"],
          depth: 1,
        },
      ],
      { includeContext: ["stream"] },
    );

    const errors = check(cfg, "test.conf");
    assert.equal(errors.length, 0);
  });
});
