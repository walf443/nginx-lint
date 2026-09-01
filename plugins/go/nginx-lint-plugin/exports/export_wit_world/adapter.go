// Package export_wit_world implements the `plugin` world's exports. Its
// package name and the two function signatures are fixed by the bindings
// componentize-go generates; it forwards to the plugin registered with
// [nginxlint.Register] and is the only place in the SDK that speaks WIT.
//
// A plugin does not import this package. It blank imports
// .../nginx-lint-plugin/export, which links it in only on wasm.
package export_wit_world

import (
	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
	configapi "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/bindings/nginx_lint_plugin_config_api"
	datatypes "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/bindings/nginx_lint_plugin_data_types"
	parsertypes "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/bindings/nginx_lint_plugin_parser_types"
	plugintypes "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/bindings/nginx_lint_plugin_types"
	witTypes "go.bytecodealliance.org/pkg/wit/types"
)

// registered fails loudly when a plugin forgot to call
// [nginxlint.Register], which the old hand-written export layout could not
// get wrong. A nil interface would otherwise panic somewhere inside the
// conversion, and reach the host as an opaque trap; this at least names the
// cause, and does it from spec, so the plugin fails to load rather than on
// the first configuration it is handed.
func registered() nginxlint.Plugin {
	plugin := nginxlint.Registered()
	if plugin == nil {
		panic("nginx-lint-plugin: no plugin registered; call nginxlint.Register from an init function")
	}
	return plugin
}

func optional(value string) witTypes.Option[string] {
	if value == "" {
		return witTypes.None[string]()
	}
	return witTypes.Some(value)
}

func text(value witTypes.Option[string]) string {
	if value.IsNone() {
		return ""
	}
	return value.Some()
}

// Spec is the world's `spec` export.
func Spec() plugintypes.PluginSpec {
	spec := registered().Spec()
	converted := plugintypes.PluginSpec{
		Name:            spec.Name,
		Category:        spec.Category,
		Description:     spec.Description,
		ApiVersion:      nginxlint.APIVersion,
		Severity:        optional(spec.Severity),
		Why:             optional(spec.Why),
		BadExample:      optional(spec.BadExample),
		GoodExample:     optional(spec.GoodExample),
		MinNginxVersion: optional(spec.MinNginxVersion),
		MaxNginxVersion: optional(spec.MaxNginxVersion),
	}
	if len(spec.References) > 0 {
		converted.References = witTypes.Some(spec.References)
	}
	return converted
}

// Check is the world's `check` export.
func Check(cfg *configapi.Config, path string) []plugintypes.LintError {
	// The config arrives as a `borrow<config>`, and the canonical ABI expects
	// the guest to release it before the call returns. The generated export
	// glue does not, so without this the host rejects the call with "borrow
	// handles still remain at the end of the call" — even from a check that
	// never touches cfg. Taking one snapshot is also why no directive handle
	// is ever created, so this is the only drop there is.
	defer cfg.Drop()
	snapshot := cfg.Snapshot()

	config := nginxlint.Config{
		Path:           path,
		IncludeContext: snapshot.IncludeContext,
		Directives: directives(
			snapshot.AllItems,
			snapshot.TopLevelIndices,
			// An included fragment is nested inside the blocks it was
			// included from, so the include context seeds the parent stack.
			// Without it a rule asking IsInside("http") would be wrong for
			// every file pulled in by an `include`.
			snapshot.IncludeContext,
		),
	}
	config.Comments, config.BlankLines = trivia(snapshot.AllItems)

	plugin := registered()
	spec := plugin.Spec()

	errors := []plugintypes.LintError{}
	for _, found := range plugin.Check(config) {
		errors = append(errors, plugintypes.LintError{
			Rule:     spec.Name,
			Category: spec.Category,
			Message:  found.Message,
			Severity: plugintypes.Severity(found.Severity),
			Line:     witTypes.Some(found.Line),
			Column:   witTypes.Some(found.Column),
			Fixes:    fixes(found.Fixes),
		})
	}
	return errors
}

// directives rebuilds the nested shape from the flat snapshot, which reports
// a block's children as indices into one array.
func directives(all []parsertypes.ConfigItem, indices []uint32, parents []string) []nginxlint.Directive {
	converted := []nginxlint.Directive{}
	for _, index := range indices {
		if int(index) >= len(all) {
			continue
		}
		item := all[index]
		if item.Value.Tag() != parsertypes.ConfigItemValueDirectiveItem {
			continue
		}
		directive := directive(item.Value.DirectiveItem(), parents)
		if len(item.ChildIndices) > 0 {
			// Copy rather than append in place: several siblings share this
			// parents slice, and appending to it would let one of them
			// overwrite another's stack.
			nested := append(append([]string{}, parents...), directive.Name)
			directive.Block = directives(all, item.ChildIndices, nested)
		}
		converted = append(converted, directive)
	}
	return converted
}

func directive(data datatypes.DirectiveData, parents []string) nginxlint.Directive {
	args := make([]nginxlint.Argument, 0, len(data.Args))
	for _, arg := range data.Args {
		args = append(args, nginxlint.Argument{
			Value:       arg.Value,
			Raw:         arg.Raw,
			Type:        nginxlint.ArgumentType(arg.ArgType),
			Line:        arg.Line,
			Column:      arg.Column,
			StartOffset: arg.StartOffset,
			EndOffset:   arg.EndOffset,
		})
	}
	return nginxlint.Directive{
		Name:                  data.Name,
		Args:                  args,
		Line:                  data.Line,
		Column:                data.Column,
		StartOffset:           data.StartOffset,
		EndOffset:             data.EndOffset,
		EndLine:               data.EndLine,
		EndColumn:             data.EndColumn,
		LeadingWhitespace:     data.LeadingWhitespace,
		TrailingWhitespace:    data.TrailingWhitespace,
		SpaceBeforeTerminator: data.SpaceBeforeTerminator,
		HasBlock:              data.HasBlock,
		BlockIsRaw:            data.BlockIsRaw,
		BlockRawContent:       text(data.BlockRawContent),
		TrailingComment:       text(data.TrailingCommentText),
		Parents:               parents,
	}
}

// trivia collects the comments and blank lines from the whole file. They are
// flat rather than woven into the directive tree, and the snapshot's array is
// already in source order, so one pass over it is enough.
func trivia(all []parsertypes.ConfigItem) ([]nginxlint.Comment, []nginxlint.BlankLine) {
	comments := []nginxlint.Comment{}
	blankLines := []nginxlint.BlankLine{}
	for _, item := range all {
		switch item.Value.Tag() {
		case parsertypes.ConfigItemValueCommentItem:
			found := item.Value.CommentItem()
			comments = append(comments, nginxlint.Comment{
				Text:               found.Text,
				Line:               found.Line,
				Column:             found.Column,
				LeadingWhitespace:  found.LeadingWhitespace,
				TrailingWhitespace: found.TrailingWhitespace,
				StartOffset:        found.StartOffset,
				EndOffset:          found.EndOffset,
			})
		case parsertypes.ConfigItemValueBlankLineItem:
			found := item.Value.BlankLineItem()
			blankLines = append(blankLines, nginxlint.BlankLine{
				Line:        found.Line,
				Content:     found.Content,
				StartOffset: found.StartOffset,
			})
		}
	}
	return comments, blankLines
}

func fixes(from []nginxlint.Fix) []plugintypes.Fix {
	converted := make([]plugintypes.Fix, 0, len(from))
	for _, fix := range from {
		out := plugintypes.Fix{
			Line:        fix.Line,
			OldText:     optional(fix.OldText),
			NewText:     fix.NewText,
			DeleteLine:  fix.DeleteLine,
			InsertAfter: fix.InsertAfter,
		}
		if fix.HasRange {
			out.StartOffset = witTypes.Some(fix.StartOffset)
			out.EndOffset = witTypes.Some(fix.EndOffset)
		}
		converted = append(converted, out)
	}
	return converted
}
