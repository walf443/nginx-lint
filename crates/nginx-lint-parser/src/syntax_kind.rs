//! Syntax kind definitions for the rowan-based nginx config parser.
//!
//! Each variant of [`SyntaxKind`] represents either a leaf token or an interior
//! node in the lossless concrete syntax tree.

/// All token and node kinds used by the nginx configuration parser.
///
/// Token kinds (leaf nodes) represent individual lexical elements such as
/// identifiers, strings, punctuation, and whitespace.  Node kinds (interior
/// nodes) group tokens into higher-level constructs like directives and blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(non_camel_case_types)]
#[repr(u16)]
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

    /// Sentinel – not a real kind; used to derive the count.
    #[doc(hidden)]
    __LAST,
}

impl SyntaxKind {
    /// Returns `true` for trivia tokens (whitespace, newlines, comments).
    pub fn is_trivia(self) -> bool {
        matches!(self, Self::WHITESPACE | Self::NEWLINE | Self::COMMENT)
    }
}

/// Converts `SyntaxKind` to a raw `u16` for rowan.
impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

/// The language tag used by rowan to parameterise the syntax tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NginxLanguage {}

impl rowan::Language for NginxLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 < SyntaxKind::__LAST as u16);
        // SAFETY: SyntaxKind is `#[repr(u16)]` and we checked the range.
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
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

    #[test]
    fn kind_round_trip() {
        for raw in 0..SyntaxKind::__LAST as u16 {
            let kind: SyntaxKind = unsafe { std::mem::transmute(raw) };
            let rowan_kind: rowan::SyntaxKind = kind.into();
            assert_eq!(rowan_kind.0, raw);
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
