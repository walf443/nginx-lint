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
/// anything the sandboxed one would not run either. Lua does not verify
/// bytecode, so `load` is held to text, as the runtime holds the script
/// itself; the file functions have no file system in the runtime; print
/// has nowhere to go there.
const PRELUDE: &str = r#"
local raw_load = load
load = function(chunk, name, _mode, env) return raw_load(chunk, name, "t", env) end
loadfile = function() return nil, "no file system in the plugin sandbox" end
dofile = function() error("no file system in the plugin sandbox") end
print = function() end
"#;

/// Loads `script` under `name` and checks that it returns a plugin table:
/// `spec` a table (or a function returning one) with the required fields,
/// `check` a function.
pub fn validate(name: &str, script: &[u8]) -> Result<()> {
    // The same libraries as the runtime's shim: no io, os, package or debug.
    let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8;
    let lua = Lua::new_with(libs, LuaOptions::default())?;
    lua.load(PRELUDE).set_name("=prelude").exec()?;

    let library: Table = lua.load(NGINX_LINT_LUA).set_name("=nginx_lint").eval()?;
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
    // and then be refused there.
    let plugin: Value = lua
        .load(script)
        .set_name(format!("@{name}"))
        .set_mode(ChunkMode::Text)
        .eval()
        .map_err(lua_message)?;
    let Value::Table(plugin) = plugin else {
        bail!(
            "{name}: the script must return a table with `spec` and `check`, not a {}",
            plugin.type_name()
        );
    };

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
    for field in ["name", "category", "description"] {
        match spec.get::<Value>(field)? {
            Value::String(value) if !value.as_bytes().is_empty() => {}
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
            Value::Nil | Value::String(_) => {}
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
                let entry = entry?;
                ensure!(
                    matches!(entry, Value::String(_)),
                    "{name}: spec.references must hold strings, not a {}",
                    entry.type_name()
                );
            }
        }
        other => bail!(
            "{name}: spec.references must be a list of strings, not a {}",
            other.type_name()
        ),
    }

    match plugin.get::<Value>("check")? {
        Value::Function(_) => Ok(()),
        Value::Nil => bail!("{name}: `check` is missing"),
        other => bail!(
            "{name}: `check` must be a function, not a {}",
            other.type_name()
        ),
    }
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
    fn accepts_the_example_plugin() {
        validate("plugin.lua", EXAMPLE).unwrap();
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
            (&b"return 42"[..], "must return a table"),
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
    fn spec_may_be_a_function() {
        validate(
            "rule.lua",
            b"return { spec = function() return { name = 'x', category = 'c', description = 'd' } end, check = function() end }",
        )
        .unwrap();
    }

    /// What the runtime refuses, validation refuses too — and never runs.
    #[test]
    fn refuses_bytecode_as_the_runtime_does() {
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
