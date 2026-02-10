//! Error types for the nginx configuration parser.
//!
//! Errors are split into two stages:
//!
//! - [`LexerError`] — failures during tokenization (unterminated strings, unexpected characters).
//! - [`ParseError`] — failures during parsing (unexpected tokens, unclosed blocks, missing semicolons).
//!
//! Both carry a [`Position`] so that error messages can
//! point to the exact line and column in the source.

use crate::ast::Position;
use std::fmt;
use thiserror::Error;

/// An error that occurs during tokenization (lexing).
#[derive(Debug, Clone, Error)]
pub enum LexerError {
    /// A quoted string was opened but never closed before end-of-file.
    #[error("Unterminated string starting at line {}, column {}", .position.line, .position.column)]
    UnterminatedString { position: Position },

    /// A backslash escape sequence was not recognized.
    #[error("Invalid escape sequence '\\{ch}' at line {}, column {}", .position.line, .position.column)]
    InvalidEscapeSequence { ch: char, position: Position },

    /// A character was encountered that is not valid in any token position.
    #[error("Unexpected character '{ch}' at line {}, column {}", .position.line, .position.column)]
    UnexpectedChar { ch: char, position: Position },
}

impl LexerError {
    /// Returns the source position where this error occurred.
    pub fn position(&self) -> Position {
        match self {
            LexerError::UnterminatedString { position } => *position,
            LexerError::InvalidEscapeSequence { position, .. } => *position,
            LexerError::UnexpectedChar { position, .. } => *position,
        }
    }
}

/// An error that occurs during parsing.
///
/// Includes both parse-level errors and forwarded [`LexerError`]s.
#[derive(Debug, Clone, Error)]
pub enum ParseError {
    /// A tokenization error propagated from the lexer.
    #[error("{0}")]
    Lexer(#[from] LexerError),

    /// The parser found a different token than expected.
    #[error("Expected '{expected}' but found '{found}' at line {}, column {}", .position.line, .position.column)]
    UnexpectedToken {
        expected: String,
        found: String,
        position: Position,
    },

    /// The input ended while the parser still expected more tokens.
    #[error("Unexpected end of file at line {}, column {}", .position.line, .position.column)]
    UnexpectedEof { position: Position },

    /// An identifier was expected at the start of a directive but not found.
    #[error("Expected directive name at line {}, column {}", .position.line, .position.column)]
    ExpectedDirectiveName { position: Position },

    /// A directive was not terminated with `;`.
    #[error("Missing semicolon at line {}, column {}", .position.line, .position.column)]
    MissingSemicolon { position: Position },

    /// A `}` was found without a matching `{`.
    #[error("Unmatched closing brace at line {}, column {}", .position.line, .position.column)]
    UnmatchedCloseBrace { position: Position },

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
            ParseError::Lexer(e) => Some(e.position()),
            ParseError::UnexpectedToken { position, .. } => Some(*position),
            ParseError::UnexpectedEof { position } => Some(*position),
            ParseError::ExpectedDirectiveName { position } => Some(*position),
            ParseError::MissingSemicolon { position } => Some(*position),
            ParseError::UnmatchedCloseBrace { position } => Some(*position),
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
