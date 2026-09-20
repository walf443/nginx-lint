-- Three rules, two of them broken: check() returning a finding that is
-- not a table, and check() throwing. Each failure is reported as that
-- rule's own finding, and the working rule's finding survives beside them.
local nginx_lint = require("nginx_lint")
local ok = { spec = { name = "ok-rule", category = "c", description = "d" },
  check = function(config)
    local errors = {}
    config:named("server_tokens", function(d) errors[#errors+1] = nginx_lint.warning(d, "found") end)
    return errors
  end }
local shape = { spec = { name = "shape-rule", category = "c", description = "d" },
  check = function(config) return { nginx_lint.warning({line=1,column=1}, "first is fine"), 42 } end }
local throws = { spec = { name = "throw-rule", category = "c", description = "d" },
  check = function(config) error("boom") end }
return { ok, shape, throws }
