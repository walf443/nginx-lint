//! Plugin system for custom lint rules
//!
//! This module provides support for loading and executing custom lint rules
//! implemented as WebAssembly modules or native Rust plugins.

#[cfg(feature = "plugins")]
pub mod builtin;
#[cfg(feature = "native-plugins")]
pub mod native_builtin;
#[cfg(feature = "plugins")]
mod error;
#[cfg(feature = "plugins")]
mod loader;
#[cfg(feature = "plugins")]
mod wasm_rule;

#[cfg(feature = "plugins")]
pub use error::PluginError;
#[cfg(feature = "plugins")]
pub use loader::PluginLoader;
#[cfg(feature = "plugins")]
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
