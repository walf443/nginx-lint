//! Syntax kind definitions for the rowan-based nginx config parser.
//!
//! Each variant of [`SyntaxKind`] represents either a leaf token or an interior
//! node in the lossless concrete syntax tree.

/// All token and node kinds used by the nginx configuration parser.
///
/// Token kinds (leaf nodes) represent individual lexical elements such as
/// identifiers, strings, punctuation, and whitespace.  Node kinds (interior
/// nodes) group tokens into higher-level constructs like directives and blocks.
///
/// **Maintenance note:** When adding a new variant, you must also update
/// `to_raw()`, `from_raw()`, and the `ALL_KINDS` array in the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // ── Tokens (leaf nodes) ─────────────────────────────────────────
    /// Horizontal whitespace (spaces / tabs).
    WHITESPACE = 0,
    /// A newline character (`\n`).
    NEWLINE,
    /// A comment (`# …` through end of line).
    COMMENT,
    /// A directive name (alphabetic / `_` start, may contain `-`).
    IDENT,
    /// An unquoted argument (numbers, paths, regex fragments, …).
    ARGUMENT,
    /// A double-quoted string (`"…"`), including the quotes.
    DOUBLE_QUOTED_STRING,
    /// A single-quoted string (`'…'`), including the quotes.
    SINGLE_QUOTED_STRING,
    /// A variable reference (`$var` or `${var}`).
    VARIABLE,
    /// Semicolon (`;`).
    SEMICOLON,
    /// Opening brace (`{`).
    L_BRACE,
    /// Closing brace (`}`).
    R_BRACE,
    /// Raw content inside a lua-block or similar directive.
    RAW_CONTENT,
    /// A token that the lexer could not classify (error recovery).
    ERROR,

    // ── Composite nodes (interior nodes) ────────────────────────────
    /// The root node, corresponding to `Config`.
    ROOT,
    /// A directive node (name + arguments + optional block).
    DIRECTIVE,
    /// A brace-delimited block (`{ … }`).
    BLOCK,
    /// A blank line (whitespace-only line).
    BLANK_LINE,
}

impl SyntaxKind {
    /// Returns `true` for trivia tokens (whitespace, newlines, comments).
    pub fn is_trivia(self) -> bool {
        matches!(self, Self::WHITESPACE | Self::NEWLINE | Self::COMMENT)
    }

    /// Convert a `SyntaxKind` to its raw `u16` discriminant.
    fn to_raw(self) -> u16 {
        match self {
            Self::WHITESPACE => 0,
            Self::NEWLINE => 1,
            Self::COMMENT => 2,
            Self::IDENT => 3,
            Self::ARGUMENT => 4,
            Self::DOUBLE_QUOTED_STRING => 5,
            Self::SINGLE_QUOTED_STRING => 6,
            Self::VARIABLE => 7,
            Self::SEMICOLON => 8,
            Self::L_BRACE => 9,
            Self::R_BRACE => 10,
            Self::RAW_CONTENT => 11,
            Self::ERROR => 12,
            Self::ROOT => 13,
            Self::DIRECTIVE => 14,
            Self::BLOCK => 15,
            Self::BLANK_LINE => 16,
        }
    }
}

/// Converts `SyntaxKind` to a raw `u16` for rowan.
impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind.to_raw())
    }
}

/// The language tag used by rowan to parameterise the syntax tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NginxLanguage {}

impl SyntaxKind {
    /// Convert a raw `u16` discriminant back to a `SyntaxKind`.
    ///
    /// Panics if the value is out of range.
    fn from_raw(raw: u16) -> Self {
        match raw {
            0 => Self::WHITESPACE,
            1 => Self::NEWLINE,
            2 => Self::COMMENT,
            3 => Self::IDENT,
            4 => Self::ARGUMENT,
            5 => Self::DOUBLE_QUOTED_STRING,
            6 => Self::SINGLE_QUOTED_STRING,
            7 => Self::VARIABLE,
            8 => Self::SEMICOLON,
            9 => Self::L_BRACE,
            10 => Self::R_BRACE,
            11 => Self::RAW_CONTENT,
            12 => Self::ERROR,
            13 => Self::ROOT,
            14 => Self::DIRECTIVE,
            15 => Self::BLOCK,
            16 => Self::BLANK_LINE,
            _ => panic!("invalid SyntaxKind raw value: {raw}"),
        }
    }
}

impl rowan::Language for NginxLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        SyntaxKind::from_raw(raw.0)
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

/// A node in the nginx configuration syntax tree.
pub type SyntaxNode = rowan::SyntaxNode<NginxLanguage>;
/// A token (leaf) in the nginx configuration syntax tree.
pub type SyntaxToken = rowan::SyntaxToken<NginxLanguage>;
/// Either a node or a token.
pub type SyntaxElement = rowan::SyntaxElement<NginxLanguage>;

#[cfg(test)]
mod tests {
    use super::*;
    use rowan::Language;

    /// All valid `SyntaxKind` variants (excluding `__LAST`).
    const ALL_KINDS: &[SyntaxKind] = &[
        SyntaxKind::WHITESPACE,
        SyntaxKind::NEWLINE,
        SyntaxKind::COMMENT,
        SyntaxKind::IDENT,
        SyntaxKind::ARGUMENT,
        SyntaxKind::DOUBLE_QUOTED_STRING,
        SyntaxKind::SINGLE_QUOTED_STRING,
        SyntaxKind::VARIABLE,
        SyntaxKind::SEMICOLON,
        SyntaxKind::L_BRACE,
        SyntaxKind::R_BRACE,
        SyntaxKind::RAW_CONTENT,
        SyntaxKind::ERROR,
        SyntaxKind::ROOT,
        SyntaxKind::DIRECTIVE,
        SyntaxKind::BLOCK,
        SyntaxKind::BLANK_LINE,
    ];

    #[test]
    fn kind_round_trip() {
        for (raw, &expected) in ALL_KINDS.iter().enumerate() {
            let kind = SyntaxKind::from_raw(raw as u16);
            assert_eq!(kind, expected);
            let rowan_kind: rowan::SyntaxKind = kind.into();
            assert_eq!(rowan_kind.0, raw as u16);
            let back = NginxLanguage::kind_from_raw(rowan_kind);
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn trivia_classification() {
        assert!(SyntaxKind::WHITESPACE.is_trivia());
        assert!(SyntaxKind::NEWLINE.is_trivia());
        assert!(SyntaxKind::COMMENT.is_trivia());
        assert!(!SyntaxKind::IDENT.is_trivia());
        assert!(!SyntaxKind::SEMICOLON.is_trivia());
    }
}
