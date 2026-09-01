package main

import (
	"testing"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
	"github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/nginxlinttest"
)

func runner() *nginxlinttest.Runner {
	return nginxlinttest.New(serverTokensEnabled{})
}

func TestReportsServerTokensOn(t *testing.T) {
	source := "http {\n    server_tokens on;\n}\n"

	runner().AssertErrors(t, source, 1)
	runner().AssertErrorOnLine(t, source, 2)
	runner().AssertMessageContains(t, source, message)
}

func TestIgnoresServerTokensOff(t *testing.T) {
	runner().AssertNoErrors(t, "http {\n    server_tokens off;\n}\n")
}

// `on` is a common argument, so the rule has to match on the directive name
// and not just on the value.
func TestIgnoresOtherDirectivesTurnedOn(t *testing.T) {
	runner().AssertNoErrors(t, "http {\n    autoindex on;\n}\n")
}

// A fragment included from inside http must look the same to the rule as one
// written inline.
func TestReportsInsideAnIncludedFragment(t *testing.T) {
	errors, err := runner().Check("server_tokens on;\n", "http", "server")
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(errors) != 1 {
		t.Fatalf("got %d findings, want 1", len(errors))
	}
}

// The fixes are applied by the linter's own applier, so this checks the
// offsets the rule produces against the ones `--fix` will use.
func TestFixTurnsItOff(t *testing.T) {
	runner().AssertFixProduces(t,
		"http {\n    server_tokens on;\n}\n",
		"http {\n    server_tokens off;\n}\n")
}

// The examples `nginx-lint why` renders have to be a rule that fires and a
// rule that does not, and --fix has to turn one into the other.
func TestDocumentedExamples(t *testing.T) {
	runner().AssertExamplesWithFix(t, badExample, goodExample)
}

func TestSpec(t *testing.T) {
	spec := runner().Spec()

	if spec.Name != "server-tokens-enabled-go" || spec.Category != "security" {
		t.Errorf("Spec() = %q/%q, want security/server-tokens-enabled-go", spec.Category, spec.Name)
	}
	if spec.BadExample == "" || spec.GoodExample == "" {
		t.Error("Spec() left an example empty; go:embed did not run")
	}
}

// The parser is not the only way to build a configuration: a rule that needs
// a shape the parser cannot easily produce can still be handed struct
// literals, and the same Check runs over them.
func TestAgainstAHandBuiltConfig(t *testing.T) {
	cfg := nginxlint.Config{Path: "nginx.conf", Directives: []nginxlint.Directive{{
		Name: "http",
		Block: []nginxlint.Directive{{
			Name:    "server_tokens",
			Args:    []nginxlint.Argument{{Value: "on"}},
			Line:    2,
			Column:  5,
			Parents: []string{"http"},
		}},
	}}}

	errors := serverTokensEnabled{}.Check(cfg)
	if len(errors) != 1 {
		t.Fatalf("got %d findings, want 1", len(errors))
	}
	if errors[0].Severity != nginxlint.SeverityWarning {
		t.Errorf("Severity = %d, want %d", errors[0].Severity, nginxlint.SeverityWarning)
	}
}
