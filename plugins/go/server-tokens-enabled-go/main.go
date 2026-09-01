// Command server-tokens-enabled-go is the Go counterpart of the builtin
// server_tokens_enabled rule. It reports every `server_tokens on`, which
// leaks the exact nginx version in response headers.
package main

import (
	_ "embed"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
	// Links the component's exports. Without it the component builds with no
	// exports and the host rejects it; on the host it is empty, which is what
	// keeps this package testable with a plain `go test`.
	_ "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/export"
)

//go:embed examples/bad.conf
var badExample string

//go:embed examples/good.conf
var goodExample string

const message = "server_tokens is on; the nginx version is exposed"

type serverTokensEnabled struct{}

func (serverTokensEnabled) Spec() nginxlint.Spec {
	return nginxlint.Spec{
		Name:        "server-tokens-enabled-go",
		Category:    "security",
		Description: "Detects when server_tokens is enabled (exposes nginx version)",
		Severity:    "warning",
		Why: "Server response headers reveal the exact nginx version, which " +
			"tells an attacker which published vulnerabilities to try.",
		BadExample:  badExample,
		GoodExample: goodExample,
	}
}

func (serverTokensEnabled) Check(cfg nginxlint.Config) []nginxlint.LintError {
	errors := []nginxlint.LintError{}
	cfg.Named("server_tokens", func(directive nginxlint.Directive) {
		if directive.FirstArgIs("on") {
			errors = append(errors, nginxlint.Warning(directive, message).
				WithFix(directive.ReplaceWith("server_tokens off;")))
		}
	})
	return errors
}

// The host calls the component's exports directly, so registration happens
// here rather than in main, which never runs.
func init() { nginxlint.Register(serverTokensEnabled{}) }

func main() {}
