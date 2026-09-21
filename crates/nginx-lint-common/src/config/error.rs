//! Why a configuration file could not be loaded.

/// Error returned when loading or parsing a configuration file fails.
#[derive(Debug)]
pub enum ConfigError {
    /// The file could not be read (missing, permission denied, etc.).
    IoError {
        /// Path that was attempted.
        path: std::path::PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The file was read but contains invalid TOML.
    ParseError {
        /// Path that was parsed.
        path: std::path::PathBuf,
        /// Underlying TOML parse error.
        source: toml::de::Error,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::IoError { path, source } => {
                write!(
                    f,
                    "Failed to read config file '{}': {}",
                    path.display(),
                    source
                )
            }
            ConfigError::ParseError { path, source } => {
                write!(
                    f,
                    "Failed to parse config file '{}': {}",
                    path.display(),
                    source
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::IoError { source, .. } => Some(source),
            ConfigError::ParseError { source, .. } => Some(source),
        }
    }
}
