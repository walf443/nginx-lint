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

## Writing a plugin

A script returns a table with a `spec` (the same fields the other SDKs
take, in snake_case) and a `check` function:

```lua
local nginx_lint = require("nginx_lint")

return {
  spec = {
    name = "server-tokens-enabled-lua",
    category = "security",
    description = "Detects when server_tokens is enabled",
    severity = "warning",
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
`nginx_lint.error(...)`, with `:with_fix(fix)` to attach a fix.

The sandbox has no file system, clock or environment: `io`, `os`,
`package` and `debug` are absent, `require` knows only `nginx_lint`, and
`print` goes nowhere. Errors thrown by the script are reported as findings
against the file being linted, with the script's file name and line.

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
