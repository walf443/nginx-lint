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

/// Current API version for the plugin interface.
///
/// This version covers:
/// - Input: The Config/AST JSON structure sent to plugins
/// - Output: The LintError JSON structure returned by plugins
///
/// Plugins declare which API version they use, and the host can support
/// multiple versions for backward compatibility.
pub const API_VERSION: &str = "1.0";
