//! WASM plugin system for custom lint rules
//!
//! This module provides support for loading and executing custom lint rules
//! implemented as WebAssembly modules.

mod error;
mod loader;
mod wasm_rule;

pub use error::PluginError;
pub use loader::PluginLoader;
pub use wasm_rule::WasmLintRule;
