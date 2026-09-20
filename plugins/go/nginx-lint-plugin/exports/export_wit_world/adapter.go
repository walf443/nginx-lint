// Package export_wit_world implements the `plugin-rules` world's exports.
// Its package name and the two function signatures are fixed by the bindings
// componentize-go generates; it forwards to the rules registered with
// [nginxlint.Register] and is the only place in the SDK that speaks WIT.
//
// A plugin does not import this package. It blank imports
// .../nginx-lint-plugin/export, which links it in only on wasm.
package export_wit_world

import (
	"slices"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
	configapi "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/bindings/nginx_lint_plugin_config_api"
	datatypes "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/bindings/nginx_lint_plugin_data_types"
	parsertypes "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/bindings/nginx_lint_plugin_parser_types"
	plugintypes "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/bindings/nginx_lint_plugin_types"
	"github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/internal/snapshot"
	witTypes "go.bytecodealliance.org/pkg/wit/types"
)

// registered fails loudly when a plugin forgot to call
// [nginxlint.Register], which the old hand-written export layout could not
// get wrong. An empty list would otherwise reach the host as a component
// with no rules, which it refuses with a message that does not name the
// cause; this does, and from specs, so the plugin fails to load rather than
// on the first configuration it is handed.
func registered() []nginxlint.Rule {
	rules := nginxlint.Registered()
	if len(rules) == 0 {
		panic("nginx-lint-plugin: no rule registered; call nginxlint.Register from an init function")
	}
	return rules
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

// Specs is the world's `specs` export: every registered rule's metadata, in
// registration order.
func Specs() []plugintypes.PluginSpec {
	rules := registered()
	specs := make([]plugintypes.PluginSpec, 0, len(rules))
	for _, rule := range rules {
		specs = append(specs, convertSpec(rule.Spec()))
	}
	return specs
}

func convertSpec(spec nginxlint.Spec) plugintypes.PluginSpec {
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

// Check is the world's `check` export: the findings of the registered rules
// named in asked, over one snapshot of the config. Names the component does
// not carry are ignored, and an empty list yields no findings.
func Check(cfg *configapi.Config, path string, asked []string) []plugintypes.LintError {
	// The config arrives as a `borrow<config>`, and the canonical ABI expects
	// the guest to release it before the call returns. The generated export
	// glue does not, so without this the host rejects the call with "borrow
	// handles still remain at the end of the call" — even from a check that
	// never touches cfg. Taking one snapshot is also why no directive handle
	// is ever created, so this is the only drop there is.
	defer cfg.Drop()

	var rules []nginxlint.Rule
	for _, rule := range registered() {
		if slices.Contains(asked, rule.Spec().Name) {
			rules = append(rules, rule)
		}
	}
	errors := []plugintypes.LintError{}
	if len(rules) == 0 {
		return errors
	}
	// One snapshot for every asked rule. The SDK's Config is plain data the
	// rules only read, so they share it.
	config := parseOutput(cfg.Snapshot()).Config(path)

	for _, rule := range rules {
		spec := rule.Spec()
		for _, found := range rule.Check(config) {
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
	}
	return errors
}

// parseOutput restates the host's snapshot in the shape the SDK converts from,
// which is the same one the parser's JSON entry point emits. The copy is
// mechanical on purpose: everything with a decision in it — the nesting, the
// parent stacks, the include context — lives in one place, so a rule tested
// against the real parser sees what it will see at run time.
func parseOutput(from configapi.ConfigSnapshot) snapshot.ParseOutput {
	items := make([]snapshot.Item, 0, len(from.AllItems))
	for _, item := range from.AllItems {
		items = append(items, snapshot.Item{
			Value:        itemValue(item.Value),
			ChildIndices: item.ChildIndices,
		})
	}
	return snapshot.ParseOutput{
		Items:           items,
		TopLevelIndices: from.TopLevelIndices,
		IncludeContext:  from.IncludeContext,
	}
}

func itemValue(from parsertypes.ConfigItemValue) snapshot.ItemValue {
	switch from.Tag() {
	case parsertypes.ConfigItemValueDirectiveItem:
		return snapshot.ItemValue{Directive: directive(from.DirectiveItem())}
	case parsertypes.ConfigItemValueCommentItem:
		found := from.CommentItem()
		return snapshot.ItemValue{Comment: &snapshot.Comment{
			Text:               found.Text,
			Line:               found.Line,
			Column:             found.Column,
			LeadingWhitespace:  found.LeadingWhitespace,
			TrailingWhitespace: found.TrailingWhitespace,
			StartOffset:        found.StartOffset,
			EndOffset:          found.EndOffset,
		}}
	case parsertypes.ConfigItemValueBlankLineItem:
		found := from.BlankLineItem()
		return snapshot.ItemValue{BlankLine: &snapshot.BlankLine{
			Line:        found.Line,
			Content:     found.Content,
			StartOffset: found.StartOffset,
		}}
	}
	return snapshot.ItemValue{}
}

func directive(from datatypes.DirectiveData) *snapshot.Directive {
	args := make([]snapshot.Argument, 0, len(from.Args))
	for _, arg := range from.Args {
		args = append(args, snapshot.Argument{
			Value:       arg.Value,
			Raw:         arg.Raw,
			Type:        snapshot.ArgumentType(arg.ArgType),
			Line:        arg.Line,
			Column:      arg.Column,
			StartOffset: arg.StartOffset,
			EndOffset:   arg.EndOffset,
		})
	}
	return &snapshot.Directive{
		Name:                  from.Name,
		Args:                  args,
		Line:                  from.Line,
		Column:                from.Column,
		StartOffset:           from.StartOffset,
		EndOffset:             from.EndOffset,
		EndLine:               from.EndLine,
		EndColumn:             from.EndColumn,
		LeadingWhitespace:     from.LeadingWhitespace,
		TrailingWhitespace:    from.TrailingWhitespace,
		SpaceBeforeTerminator: from.SpaceBeforeTerminator,
		HasBlock:              from.HasBlock,
		BlockIsRaw:            from.BlockIsRaw,
		BlockRawContent:       text(from.BlockRawContent),
		TrailingComment:       text(from.TrailingCommentText),
	}
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
