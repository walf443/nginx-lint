use crate::ast::Position;
use std::fmt;
use thiserror::Error;

/// Lexer error (tokenization failure)
#[derive(Debug, Clone, Error)]
pub enum LexerError {
    #[error("Unterminated string starting at line {}, column {}", .position.line, .position.column)]
    UnterminatedString { position: Position },

    #[error("Invalid escape sequence '\\{ch}' at line {}, column {}", .position.line, .position.column)]
    InvalidEscapeSequence { ch: char, position: Position },

    #[error("Unexpected character '{ch}' at line {}, column {}", .position.line, .position.column)]
    UnexpectedChar { ch: char, position: Position },
}

impl LexerError {
    pub fn position(&self) -> Position {
        match self {
            LexerError::UnterminatedString { position } => *position,
            LexerError::InvalidEscapeSequence { position, .. } => *position,
            LexerError::UnexpectedChar { position, .. } => *position,
        }
    }
}

/// Parser error (syntax error)
#[derive(Debug, Clone, Error)]
pub enum ParseError {
    #[error("{0}")]
    Lexer(#[from] LexerError),

    #[error("Expected '{expected}' but found '{found}' at line {}, column {}", .position.line, .position.column)]
    UnexpectedToken {
        expected: String,
        found: String,
        position: Position,
    },

    #[error("Unexpected end of file at line {}, column {}", .position.line, .position.column)]
    UnexpectedEof { position: Position },

    #[error("Expected directive name at line {}, column {}", .position.line, .position.column)]
    ExpectedDirectiveName { position: Position },

    #[error("Missing semicolon at line {}, column {}", .position.line, .position.column)]
    MissingSemicolon { position: Position },

    #[error("Unmatched closing brace at line {}, column {}", .position.line, .position.column)]
    UnmatchedCloseBrace { position: Position },

    #[error("Unclosed block starting at line {}, column {}", .position.line, .position.column)]
    UnclosedBlock { position: Position },

    #[error("Failed to read file: {0}")]
    IoError(String),
}

impl ParseError {
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
