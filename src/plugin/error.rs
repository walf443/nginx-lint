//! Plugin error types

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur when loading or executing plugins
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("Failed to read plugin file '{path}': {source}")]
    IoError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to compile WASM module '{path}': {message}")]
    CompileError { path: PathBuf, message: String },

    #[error("Failed to instantiate WASM module '{path}': {message}")]
    InstantiateError { path: PathBuf, message: String },

    #[error("Missing required export '{export}' in plugin '{path}'")]
    MissingExport { path: PathBuf, export: String },

    #[error("Invalid plugin info from '{path}': {message}")]
    InvalidPluginInfo { path: PathBuf, message: String },

    #[error("Plugin execution error in '{path}': {message}")]
    ExecutionError { path: PathBuf, message: String },

    #[error("Plugin execution timed out in '{path}'")]
    Timeout { path: PathBuf },

    #[error("Plugin exceeded memory limit in '{path}'")]
    MemoryLimitExceeded { path: PathBuf },

    #[error("Failed to parse plugin result from '{path}': {message}")]
    ResultParseError { path: PathBuf, message: String },

    #[error("Plugin directory not found: {path}")]
    DirectoryNotFound { path: PathBuf },

    #[error("Invalid WASM file '{path}': not a valid WebAssembly module")]
    InvalidWasmFile { path: PathBuf },
}

impl PluginError {
    pub fn io_error(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::IoError {
            path: path.into(),
            source,
        }
    }

    pub fn compile_error(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::CompileError {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn instantiate_error(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::InstantiateError {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn missing_export(path: impl Into<PathBuf>, export: impl Into<String>) -> Self {
        Self::MissingExport {
            path: path.into(),
            export: export.into(),
        }
    }

    pub fn invalid_plugin_info(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::InvalidPluginInfo {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn execution_error(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::ExecutionError {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn timeout(path: impl Into<PathBuf>) -> Self {
        Self::Timeout { path: path.into() }
    }

    pub fn memory_limit_exceeded(path: impl Into<PathBuf>) -> Self {
        Self::MemoryLimitExceeded { path: path.into() }
    }

    pub fn result_parse_error(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::ResultParseError {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn directory_not_found(path: impl Into<PathBuf>) -> Self {
        Self::DirectoryNotFound { path: path.into() }
    }

    pub fn invalid_wasm_file(path: impl Into<PathBuf>) -> Self {
        Self::InvalidWasmFile { path: path.into() }
    }
}
