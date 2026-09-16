-- Exercises every fix builder in nginx_lint.lua against the host's fix
-- applier: the SDK's Makefile applies this plugin's fixes to bad.conf and
-- requires the result to be good.conf, byte for byte. The fix arithmetic is
-- reimplemented in Lua rather than delegated to the host, so this is what
-- would catch it drifting from the host's DirectiveResource.
local nginx_lint = require("nginx_lint")

local BAD = [[
http {
    replace_me on;
    delete_me;
    insert_after_me;
    insert_before_me;
}
]]

local GOOD = [[
http {
    replaced;
    insert_after_me;
    inserted_after 1;
    inserted_after 2;
    inserted_before;
    insert_before_me;
}
]]

-- Each rule is keyed on a directive name; the two insertions also look at
-- the surrounding block so that, once the fix is applied, they no longer
-- report and good.conf is clean.
local rules = {
  replace_me = function(d) return d:replace_with("replaced;") end,
  delete_me = function(d) return d:delete_line() end,
  insert_after_me = function(d, siblings)
    if siblings.inserted_after then return nil end
    return d:insert_after("inserted_after 1;", "inserted_after 2;")
  end,
  insert_before_me = function(d, siblings)
    if siblings.inserted_before then return nil end
    return d:insert_before("inserted_before;")
  end,
}

return {
  spec = {
    name = "fix-kinds",
    category = "test",
    description = "Applies one fix of each kind, for pinning their bytes",
    severity = "warning",
    bad_example = BAD,
    good_example = GOOD,
  },

  check = function(config)
    local errors = {}
    config:all(function(block)
      local siblings = {}
      for _, d in ipairs(block.block) do siblings[d.name] = true end
      for _, d in ipairs(block.block) do
        local rule = rules[d.name]
        local fix = rule and rule(d, siblings)
        if fix then
          errors[#errors + 1] = nginx_lint.warning(d, d.name):with_fix(fix)
        end
      end
    end)
    -- Returned through table.pack so that on good.conf, where nothing is
    -- found, the list is `{ n = 0 }`: a bookkeeping field on an empty list,
    -- which the runtime must read as no findings rather than as one.
    return table.pack(table.unpack(errors))
  end,
}
