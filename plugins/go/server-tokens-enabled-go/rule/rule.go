// Package rule holds what the plugin actually decides, kept clear of the
// generated bindings so it can be tested with a plain `go test`: those
// bindings only build for wasm, and importing them anywhere in a package
// makes that package untestable on the host.
package rule

// Directive is the part of the WIT `directive` resource this rule uses. The
// generated `*nginx_lint_plugin_config_api.Directive` satisfies it as it
// stands, so the adapter passes those straight through and tests pass fakes.
type Directive interface {
	Is(name string) bool
	FirstArgIs(value string) bool
	Line() uint32
	Column() uint32
}

// Finding is one reported occurrence, in the terms this package knows about:
// where it is, not how the host wants it spelled.
type Finding struct {
	Line   uint32
	Column uint32
}

// Message is what every finding reports.
const Message = "server_tokens is on; the nginx version is exposed"

// Check reports every `server_tokens on`, which leaks the exact nginx
// version in response headers.
func Check(directives []Directive) []Finding {
	findings := []Finding{}
	for _, directive := range directives {
		if directive.Is("server_tokens") && directive.FirstArgIs("on") {
			findings = append(findings, Finding{
				Line:   directive.Line(),
				Column: directive.Column(),
			})
		}
	}
	return findings
}
