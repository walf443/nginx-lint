//! Build-time checks of a plugin script. A native Lua 5.4 runs the script's
//! top level with the same standard libraries the runtime opens and the same
//! `nginx_lint` module it embeds, then looks at what came back: this is
//! what the runtime would do on first use, so a syntax error, a script that
//! throws on load or a malformed `spec` fails `build` here — with Lua's own
//! message and the script's file name — rather than every later lint run.
//! `check` itself is not called: it needs a parsed config, which is what
//! `nginx-lint test-plugins` provides.

use anyhow::{Result, anyhow, bail, ensure};
use mlua::chunk::ChunkMode;
use mlua::{Lua, LuaOptions, StdLib, Table, Value};

/// The Lua half of the runtime, shared with it verbatim.
const NGINX_LINT_LUA: &str = include_str!("../runtimes/lua/nginx_lint.lua");

/// Brings the base library in line with the runtime's, which matters twice
/// over here: a script must not pass validation by doing something the
/// runtime refuses, and this Lua is native, so it must not be handed
/// anything the sandboxed one would not run either. The file functions
/// have no file system in the runtime, and print has nowhere to go there.
/// `load` is stricter than the runtime's stock one: Lua does not verify
/// bytecode, and what that can do is contained by the wasm sandbox there
/// and by nothing here, so it is held to text.
const PRELUDE: &str = r#"
local raw_load = load
-- Varargs, not a named env: load() tells an omitted env from an explicit
-- nil, and a nil env would leave the chunk with no _ENV at all.
load = function(chunk, name, _mode, ...) return raw_load(chunk, name, "t", ...) end
loadfile = function() return nil, "no file system in the plugin sandbox" end
dofile = function() error("no file system in the plugin sandbox") end
print = function() end
"#;

/// Loads `script` under `name` and checks that it returns a rule table —
/// `spec` a table (or a function returning one) with the required fields,
/// `check` a function — or a list of them with distinct names. The runtime
/// reads the script the same way (see `rules_from_script` in shim.c).
pub fn validate(name: &str, script: &[u8]) -> Result<()> {
    // The same libraries as the runtime's shim: no io, os, package or debug.
    let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8;
    let lua = Lua::new_with(libs, LuaOptions::default())?;
    lua.load(PRELUDE).set_name("=prelude").exec()?;

    let library: Table = lua.load(NGINX_LINT_LUA).set_name("=nginx_lint").call(())?;
    let require = lua.create_function(move |_, module: String| {
        if module == "nginx_lint" {
            Ok(library.clone())
        } else {
            Err(mlua::Error::runtime(format!(
                "module '{module}' not found: the plugin sandbox has no package path"
            )))
        }
    })?;
    lua.globals().set("require", require)?;

    // Text only, as the runtime loads it: bytecode would run here natively
    // and then be refused there. Loaded as a chunk and called, not
    // `eval`ed: eval first tries the script as an expression, so a bare
    // `{ spec = ..., check = ... }` missing its `return` would pass here and
    // then fail to parse in the runtime, which only knows chunks.
    let plugin: Value = lua
        .load(script)
        .set_name(format!("@{name}"))
        .set_mode(ChunkMode::Text)
        .call(())
        .map_err(lua_message)?;
    let Value::Table(plugin) = plugin else {
        bail!(
            "{name}: the script must return a rule table (`spec` and `check`) or a list of them, not a {}",
            plugin.type_name()
        );
    };

    // One rule, or a list of them: a rule table has `check`, a list has
    // an array part
    let rules: Vec<Table> = if plugin.contains_key("check")? || plugin.contains_key("spec")? {
        vec![plugin]
    } else {
        let rules: Vec<Value> = plugin.sequence_values().collect::<Result<_, _>>()?;
        if rules.is_empty() {
            bail!(
                "{name}: the script returned neither a rule table (`spec` and `check`) nor a list of them"
            );
        }
        rules
            .into_iter()
            .enumerate()
            .map(|(i, rule)| match rule {
                Value::Table(rule) => Ok(rule),
                other => bail!(
                    "{name}: rule {} is a{} {}, not a table",
                    i + 1,
                    if other.type_name().starts_with("i") {
                        "n"
                    } else {
                        ""
                    },
                    other.type_name()
                ),
            })
            .collect::<Result<_>>()?
    };

    let mut names: Vec<String> = Vec::new();
    for rule in &rules {
        let rule_name = validate_rule(name, rule)?;
        if names.contains(&rule_name) {
            bail!("{name}: two rules are named {rule_name:?}");
        }
        names.push(rule_name);
    }
    Ok(())
}

/// Checks one rule table, returning its name.
fn validate_rule(name: &str, plugin: &Table) -> Result<String> {
    let spec = match plugin.get::<Value>("spec")? {
        Value::Table(spec) => spec,
        Value::Function(spec) => match spec.call::<Value>(()).map_err(lua_message)? {
            Value::Table(spec) => spec,
            other => bail!(
                "{name}: `spec` returned a {}, not a table",
                other.type_name()
            ),
        },
        other => bail!(
            "{name}: `spec` must be a table or a function returning one, not a {}",
            other.type_name()
        ),
    };
    // WIT strings are UTF-8, and Lua strings are bytes: a script saved in
    // another encoding builds fine and then fails to load, so the bytes are
    // checked here while the file name is at hand.
    let utf8 = |field: &str, value: &mlua::LuaString| -> Result<()> {
        ensure!(
            std::str::from_utf8(&value.as_bytes()).is_ok(),
            "{name}: spec.{field} is not valid UTF-8"
        );
        Ok(())
    };
    for field in ["name", "category", "description"] {
        match spec.get::<Value>(field)? {
            Value::String(value) if !value.as_bytes().is_empty() => utf8(field, &value)?,
            Value::String(_) => bail!("{name}: spec.{field} is empty"),
            Value::Nil => bail!("{name}: spec.{field} is missing"),
            other => bail!(
                "{name}: spec.{field} must be a string, not a {}",
                other.type_name()
            ),
        }
    }
    for field in [
        "severity",
        "why",
        "bad_example",
        "good_example",
        "min_nginx_version",
        "max_nginx_version",
    ] {
        match spec.get::<Value>(field)? {
            Value::Nil => {}
            Value::String(value) => utf8(field, &value)?,
            other => bail!(
                "{name}: spec.{field} must be a string, not a {}",
                other.type_name()
            ),
        }
    }
    if let Value::String(severity) = spec.get::<Value>("severity")? {
        let severity = severity.to_str()?;
        ensure!(
            *severity == *"error" || *severity == *"warning",
            "{name}: spec.severity must be \"error\" or \"warning\", not \"{}\"",
            &*severity
        );
    }
    match spec.get::<Value>("references")? {
        Value::Nil => {}
        Value::Table(references) => {
            for entry in references.sequence_values::<Value>() {
                match entry? {
                    Value::String(value) => utf8("references", &value)?,
                    other => bail!(
                        "{name}: spec.references must hold strings, not a {}",
                        other.type_name()
                    ),
                }
            }
        }
        other => bail!(
            "{name}: spec.references must be a list of strings, not a {}",
            other.type_name()
        ),
    }

    match plugin.get::<Value>("check")? {
        Value::Function(_) => {}
        Value::Nil => bail!("{name}: `check` is missing"),
        other => bail!(
            "{name}: `check` must be a function, not a {}",
            other.type_name()
        ),
    }
    Ok(spec.get::<String>("name")?)
}

/// Lua's own message (with its traceback, for a runtime error), without
/// mlua's "runtime error: " / "syntax error: " prefix: it already starts
/// with the script name and line.
fn lua_message(error: mlua::Error) -> anyhow::Error {
    match error {
        mlua::Error::SyntaxError { message, .. } | mlua::Error::RuntimeError(message) => {
            anyhow!("{message}")
        }
        other => anyhow!("{other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &[u8] = include_bytes!("../../lua/server-tokens-enabled-lua/plugin.lua");

    #[test]
    fn accepts_the_two_rule_script() {
        let script = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/two-rules/plugin.lua"
        ))
        .unwrap();
        validate("plugin.lua", &script).unwrap();
    }

    #[test]
    fn accepts_the_example_plugin() {
        validate("plugin.lua", EXAMPLE).unwrap();
    }

    /// The example's Makefile diffs `--fix` output against examples/good.conf,
    /// while the plugin's spec carries the same text inline (a script cannot
    /// embed a file); this keeps the two from drifting apart.
    #[test]
    fn example_plugin_embeds_its_example_files() {
        let source = std::str::from_utf8(EXAMPLE).unwrap();
        for file in [
            include_str!("../../lua/server-tokens-enabled-lua/examples/bad.conf"),
            include_str!("../../lua/server-tokens-enabled-lua/examples/good.conf"),
        ] {
            assert!(source.contains(file), "plugin.lua does not embed:\n{file}");
        }
    }

    #[test]
    fn reports_syntax_errors_with_the_script_name_and_line() {
        let err = validate(
            "rule.lua",
            b"return {\n  check = function() local x = = 1 end,\n}\n",
        )
        .unwrap_err();
        assert!(err.to_string().starts_with("rule.lua:2:"), "{err}");
        assert!(err.to_string().contains("unexpected symbol"), "{err}");
    }

    /// A script that is a single expression, such as the plugin table with
    /// its `return` forgotten, is a syntax error to the runtime; it must be
    /// one here too, not silently evaluated as an expression.
    #[test]
    fn rejects_a_script_that_is_only_an_expression() {
        let err = validate(
            "rule.lua",
            b"{\n  spec = { name = 'x', category = 'c', description = 'd' },\n  check = function() end,\n}\n",
        )
        .unwrap_err();
        assert!(err.to_string().starts_with("rule.lua:1:"), "{err}");
        assert!(
            err.to_string().contains("unexpected symbol near '{'"),
            "{err}"
        );
        let err = validate(
            "rule.lua",
            b"(function() return { check = function() end } end)()",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("must return a rule table"),
            "{err}"
        );
    }

    #[test]
    fn reports_a_script_that_throws_on_load() {
        let err = validate("rule.lua", b"error('boom')").unwrap_err();
        assert!(err.to_string().starts_with("rule.lua:1: boom"), "{err}");
        let err = validate("rule.lua", b"error({ code = 1 })").unwrap_err();
        assert!(err.to_string().contains("table"), "{err}");
    }

    #[test]
    fn requires_a_plugin_table_with_spec_and_check() {
        for (script, expected) in [
            (&b"return 42"[..], "must return a rule table"),
            (b"return { check = function() end }", "spec` must be a table"),
            (b"return { spec = { name = 'x', category = 'c', description = 'd' } }", "`check` is missing"),
            (b"return { spec = { name = 'x', category = 'c', description = 'd' }, check = 1 }", "`check` must be a function"),
            (b"return { spec = { category = 'c', description = 'd' }, check = function() end }", "spec.name is missing"),
            (b"return { spec = { name = '', category = 'c', description = 'd' }, check = function() end }", "spec.name is empty"),
            (b"return { spec = { name = 'x', category = 'c', description = 'd', severity = 'info' }, check = function() end }", "spec.severity must be"),
            (b"return { spec = { name = 'x', category = 'c', description = 'd', references = 'x' }, check = function() end }", "spec.references must be a list"),
            (b"return { spec = function() return 1 end, check = function() end }", "`spec` returned a"),
        ] {
            let err = validate("rule.lua", script).unwrap_err();
            assert!(err.to_string().contains(expected), "{expected}: {err}");
        }
    }

    #[test]
    fn accepts_a_list_of_rules_with_distinct_names() {
        validate(
            "rules.lua",
            b"local function rule(name) return { spec = { name = name, category = 'c', description = 'd' }, check = function() end } end\nreturn { rule('a'), rule('b') }",
        )
        .unwrap();
        for (script, expected) in [
            (&b"return {}"[..], "neither a rule table"),
            (b"return { 1 }", "rule 1 is an integer, not a table"),
            (b"local r = { spec = { name = 'a', category = 'c', description = 'd' }, check = function() end }\nreturn { r, r }", "two rules are named \"a\""),
            (b"return { { spec = { name = 'a', category = 'c', description = 'd' } } }", "`check` is missing"),
        ] {
            let err = validate("rules.lua", script).unwrap_err();
            assert!(err.to_string().contains(expected), "{expected}: {err}");
        }
    }

    #[test]
    fn rejects_spec_strings_that_are_not_utf8() {
        let script = b"return { spec = { name = 'x', category = 'c', description = 'caf\xe9' }, check = function() end }";
        let err = validate("rule.lua", script).unwrap_err();
        assert!(
            err.to_string()
                .contains("spec.description is not valid UTF-8"),
            "{err}"
        );
        let script = b"return { spec = { name = 'x', category = 'c', description = 'd', references = { '\xff' } }, check = function() end }";
        let err = validate("rule.lua", script).unwrap_err();
        assert!(
            err.to_string()
                .contains("spec.references is not valid UTF-8"),
            "{err}"
        );
    }

    /// load() with an omitted env must behave as in the runtime: the chunk
    /// sees the globals.
    #[test]
    fn load_keeps_the_global_environment() {
        validate(
            "rule.lua",
            b"local f = assert(load('return math.pi')); assert(f() > 3)\nreturn { spec = { name = 'x', category = 'c', description = 'd' }, check = function() end }",
        )
        .unwrap();
    }

    #[test]
    fn spec_may_be_a_function() {
        validate(
            "rule.lua",
            b"return { spec = function() return { name = 'x', category = 'c', description = 'd' } end, check = function() end }",
        )
        .unwrap();
    }

    /// Bytecode never runs on this native Lua. The runtime refuses the
    /// script itself in bytecode form too; its own `load` is stock and would
    /// accept a bytecode string, so this side is deliberately the stricter
    /// one — the wasm sandbox contains what unverified bytecode can do, and
    /// nothing here does.
    #[test]
    fn never_runs_bytecode_natively() {
        let err = validate("rule.lua", b"\x1bLua\x54\x00").unwrap_err();
        assert!(err.to_string().contains("binary chunk"), "{err}");
        let err = validate("rule.lua", b"local _, err = load('\\27Lua'); error(err)").unwrap_err();
        assert!(err.to_string().contains("binary chunk"), "{err}");
    }

    #[test]
    fn has_no_file_system_like_the_runtime() {
        let err = validate("rule.lua", b"dofile('/etc/passwd')").unwrap_err();
        assert!(err.to_string().contains("no file system"), "{err}");
        let err = validate(
            "rule.lua",
            b"local f, err = loadfile('/etc/passwd'); error(err)",
        )
        .unwrap_err();
        assert!(err.to_string().contains("no file system"), "{err}");
    }

    #[test]
    fn require_knows_only_nginx_lint() {
        let err = validate("rule.lua", b"local socket = require('socket')").unwrap_err();
        assert!(
            err.to_string().contains("module 'socket' not found"),
            "{err}"
        );
    }
}
