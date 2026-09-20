# A component of the original `plugin` world

`server-tokens-enabled-lua.wasm` is `plugins/lua/server-tokens-enabled-lua`
built by the Lua runtime as it was before it moved to `plugin-rules`
(`runtimes/lua/runtime.core.wasm` as of commit 11fd5fe, the last with the
`plugin` world): a component exporting `spec` and `check(cfg, path)`, one
rule, importing `nginx-lint:plugin/*@4.0.0`.

Every SDK builds `plugin-rules` from #452 on, so nothing in the tree
produces this world any more, and this file cannot be rebuilt from the
tree — which is the point. The host keeps loading the world, for plugins
built before the move, and this file is what keeps that path tested
(`plugin_world_component_still_loads` in `src/plugin/component_rule.rs`).

Two things end it, and the test with it:

- dropping the `plugin` world from the host — delete this directory with
  the code;
- bumping the WIT package version (`package nginx-lint:plugin@…`), which
  is baked into this component's import names, so it stops instantiating.
  A bump is a decision that no component built before it loads; that
  includes this one, so delete it then too, and the test it feeds.
