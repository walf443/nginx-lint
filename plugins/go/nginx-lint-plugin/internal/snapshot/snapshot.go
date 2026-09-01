// Package snapshot is the flat parse output both ways into a
// [nginxlint.Config] go through.
//
// The host hands a plugin this shape through `config.snapshot()`, and the
// parser's JSON entry point emits the same one; the field tags describe that
// JSON. Keeping the walk here rather than in each caller is what stops a rule
// tested against the real parser from seeing a differently shaped
// configuration than the same rule sees at run time.
package snapshot

import nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"

// ParseOutput is the whole configuration as one flat array.
type ParseOutput struct {
	// Items is every directive, comment and blank line in depth-first order.
	Items []Item `json:"all_items"`
	// TopLevelIndices indexes Items for the items at the top of the file.
	TopLevelIndices []uint32 `json:"top_level_indices"`
	// IncludeContext names the blocks this file was included from.
	IncludeContext []string `json:"include_context"`
}

// Item is one entry in the flat array: exactly one of the three pointers is
// set, which is how the WIT variant arrives over JSON.
type Item struct {
	Value ItemValue `json:"value"`
	// ChildIndices indexes Items for a directive block's contents.
	ChildIndices []uint32 `json:"child_indices"`
}

// ItemValue is the variant.
type ItemValue struct {
	Directive *Directive `json:"DirectiveItem"`
	Comment   *Comment   `json:"CommentItem"`
	BlankLine *BlankLine `json:"BlankLineItem"`
}

// Directive is a directive's flat properties.
type Directive struct {
	Name                  string     `json:"name"`
	Args                  []Argument `json:"args"`
	Line                  uint32     `json:"line"`
	Column                uint32     `json:"column"`
	StartOffset           uint32     `json:"start_offset"`
	EndOffset             uint32     `json:"end_offset"`
	EndLine               uint32     `json:"end_line"`
	EndColumn             uint32     `json:"end_column"`
	LeadingWhitespace     string     `json:"leading_whitespace"`
	TrailingWhitespace    string     `json:"trailing_whitespace"`
	SpaceBeforeTerminator string     `json:"space_before_terminator"`
	HasBlock              bool       `json:"has_block"`
	BlockIsRaw            bool       `json:"block_is_raw"`
	// BlockRawContent and TrailingComment are options in the WIT, and arrive
	// as JSON null when absent, which leaves these empty.
	BlockRawContent string `json:"block_raw_content"`
	TrailingComment string `json:"trailing_comment_text"`
}

// Argument is one directive argument.
type Argument struct {
	Value       string       `json:"value"`
	Raw         string       `json:"raw"`
	Type        ArgumentType `json:"arg_type"`
	Line        uint32       `json:"line"`
	Column      uint32       `json:"column"`
	StartOffset uint32       `json:"start_offset"`
	EndOffset   uint32       `json:"end_offset"`
}

// Comment is a standalone comment line.
type Comment struct {
	Text               string `json:"text"`
	Line               uint32 `json:"line"`
	Column             uint32 `json:"column"`
	LeadingWhitespace  string `json:"leading_whitespace"`
	TrailingWhitespace string `json:"trailing_whitespace"`
	StartOffset        uint32 `json:"start_offset"`
	EndOffset          uint32 `json:"end_offset"`
}

// BlankLine is an empty line.
type BlankLine struct {
	Line        uint32 `json:"line"`
	Content     string `json:"content"`
	StartOffset uint32 `json:"start_offset"`
}

// ArgumentType carries the SDK's value. Reading it from the WIT enum's name,
// which is what serde emits over JSON, lives in argument_type_json.go: that
// file is excluded from wasm builds, because pulling encoding/json into a
// component that only ever receives the value from the bindings costs every
// plugin several hundred kilobytes.
type ArgumentType nginxlint.ArgumentType

// Config builds what a check receives.
func (o ParseOutput) Config(path string) nginxlint.Config {
	config := nginxlint.Config{
		Path:           path,
		IncludeContext: o.IncludeContext,
		// An included fragment is nested inside the blocks it was included
		// from, so the include context seeds the parent stack. Without it a
		// rule asking IsInside("http") would be wrong for every file pulled
		// in by an `include`.
		Directives: o.directives(o.TopLevelIndices, o.IncludeContext),
	}
	config.Comments, config.BlankLines = o.trivia()
	return config
}

func (o ParseOutput) directives(indices []uint32, parents []string) []nginxlint.Directive {
	converted := []nginxlint.Directive{}
	for _, index := range indices {
		if int(index) >= len(o.Items) {
			continue
		}
		item := o.Items[index]
		if item.Value.Directive == nil {
			continue
		}
		directive := item.Value.Directive.convert(parents)
		if len(item.ChildIndices) > 0 {
			// Copy rather than append in place: several siblings share this
			// parents slice, and appending to it would let one of them
			// overwrite another's stack.
			nested := append(append([]string{}, parents...), directive.Name)
			directive.Block = o.directives(item.ChildIndices, nested)
		}
		converted = append(converted, directive)
	}
	return converted
}

func (d Directive) convert(parents []string) nginxlint.Directive {
	args := make([]nginxlint.Argument, 0, len(d.Args))
	for _, arg := range d.Args {
		args = append(args, nginxlint.Argument{
			Value:       arg.Value,
			Raw:         arg.Raw,
			Type:        nginxlint.ArgumentType(arg.Type),
			Line:        arg.Line,
			Column:      arg.Column,
			StartOffset: arg.StartOffset,
			EndOffset:   arg.EndOffset,
		})
	}
	return nginxlint.Directive{
		Name:                  d.Name,
		Args:                  args,
		Line:                  d.Line,
		Column:                d.Column,
		StartOffset:           d.StartOffset,
		EndOffset:             d.EndOffset,
		EndLine:               d.EndLine,
		EndColumn:             d.EndColumn,
		LeadingWhitespace:     d.LeadingWhitespace,
		TrailingWhitespace:    d.TrailingWhitespace,
		SpaceBeforeTerminator: d.SpaceBeforeTerminator,
		HasBlock:              d.HasBlock,
		BlockIsRaw:            d.BlockIsRaw,
		BlockRawContent:       d.BlockRawContent,
		TrailingComment:       d.TrailingComment,
		Parents:               parents,
	}
}

// trivia collects the comments and blank lines from the whole file. They are
// flat rather than woven into the directive tree, and the array is already in
// source order, so one pass over it is enough.
func (o ParseOutput) trivia() ([]nginxlint.Comment, []nginxlint.BlankLine) {
	comments := []nginxlint.Comment{}
	blankLines := []nginxlint.BlankLine{}
	for _, item := range o.Items {
		switch {
		case item.Value.Comment != nil:
			found := item.Value.Comment
			comments = append(comments, nginxlint.Comment{
				Text:               found.Text,
				Line:               found.Line,
				Column:             found.Column,
				LeadingWhitespace:  found.LeadingWhitespace,
				TrailingWhitespace: found.TrailingWhitespace,
				StartOffset:        found.StartOffset,
				EndOffset:          found.EndOffset,
			})
		case item.Value.BlankLine != nil:
			found := item.Value.BlankLine
			blankLines = append(blankLines, nginxlint.BlankLine{
				Line:        found.Line,
				Content:     found.Content,
				StartOffset: found.StartOffset,
			})
		}
	}
	return comments, blankLines
}
