package nginxlint

import "strings"

// Fix is an edit that resolves a finding. Build one with the methods on
// [Directive] rather than by hand; they reproduce, in pure Go, exactly what
// the host's directive resource would have computed, which is what keeps a
// rule testable without a component runtime.
type Fix struct {
	// Line is the 1-based line a whole-line fix applies to, and is unused by
	// a range fix.
	Line uint32
	// OldText is the text being replaced, when the fix names it.
	OldText string
	// NewText is what gets written.
	NewText string
	// DeleteLine removes the whole line.
	DeleteLine bool
	// InsertAfter writes NewText as a new line after Line.
	InsertAfter bool
	// StartOffset and EndOffset bound the byte range a range fix replaces.
	// They are only read when HasRange is set, which is how a zero-length
	// insertion at offset 0 stays distinguishable from an unset range.
	StartOffset uint32
	EndOffset   uint32
	HasRange    bool
}

// replaceRange is the shape every positional fix takes: the host applies
// several of them to one file as long as their ranges do not overlap.
func replaceRange(start, end uint32, newText string) Fix {
	return Fix{NewText: newText, StartOffset: start, EndOffset: end, HasRange: true}
}

// lineStartOffset is where the directive's line begins, and indent is the
// whitespace that would put new text in the same column. Both are derived the
// way the host derives them, from the 1-based column.
func (d Directive) lineStartOffset() uint32 {
	return saturatingSub(d.StartOffset, d.Column-1)
}

func (d Directive) indent() string {
	if d.Column <= 1 {
		return ""
	}
	return strings.Repeat(" ", int(d.Column-1))
}

// fullStartOffset includes the indentation in front of the directive, so that
// replacing it does not leave the old indent behind.
func (d Directive) fullStartOffset() uint32 {
	return saturatingSub(d.StartOffset, uint32(len(d.LeadingWhitespace)))
}

// ReplaceWith rewrites the directive, indentation included, as newText.
func (d Directive) ReplaceWith(newText string) Fix {
	return replaceRange(d.fullStartOffset(), d.EndOffset, d.LeadingWhitespace+newText)
}

// DeleteLine removes the directive's line.
func (d Directive) DeleteLine() Fix {
	return Fix{Line: d.Line, DeleteLine: true}
}

// InsertAfter writes newText on its own line after the directive, at the
// directive's indentation.
func (d Directive) InsertAfter(newText string) Fix {
	return d.InsertAfterMany(newText)
}

// InsertAfterMany writes each line after the directive, at its indentation.
func (d Directive) InsertAfterMany(lines ...string) Fix {
	indent := d.indent()
	var text strings.Builder
	for _, line := range lines {
		text.WriteString("\n")
		text.WriteString(indent)
		text.WriteString(line)
	}
	return replaceRange(d.EndOffset, d.EndOffset, text.String())
}

// InsertBefore writes newText on its own line before the directive, at the
// directive's indentation.
func (d Directive) InsertBefore(newText string) Fix {
	return d.InsertBeforeMany(newText)
}

// InsertBeforeMany writes each line before the directive, at its indentation.
func (d Directive) InsertBeforeMany(lines ...string) Fix {
	indent := d.indent()
	var text strings.Builder
	for _, line := range lines {
		text.WriteString(indent)
		text.WriteString(line)
		text.WriteString("\n")
	}
	offset := d.lineStartOffset()
	return replaceRange(offset, offset, text.String())
}

// saturatingSub keeps an offset from wrapping around: unsigned arithmetic on
// a directive at the very start of a file would otherwise produce an enormous
// offset instead of zero.
func saturatingSub(value, amount uint32) uint32 {
	if amount > value {
		return 0
	}
	return value - amount
}
