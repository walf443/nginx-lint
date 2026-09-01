package nginxlinttest

import (
	"encoding/json"
	"fmt"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
)

// jsonFix is a fix in the shape the linter's own applier reads.
type jsonFix struct {
	Line        uint32  `json:"line"`
	OldText     *string `json:"old_text,omitempty"`
	NewText     string  `json:"new_text"`
	DeleteLine  bool    `json:"delete_line"`
	InsertAfter bool    `json:"insert_after"`
	StartOffset *uint32 `json:"start_offset,omitempty"`
	EndOffset   *uint32 `json:"end_offset,omitempty"`
}

func toJSONFix(fix nginxlint.Fix) jsonFix {
	converted := jsonFix{
		Line:        fix.Line,
		NewText:     fix.NewText,
		DeleteLine:  fix.DeleteLine,
		InsertAfter: fix.InsertAfter,
	}
	if fix.OldText != "" {
		converted.OldText = &fix.OldText
	}
	if fix.HasRange {
		start, end := fix.StartOffset, fix.EndOffset
		converted.StartOffset, converted.EndOffset = &start, &end
	}
	return converted
}

type applyRequest struct {
	Content string    `json:"content"`
	Fixes   []jsonFix `json:"fixes"`
}

type applyResponse struct {
	Content        string `json:"content"`
	Applied        int    `json:"applied"`
	SkippedInvalid int    `json:"skipped_invalid"`
	Error          string `json:"error"`
}

// FixResult is what applying a rule's fixes produced.
type FixResult struct {
	Content string
	// Applied counts the fixes that made it into Content. It is the number to
	// compare against how many were submitted: a fix can also be dropped for
	// overlapping one already applied, and the applier does not count those
	// anywhere.
	Applied int
	// SkippedInvalid counts fixes the applier rejected outright: a range out
	// of bounds or splitting a character, or a line-based fix naming a line
	// that is not there.
	SkippedInvalid int
}

// ApplyFixes applies fixes to content with the linter's own applier — the
// function behind `--fix` — rather than a reimplementation of it.
func ApplyFixes(content string, fixes []nginxlint.Fix) (FixResult, error) {
	request := applyRequest{Content: content, Fixes: make([]jsonFix, 0, len(fixes))}
	for _, fix := range fixes {
		request.Fixes = append(request.Fixes, toJSONFix(fix))
	}

	encoded, err := json.Marshal(request)
	if err != nil {
		return FixResult{}, err
	}

	out, err := call(fixerModule, "apply_fixes_json", encoded)
	if err != nil {
		return FixResult{}, err
	}

	var response applyResponse
	if err := json.Unmarshal(out, &response); err != nil {
		return FixResult{}, fmt.Errorf("decoding the fix result: %w", err)
	}
	if response.Error != "" {
		return FixResult{}, fmt.Errorf("%s", response.Error)
	}

	return FixResult{
		Content:        response.Content,
		Applied:        response.Applied,
		SkippedInvalid: response.SkippedInvalid,
	}, nil
}
