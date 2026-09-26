//! What the host provides to a component: the generated bindings of both
//! worlds, the store data every instance runs in — resource table, memory
//! limit, the WASI subset a plugin may be granted — and the `Host` impls of
//! the imported interfaces (`config-api` in [`config_api_host`], the
//! snapshots it hands out in [`snapshot`]; what a component returns is
//! converted in [`findings`]).

pub(crate) mod config_api_host;
pub(crate) mod findings;
pub(crate) mod snapshot;

use nginx_lint_common::parser::ast::{self, Config};
use std::sync::Arc;
use wasmtime::StoreLimits;
use wasmtime::component::{Resource, ResourceTable};

/// Host-side config resource, holding the parsed Config.
pub struct ConfigResource {
    pub(crate) config: Arc<Config>,
}

/// Host-side directive resource, referencing a directive inside the shared
/// Config AST by its path.
///
/// Holding `(Arc<Config>, path)` instead of a cloned `Directive` keeps each
/// handle small: cloning the directive would deep-copy its whole block
/// subtree, which is O(n^2) host memory when a plugin requests handles for
/// every directive of a large config.
pub struct DirectiveResource {
    config: Arc<Config>,
    /// Indices locating the directive: each element is the position of a
    /// directive within the current `ConfigItem` list, descending into that
    /// directive's block items for the next element.
    path: Vec<usize>,
}

/// Generated bindings from WIT file, isolated in a submodule to avoid name conflicts
pub(crate) mod bindings {
    wasmtime::component::bindgen!({
        path: "wit/nginx-lint-plugin.wit",
        world: "plugin",
        with: {
            "nginx-lint:plugin/config-api.config": super::ConfigResource,
            "nginx-lint:plugin/config-api.directive": super::DirectiveResource,
        },
    });
}

/// Bindings for the `plugin-rules` world. It imports the same interfaces
/// as the `plugin` world, so they are mapped onto the modules generated
/// above rather than generated again: the one set of `Host` impls below
/// serves both worlds.
pub(crate) mod rules_bindings {
    wasmtime::component::bindgen!({
        path: "wit/nginx-lint-plugin.wit",
        world: "plugin-rules",
        with: {
            "nginx-lint:plugin/types": super::bindings::nginx_lint::plugin::types,
            "nginx-lint:plugin/data-types": super::bindings::nginx_lint::plugin::data_types,
            "nginx-lint:plugin/parser-types": super::bindings::nginx_lint::plugin::parser_types,
            "nginx-lint:plugin/config-api": super::bindings::nginx_lint::plugin::config_api,
        },
    });
}

pub(crate) use bindings::nginx_lint::plugin::config_api;

/// Store data for component model execution
pub(crate) struct ComponentStoreData {
    pub(crate) limits: StoreLimits,
    pub(crate) table: ResourceTable,
    /// Backing state for the `wasi:*` interfaces, built on first use and
    /// therefore never when they are not linked (see [`add_wasi_subset`]) —
    /// a store is created per plugin per file, and the builder seeds a random
    /// generator from the operating system every time. Built empty: no
    /// preopened directories, no environment, no arguments, no stdio.
    pub(crate) wasi: Option<wasmtime_wasi::WasiCtx>,
}

/// The empty WASI context every plugin store gets.
///
/// `WasiCtxBuilder` is deny-by-default, so this grants nothing beyond what
/// the linked interfaces imply: the clocks and randomness. Notably stdio
/// stays disconnected. Inheriting stderr would let a plugin write straight to
/// the user's terminal, around the control-character sanitizing that
/// `sanitize_text` does at the WIT boundary.
fn empty_wasi_ctx() -> wasmtime_wasi::WasiCtx {
    wasmtime_wasi::WasiCtxBuilder::new().build()
}

impl wasmtime_wasi::WasiView for ComponentStoreData {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        wasmtime_wasi::WasiCtxView {
            // Only a linked interface reaches this, so the context is built
            // for the plugins that were granted WASI and for no others.
            ctx: self.wasi.get_or_insert_with(empty_wasi_ctx),
            table: &mut self.table,
        }
    }
}

/// `HasData` marker for the `wasi:io` interfaces, which are backed by the
/// resource table alone. `wasmtime-wasi` keeps its own equivalent private, so
/// linking those interfaces individually means declaring one here.
struct HasIo;

impl wasmtime::component::HasData for HasIo {
    type Data<'a> = &'a mut ResourceTable;
}

/// Link the `wasi:*` interfaces a plugin may import.
///
/// A deliberate subset rather than `wasmtime_wasi::p2::add_to_linker_sync`,
/// which also links `wasi:sockets/*`. No plugin has any use for sockets, and
/// leaving them out means they are absent from the linker rather than merely
/// denied at runtime by an empty context.
///
/// The subset is what a Go plugin imports: componentize-go adapts a wasip1
/// module, so the Go runtime pulls in stdio, environment, clocks, preopens and
/// randomness even for a plugin that only walks the config it is handed.
/// Backed by [`empty_wasi_ctx`], the capabilities that actually result are the
/// two clocks and randomness.
///
/// Note that linking these gives up part of the execution timeout: epoch
/// interruption traps at instruction boundaries, so a plugin blocked inside a
/// host call (`wasi:io/poll` on a timer, say) runs past its deadline until the
/// call returns.
pub(crate) fn add_wasi_subset(
    linker: &mut wasmtime::component::Linker<ComponentStoreData>,
) -> wasmtime::Result<()> {
    // The view traits are blanket-implemented for every `WasiView` and supply
    // the per-interface accessors the linkers take
    use wasmtime_wasi::cli::{WasiCli, WasiCliView};
    use wasmtime_wasi::clocks::{WasiClocks, WasiClocksView};
    use wasmtime_wasi::filesystem::{WasiFilesystem, WasiFilesystemView};
    use wasmtime_wasi::p2::bindings::{cli, clocks, filesystem, random, sync};
    use wasmtime_wasi::random::{WasiRandom, WasiRandomView};

    type T = ComponentStoreData;
    let l = linker;

    clocks::wall_clock::add_to_linker::<T, WasiClocks>(l, T::clocks)?;
    clocks::monotonic_clock::add_to_linker::<T, WasiClocks>(l, T::clocks)?;
    random::random::add_to_linker::<T, WasiRandom>(l, T::random)?;
    cli::exit::add_to_linker::<T, WasiCli>(l, T::cli)?;
    cli::environment::add_to_linker::<T, WasiCli>(l, T::cli)?;
    cli::stdin::add_to_linker::<T, WasiCli>(l, T::cli)?;
    cli::stdout::add_to_linker::<T, WasiCli>(l, T::cli)?;
    cli::stderr::add_to_linker::<T, WasiCli>(l, T::cli)?;
    cli::terminal_input::add_to_linker::<T, WasiCli>(l, T::cli)?;
    cli::terminal_output::add_to_linker::<T, WasiCli>(l, T::cli)?;
    cli::terminal_stdin::add_to_linker::<T, WasiCli>(l, T::cli)?;
    cli::terminal_stdout::add_to_linker::<T, WasiCli>(l, T::cli)?;
    cli::terminal_stderr::add_to_linker::<T, WasiCli>(l, T::cli)?;
    filesystem::preopens::add_to_linker::<T, WasiFilesystem>(l, T::filesystem)?;
    sync::filesystem::types::add_to_linker::<T, WasiFilesystem>(l, T::filesystem)?;
    wasmtime_wasi::p2::bindings::io::error::add_to_linker::<T, HasIo>(l, |t| {
        wasmtime_wasi::WasiView::ctx(t).table
    })?;
    sync::io::poll::add_to_linker::<T, HasIo>(l, |t| wasmtime_wasi::WasiView::ctx(t).table)?;
    sync::io::streams::add_to_linker::<T, HasIo>(l, |t| wasmtime_wasi::WasiView::ctx(t).table)?;
    Ok(())
}

impl wasmtime::component::HasData for ComponentStoreData {
    type Data<'a> = &'a mut ComponentStoreData;
}

/// Helper methods for resource table access.
///
/// These methods panic on invalid handles or table exhaustion. In the wasmtime
/// runtime context, panics in host functions are caught and converted to traps,
/// so the host process will not crash. The descriptive panic messages appear in
/// the trap error for debugging.
impl ComponentStoreData {
    fn get_directive(&self, self_: &Resource<DirectiveResource>) -> &ast::Directive {
        let resource = self
            .table
            .get(self_)
            .expect("invalid directive resource handle");
        resolve_directive(&resource.config, &resource.path)
    }

    fn get_config(&self, self_: &Resource<ConfigResource>) -> &Arc<Config> {
        &self
            .table
            .get(self_)
            .expect("invalid config resource handle")
            .config
    }

    /// Push a directive path into the resource table.
    ///
    /// Panics (trapped by wasmtime) if the resource table is full. This can
    /// happen if an untrusted plugin requests an excessive number of handles.
    /// The execution timeout should prevent this in practice.
    fn push_directive(
        &mut self,
        config: Arc<Config>,
        path: Vec<usize>,
    ) -> Resource<DirectiveResource> {
        self.table
            .push(DirectiveResource { config, path })
            .expect("resource table full: too many directive handles allocated")
    }
}

/// Resolve a directive path (see [`DirectiveResource`]) to the directive it
/// points at. Panics (trapped by wasmtime) on a path that does not point at
/// a directive; paths are host-constructed, so this only happens on a host
/// bug, not on plugin input.
fn resolve_directive<'a>(config: &'a Config, path: &[usize]) -> &'a ast::Directive {
    let mut items: &[ast::ConfigItem] = &config.items;
    let mut found: Option<&'a ast::Directive> = None;
    for &index in path {
        let ast::ConfigItem::Directive(directive) =
            items.get(index).expect("invalid directive path index")
        else {
            panic!("directive path does not point at a directive");
        };
        items = directive
            .block
            .as_ref()
            .map(|block| block.items.as_slice())
            .unwrap_or(&[]);
        found = Some(directive);
    }
    found.expect("empty directive path")
}

/// Resolve a directive path to the directive's block items. An empty path
/// resolves to the config's top-level items; a directive without a block
/// resolves to an empty slice.
fn resolve_block_items<'a>(config: &'a Config, path: &[usize]) -> &'a [ast::ConfigItem] {
    if path.is_empty() {
        return &config.items;
    }
    resolve_directive(config, path)
        .block
        .as_ref()
        .map(|block| block.items.as_slice())
        .unwrap_or(&[])
}

// === Host trait implementations ===

impl bindings::nginx_lint::plugin::types::Host for ComponentStoreData {}

impl bindings::nginx_lint::plugin::data_types::Host for ComponentStoreData {}

impl bindings::nginx_lint::plugin::parser_types::Host for ComponentStoreData {}

impl config_api::Host for ComponentStoreData {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A component whose only import is `wasi:cli/environment`. The signature
    /// has to match the real interface, otherwise the linker rejects it on
    /// types rather than on the interface being absent.
    const IMPORTS_WASI_ENVIRONMENT: &str = r#"(component
        (import "wasi:cli/environment@0.2.12" (instance
            (export "get-environment" (func (result (list (tuple string string)))))
        ))
    )"#;

    /// Resolve a component's imports against a linker built the way
    /// [`ComponentLintRule::load`] builds one, with or without the WASI subset.
    fn instantiates_with_wasi(wat: &str, allow_wasi: bool) -> Result<(), String> {
        let engine = wasmtime::Engine::default();
        let component =
            wasmtime::component::Component::new(&engine, wat).expect("component should compile");
        let mut linker = wasmtime::component::Linker::<ComponentStoreData>::new(&engine);
        if allow_wasi {
            add_wasi_subset(&mut linker).expect("WASI subset should link");
        }
        linker
            .instantiate_pre(&component)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Whether the WASI subset puts `interface` in the linker, probed by
    /// defining a name inside it: wasmtime refuses a duplicate definition, so
    /// a conflict means the interface is already there.
    fn wasi_subset_links(interface: &str, func: &str) -> bool {
        let engine = wasmtime::Engine::default();
        let mut linker = wasmtime::component::Linker::<ComponentStoreData>::new(&engine);
        add_wasi_subset(&mut linker).expect("WASI subset should link");
        linker
            .instance(interface)
            .expect("linker instance")
            .func_wrap(
                func,
                |_: wasmtime::StoreContextMut<'_, ComponentStoreData>, (): ()| Ok(()),
            )
            .is_err()
    }

    #[test]
    fn wasi_import_is_rejected_unless_allowed() {
        // The guarantee every plugin has had until now: a component that
        // wants WASI does not load at all.
        let err = instantiates_with_wasi(IMPORTS_WASI_ENVIRONMENT, false).unwrap_err();
        assert!(
            err.contains("wasi:cli/environment"),
            "unexpected error: {err}"
        );
        instantiates_with_wasi(IMPORTS_WASI_ENVIRONMENT, true)
            .expect("should resolve once WASI is allowed");
    }

    #[test]
    fn sockets_are_never_linked() {
        // The reason add_wasi_subset lists interfaces instead of calling
        // add_to_linker_sync: no plugin has any use for sockets, so they stay
        // absent from the linker rather than merely denied by an empty
        // context.
        assert!(
            wasi_subset_links("wasi:cli/environment@0.2.12", "get-environment"),
            "the probe should detect an interface the subset does link"
        );
        assert!(!wasi_subset_links(
            "wasi:sockets/instance-network@0.2.12",
            "instance-network"
        ));
        assert!(!wasi_subset_links(
            "wasi:sockets/tcp@0.2.12",
            "[method]tcp-socket.start-bind"
        ));
    }
}
