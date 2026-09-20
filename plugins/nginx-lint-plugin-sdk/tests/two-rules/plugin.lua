-- Two rules in one script: the plugin-rules world from Lua. The script
-- returns a list of rule tables; the host loads each as its own rule.
local nginx_lint = require("nginx_lint")

local BAD_TOKENS = [[
http {
    server_tokens on;
}
]]
local GOOD_TOKENS = [[
http {
    server_tokens off;
}
]]
local BAD_AUTOINDEX = [[
http {
    location /files/ {
        autoindex on;
    }
}
]]
local GOOD_AUTOINDEX = [[
http {
    location /files/ {
        autoindex off;
    }
}
]]

local server_tokens = {
  spec = {
    name = "server-tokens-enabled-two",
    category = "security",
    description = "server_tokens on, from a two-rule script",
    severity = nginx_lint.SEVERITY_WARNING,
    bad_example = BAD_TOKENS,
    good_example = GOOD_TOKENS,
  },
  check = function(config)
    local errors = {}
    config:named("server_tokens", function(directive)
      if directive:first_arg_is("on") then
        errors[#errors + 1] = nginx_lint
          .warning(directive, "server_tokens is on")
          :with_fix(directive:replace_with("server_tokens off;"))
      end
    end)
    return errors
  end,
}

local autoindex = {
  -- spec as a function, which the runtime accepts as well
  spec = function()
    return {
      name = "autoindex-enabled-two",
      category = "security",
      description = "autoindex on, from a two-rule script",
      severity = nginx_lint.SEVERITY_WARNING,
      bad_example = BAD_AUTOINDEX,
      good_example = GOOD_AUTOINDEX,
    }
  end,
  check = function(config)
    local errors = {}
    config:named("autoindex", function(directive)
      if directive:first_arg_is("on") then
        errors[#errors + 1] = nginx_lint
          .warning(directive, "autoindex is on")
          :with_fix(directive:replace_with("autoindex off;"))
      end
    end)
    return errors
  end,
}

return { server_tokens, autoindex }
