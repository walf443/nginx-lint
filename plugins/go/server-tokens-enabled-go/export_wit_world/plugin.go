// Package export_wit_world implements the `plugin` world's exports. The
// package name and the two function signatures are fixed by the bindings
// componentize-go generates; this package is the adapter between them and
// the rule itself, which lives in ../rule so it can be unit-tested.
package export_wit_world

import (
	"strings"

	"wit_component/examples"
	"wit_component/nginx_lint_plugin_config_api"
	"wit_component/nginx_lint_plugin_types"
	"wit_component/rule"

	witTypes "go.bytecodealliance.org/pkg/wit/types"
)

const (
	name     = "server-tokens-enabled-go"
	category = "security"
)

// Spec returns the rule's metadata, which `nginx-lint why` renders.
func Spec() nginx_lint_plugin_types.PluginSpec {
	return nginx_lint_plugin_types.PluginSpec{
		Name:        name,
		Category:    category,
		Description: "Detects when server_tokens is enabled (exposes nginx version)",
		ApiVersion:  "1.2",
		Severity:    witTypes.Some("warning"),
		Why: witTypes.Some(
			"Server response headers reveal the exact nginx version, which " +
				"tells an attacker which published vulnerabilities to try.",
		),
		BadExample:  witTypes.Some(strings.TrimSpace(examples.Bad)),
		GoodExample: witTypes.Some(strings.TrimSpace(examples.Good)),
	}
}

// Check hands the configuration's directives to the rule and translates what
// comes back into the host's error type.
func Check(cfg *nginx_lint_plugin_config_api.Config, path string) []nginx_lint_plugin_types.LintError {
	// The config arrives as a `borrow<config>`, and the canonical ABI expects
	// the guest to release it before the call returns. The generated export
	// glue does not, so without this the host rejects the call with "borrow
	// handles still remain at the end of the call" — even from a Check that
	// never touches cfg.
	defer cfg.Drop()

	contexts := cfg.AllDirectivesWithContext()
	directives := make([]rule.Directive, 0, len(contexts))
	for _, ctx := range contexts {
		// Each directive handle is owned by this call and has to be released
		// before it returns. The bindings only drop them from a GC finalizer,
		// which does not run inside a check this short.
		defer ctx.Directive.Drop()
		directives = append(directives, ctx.Directive)
	}

	errs := []nginx_lint_plugin_types.LintError{}
	for _, finding := range rule.Check(directives) {
		errs = append(errs, nginx_lint_plugin_types.LintError{
			Rule:     name,
			Category: category,
			Message:  rule.Message,
			Severity: nginx_lint_plugin_types.SeverityWarning,
			Line:     witTypes.Some(finding.Line),
			Column:   witTypes.Some(finding.Column),
		})
	}
	return errs
}
