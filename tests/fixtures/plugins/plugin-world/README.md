# A component of the original `plugin` world

`server-tokens-enabled-lua.wasm` is `plugins/lua/server-tokens-enabled-lua`
built by the Lua runtime as it was before it moved to `plugin-rules`: a
component exporting `spec` and `check(cfg, path)`, one rule.

Every SDK now builds `plugin-rules`, so nothing in the tree produces this
world any more. The host keeps loading it, for plugins built before the
move, and this file is what keeps that path tested
(`plugin_world_component_still_loads` in `src/plugin/component_rule.rs`).
Rebuilding it is not possible with the current tree, which is the point;
if the world is ever dropped from the host, delete the file with the code.
