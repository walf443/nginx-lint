local nginx_lint = require("nginx_lint")

-- The same text as examples/bad.conf and examples/good.conf, which the
-- Makefile's --fix check reads; a Lua script has no way to embed a file.
local BAD = [[
http {
    server_tokens on;
}
]]

local GOOD = [[
http {
    server_tokens off;
}
]]

return {
  spec = {
    name = "server-tokens-enabled-lua",
    category = "security",
    description = "Detects when server_tokens is enabled (exposes nginx version)",
    severity = nginx_lint.SEVERITY_WARNING,
    why = "Server response headers reveal the exact nginx version, which tells an attacker which published vulnerabilities to try.",
    bad_example = BAD,
    good_example = GOOD,
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
