# nginx-lint-plugin-sdk

The plugin SDK for languages that have no library SDK: a single binary that
carries the runtimes and tooling a plugin needs, the way `wasi-sdk` bundles
a toolchain. Today it builds plugins from Lua scripts; a plugin author needs
this binary and a `.lua` file, nothing else:

```bash
nginx-lint-plugin-sdk build my_rule.lua     # writes my_rule.wasm
nginx-lint test-plugins --plugins .         # checks it the way CI does
nginx-lint --plugins . nginx.conf
```

The result is an ordinary plugin component, and it runs under the default
sandbox: the Lua runtime imports nothing from `wasi:*`, so no
`--allow-wasi-plugins` is needed.

`build` runs the script's top level first, on a native Lua with the same
libraries the runtime has, so a syntax error, a script that throws while
loading, a missing `check` or a malformed `spec` fails the build with Lua's
own message (`my_rule.lua:3: unexpected symbol near '='`). What `check`
does with a config is only exercised by `nginx-lint test-plugins`; a
finding or fix of the wrong shape is reported there as an error finding,
not as a crash.

That first run is native, not sandboxed: the script has no file system,
network or process access, and cannot load bytecode, but nothing bounds
its time or memory. Build your own scripts with it; treat a `.lua` from
someone you do not trust the way you would any other code you run.

## Writing a plugin

A script returns a rule: a table with a `spec` (the same fields the other
SDKs take, in snake_case) and a `check` function. A script with several
rules returns a list of such tables (see the next section):

```lua
local nginx_lint = require("nginx_lint")

return {
  spec = {
    name = "server-tokens-enabled-lua",
    category = "security",
    description = "Detects when server_tokens is enabled",
    severity = nginx_lint.SEVERITY_WARNING,
    bad_example = "http {\n    server_tokens on;\n}\n",
    good_example = "http {\n    server_tokens off;\n}\n",
  },

  check = function(config)
    local errors = {}
    config:named("server_tokens", function(directive)
      if directive:first_arg_is("on") then
        errors[#errors + 1] = nginx_lint
          .warning(directive, "server_tokens is on; the nginx version is exposed")
          :with_fix(directive:replace_with("server_tokens off;"))
      end
    end)
    return errors
  end,
}
```

`config:all(visit)` walks every directive, `config:named(name, visit)` the
ones with that name. A directive has `name`, `args` (each with `value`,
`raw`, `type`), `line`, `column`, `parents` and `block`, plus the helpers
`is`, `first_arg`, `first_arg_is`, `arg_at`, `last_arg`, `has_arg`,
`arg_count`, `arg_values`, `is_inside`, `parent`, and the fix builders
`replace_with`, `delete_line`, `insert_after`, `insert_before`. Findings
come from `nginx_lint.warning(directive, message)` and
`nginx_lint.error(...)`, with `:with_fix(fix)` to attach a fix. The two
severities are `nginx_lint.SEVERITY_ERROR` and `nginx_lint.SEVERITY_WARNING`,
for `spec.severity` and for a finding built without those constructors;
the runtime rejects any other value.

The sandbox has no file system, clock or environment: `io`, `os`,
`package` and `debug` are absent, `require` knows only `nginx_lint`, and
`print` goes nowhere. Errors thrown by the script are reported as findings
against the file being linted, with the script's file name and line.

### Several rules in one script

Return a list of rule tables instead of one:

```lua
local server_tokens = { spec = { ... }, check = function(config) ... end }
local autoindex = { spec = { ... }, check = function(config) ... end }
return { server_tokens, autoindex }
```

The host loads the component as one rule per entry, each with its own name,
documentation and configuration, and names the rules it wants run when it
checks a file; the runtime builds the config table once and calls each
asked rule's `check` with it, in the script's order. The rules share that
table, so treat it as read-only. A rule without a name, or two rules with
one name, fail the build (and would fail to load). `tests/two-rules` is a
two-rule script.

The component targets the `plugin-rules` world, which the host loads from
the same release of nginx-lint as this builder onwards (the two share a
version number). A component built with this builder does not load on an
older nginx-lint.

Strings cross to the host as UTF-8: a message holding bytes that are not
(a multibyte argument cut with `string.sub`, say) has each such byte
replaced with U+FFFD rather than failing the check.

## How it works

`runtimes/lua/` holds a C shim and PUC Lua 5.4, built once with wasi-sdk into
`runtime.core.wasm`, which is committed and embedded into this binary.
The runtime reserves a 1 MiB buffer for the script and exports its
address; `build` appends a data segment that fills the buffer, then wraps
the module as a component.

Rebuild the runtime with `make build-lua-runtime` at the repository root
after changing the shim, the Lua-side library or the WIT, and commit the
result. The module records a hash of those inputs and the plugin API
version in its producers section (`wasm-tools metadata show` prints them),
and this crate's tests check them against the tree, so a forgotten rebuild
fails `cargo test -p nginx-lint-plugin-sdk` — with no C toolchain needed.

## Licenses

`nginx-lint-plugin-sdk license` prints the notices of the third-party code
this tool distributes: the Lua runtime's (Lua, wasi-libc, LLVM compiler-rt),
which every plugin it builds carries too, and those of the Rust crates in the
`nginx-lint-plugin-sdk` binary itself.
