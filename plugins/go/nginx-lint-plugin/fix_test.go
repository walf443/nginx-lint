package nginxlint_test

import (
	"testing"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
)

// The fixtures below describe this configuration, byte for byte, because the
// fix builders are pure offset arithmetic and have to agree with what the
// host would have computed from the same directive:
//
//	http {
//	    server_tokens on;
//	}
//
// `http` starts at offset 0 in column 1. Its block opens at offset 5, so the
// indented directive's name begins at offset 11 in column 5, four spaces of
// leading whitespace after the newline at offset 6, and ends after the
// semicolon at offset 28.
func indented() nginxlint.Directive {
	return nginxlint.Directive{
		Name:              "server_tokens",
		Args:              []nginxlint.Argument{{Value: "on"}},
		Line:              2,
		Column:            5,
		StartOffset:       11,
		EndOffset:         28,
		LeadingWhitespace: "    ",
	}
}

func topLevel() nginxlint.Directive {
	return nginxlint.Directive{Name: "http", Line: 1, Column: 1, StartOffset: 0, EndOffset: 31}
}

func assertRange(t *testing.T, fix nginxlint.Fix, start, end uint32, text string) {
	t.Helper()
	if !fix.HasRange {
		t.Fatalf("fix has no range, want %d..%d", start, end)
	}
	if fix.StartOffset != start || fix.EndOffset != end {
		t.Errorf("range = %d..%d, want %d..%d", fix.StartOffset, fix.EndOffset, start, end)
	}
	if fix.NewText != text {
		t.Errorf("NewText = %q, want %q", fix.NewText, text)
	}
}

func TestReplaceWithCoversTheIndentation(t *testing.T) {
	// The range starts at 7, not 11: replacing from the name alone would
	// leave the original indentation in front of the new text.
	assertRange(t, indented().ReplaceWith("server_tokens off;"),
		7, 28, "    server_tokens off;")
}

func TestReplaceWithOnAnUnindentedDirective(t *testing.T) {
	assertRange(t, topLevel().ReplaceWith("stream {"), 0, 31, "stream {")
}

func TestInsertAfterOpensANewLineAtTheSameColumn(t *testing.T) {
	assertRange(t, indented().InsertAfter("gzip on;"), 28, 28, "\n    gzip on;")
}

func TestInsertBeforeClosesItsLineAtTheSameColumn(t *testing.T) {
	assertRange(t, indented().InsertBefore("gzip on;"), 7, 7, "    gzip on;\n")
}

func TestInsertAfterMany(t *testing.T) {
	assertRange(t, indented().InsertAfterMany("gzip on;", "gzip_vary on;"),
		28, 28, "\n    gzip on;\n    gzip_vary on;")
}

func TestInsertBeforeMany(t *testing.T) {
	assertRange(t, indented().InsertBeforeMany("gzip on;", "gzip_vary on;"),
		7, 7, "    gzip on;\n    gzip_vary on;\n")
}

func TestDeleteLineIsALineFixNotARange(t *testing.T) {
	fix := indented().DeleteLine()

	if !fix.DeleteLine {
		t.Error("DeleteLine() did not set DeleteLine")
	}
	if fix.Line != 2 {
		t.Errorf("Line = %d, want 2", fix.Line)
	}
	if fix.HasRange {
		t.Error("DeleteLine() set a range; the host applies it by line")
	}
}

func TestOffsetsNeverWrapAround(t *testing.T) {
	// A directive whose recorded column or leading whitespace runs past its
	// offset would underflow unsigned arithmetic into an enormous offset,
	// which the host would then apply somewhere far away in the file.
	directive := nginxlint.Directive{
		Name:              "http",
		Column:            9,
		StartOffset:       2,
		EndOffset:         6,
		LeadingWhitespace: "        ",
	}

	assertRange(t, directive.ReplaceWith("stream {"), 0, 6, "        stream {")
	assertRange(t, directive.InsertBefore("# note"), 0, 0, "        # note\n")
}
