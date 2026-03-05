//! Error types for the nginx configuration parser.
//!
//! [`ParseError`] covers failures during parsing (unexpected tokens, unclosed
//! blocks, I/O errors). Each variant carries a [`Position`] so that error
//! messages can point to the exact line and column in the source.

use crate::ast::Position;
use std::fmt;
use thiserror::Error;

/// An error that occurs during parsing.
#[derive(Debug, Clone, Error)]
pub enum ParseError {
    /// The parser found a different token than expected.
    #[error("Expected '{expected}' but found '{found}' at line {}, column {}", .position.line, .position.column)]
    UnexpectedToken {
        expected: String,
        found: String,
        position: Position,
    },

    /// A `{` was opened but never closed before end-of-file.
    #[error("Unclosed block starting at line {}, column {}", .position.line, .position.column)]
    UnclosedBlock { position: Position },

    /// A file could not be read from disk.
    #[error("Failed to read file: {0}")]
    IoError(String),
}

impl ParseError {
    /// Returns the source position where this error occurred, if available.
    ///
    /// Returns `None` only for [`IoError`](ParseError::IoError) which has no
    /// source position.
    pub fn position(&self) -> Option<Position> {
        match self {
            ParseError::UnexpectedToken { position, .. } => Some(*position),
            ParseError::UnclosedBlock { position } => Some(*position),
            ParseError::IoError(_) => None,
        }
    }
}

/// Result type alias for parser operations
pub type ParseResult<T> = Result<T, ParseError>;

/// Display implementation for user-friendly error messages
impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}
