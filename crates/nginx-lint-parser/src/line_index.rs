//! Byte-offset → line/column conversion for rowan CST nodes.
//!
//! Rowan provides byte-offset ranges via `text_range()`. This module builds an
//! index of newline positions so that offsets can be efficiently mapped to the
//! 1-based `(line, column)` pairs used by the existing AST types.

use crate::ast::{Position, Span};

/// Pre-computed index of line-start byte offsets for a source string.
///
/// Construct with [`LineIndex::new`], then call [`position`](LineIndex::position)
/// or [`span`](LineIndex::span) to convert rowan `TextRange` values into AST
/// [`Position`] / [`Span`].
pub struct LineIndex {
    /// Byte offsets where each line begins. `line_starts[0]` is always `0`.
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build a line index from the full source text.
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts }
    }

    /// Convert a byte offset to a 1-based `Position`.
    pub fn position(&self, offset: usize) -> Position {
        // Binary search for the line containing `offset`.
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,  // offset is at a line start
            Err(ins) => ins - 1, // offset is within the preceding line
        };
        let col = offset - self.line_starts[line_idx];
        Position::new(line_idx + 1, col + 1, offset)
    }

    /// Convert a rowan `TextRange` to an AST `Span`.
    pub fn span(&self, range: rowan::TextRange) -> Span {
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        Span::new(self.position(start), self.position(end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line() {
        let idx = LineIndex::new("listen 80;");
        assert_eq!(idx.position(0), Position::new(1, 1, 0));
        assert_eq!(idx.position(7), Position::new(1, 8, 7));
    }

    #[test]
    fn multi_line() {
        let src = "http {\n    listen 80;\n}\n";
        let idx = LineIndex::new(src);
        // line 1: "http {\n"  offsets 0..7
        assert_eq!(idx.position(0), Position::new(1, 1, 0));
        // line 2: "    listen 80;\n"  starts at offset 7
        assert_eq!(idx.position(7), Position::new(2, 1, 7));
        assert_eq!(idx.position(11), Position::new(2, 5, 11)); // 'l' of listen
        // line 3: "}\n"  starts at offset 22
        assert_eq!(idx.position(22), Position::new(3, 1, 22));
    }

    #[test]
    fn span_conversion() {
        let src = "listen 80;";
        let idx = LineIndex::new(src);
        let range = rowan::TextRange::new(0.into(), 6.into());
        let span = idx.span(range);
        assert_eq!(span.start, Position::new(1, 1, 0));
        assert_eq!(span.end, Position::new(1, 7, 6));
    }
}
