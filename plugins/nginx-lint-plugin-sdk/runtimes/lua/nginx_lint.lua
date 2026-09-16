-- The Lua side of the nginx-lint Lua plugin runtime. Loaded by the C shim
-- before the plugin script; reachable from the script as
-- `require("nginx_lint")`.
local M = {}

-- The two severities the host knows, for `spec.severity` and for a finding
-- built by hand rather than through M.warning / M.error. The runtime
-- rejects any other value for either.
M.SEVERITY_ERROR = "error"
M.SEVERITY_WARNING = "warning"

local Directive = {}
Directive.__index = Directive

function Directive:is(name) return self.name == name end

function Directive:arg_at(index)
  local arg = self.args[index]
  return arg and arg.value or nil
end

function Directive:first_arg() return self:arg_at(1) end
function Directive:last_arg() return self:arg_at(#self.args) end
function Directive:first_arg_is(value) return self:first_arg() == value end
function Directive:arg_count() return #self.args end

function Directive:has_arg(value)
  for _, arg in ipairs(self.args) do
    if arg.value == value then return true end
  end
  return false
end

function Directive:arg_values()
  local values = {}
  for i, arg in ipairs(self.args) do values[i] = arg.value end
  return values
end

function Directive:is_inside(block)
  for _, parent in ipairs(self.parents) do
    if parent == block then return true end
  end
  return false
end

function Directive:parent() return self.parents[#self.parents] end

-- Fixes mirror what the host's directive resource computes, so a rule's
-- fixes are the same whichever SDK produced them.
local function saturating_sub(value, amount)
  if amount > value then return 0 end
  return value - amount
end

local function replace_range(start_offset, end_offset, new_text)
  return { new_text = new_text, start_offset = start_offset, end_offset = end_offset }
end

local function line_start_offset(d)
  return saturating_sub(d.start_offset, saturating_sub(d.column, 1))
end

local function indent(d)
  if d.column <= 1 then return "" end
  return string.rep(" ", d.column - 1)
end

function Directive:replace_with(new_text)
  local start_offset = saturating_sub(self.start_offset, #self.leading_whitespace)
  return replace_range(start_offset, self.end_offset, self.leading_whitespace .. new_text)
end

function Directive:delete_line()
  return { line = self.line, delete_line = true, new_text = "" }
end

function Directive:insert_after(...)
  local pad = indent(self)
  local parts = {}
  for _, line in ipairs({ ... }) do
    parts[#parts + 1] = "\n" .. pad .. line
  end
  return replace_range(self.end_offset, self.end_offset, table.concat(parts))
end

function Directive:insert_before(...)
  local pad = indent(self)
  local parts = {}
  for _, line in ipairs({ ... }) do
    parts[#parts + 1] = pad .. line .. "\n"
  end
  local offset = line_start_offset(self)
  return replace_range(offset, offset, table.concat(parts))
end

local Config = {}
Config.__index = Config

local function walk(directives, visit)
  for _, directive in ipairs(directives) do
    visit(directive)
    walk(directive.block, visit)
  end
end

-- Calls visit for every directive, depth first.
function Config:all(visit) walk(self.directives, visit) end

-- Calls visit for every directive named `name`.
function Config:named(name, visit)
  walk(self.directives, function(directive)
    if directive.name == name then visit(directive) end
  end)
end

function Config:is_included_from(block)
  for _, context in ipairs(self.include_context) do
    if context == block then return true end
  end
  return false
end

local LintError = {}
LintError.__index = LintError

local function new_error(severity, directive, message)
  return setmetatable({
    message = message,
    severity = severity,
    line = directive.line,
    column = directive.column,
    fixes = {},
  }, LintError)
end

function M.warning(directive, message) return new_error(M.SEVERITY_WARNING, directive, message) end
function M.error(directive, message) return new_error(M.SEVERITY_ERROR, directive, message) end

function LintError:with_fix(fix)
  self.fixes[#self.fixes + 1] = fix
  return self
end

-- Called by the C shim with the host's flat snapshot: `items` in DFS order,
-- each directive item carrying the indices of its block children; `top` the
-- indices of the top-level items. Links the tree and attaches the methods.
function M._build_config(items, top, include_context, path)
  local config = setmetatable({
    path = path,
    directives = {},
    comments = {},
    blank_lines = {},
    include_context = include_context,
  }, Config)

  for _, item in ipairs(items) do
    if item.kind == "comment" then
      config.comments[#config.comments + 1] = item
    elseif item.kind == "blank_line" then
      config.blank_lines[#config.blank_lines + 1] = item
    end
  end

  local function link(indices, parents)
    local directives = {}
    for _, index in ipairs(indices) do
      local item = items[index]
      if item.kind == "directive" then
        -- Each directive gets its own copy: siblings sharing one table
        -- would see each other's edits to `parents`.
        item.parents = {}
        for i, name in ipairs(parents) do item.parents[i] = name end
        local child_parents = {}
        for i, name in ipairs(parents) do child_parents[i] = name end
        child_parents[#child_parents + 1] = item.name
        item.block = link(item.children, child_parents)
        item.children = nil
        directives[#directives + 1] = setmetatable(item, Directive)
      end
    end
    return directives
  end
  config.directives = link(top, {})
  return config
end

return M
