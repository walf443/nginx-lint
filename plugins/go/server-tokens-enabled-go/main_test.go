package main

import (
	"testing"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
)

// serverTokens is what the SDK hands the rule for `server_tokens <value>;`
// nested in an http block, at line 2 column 5.
func serverTokens(value string) nginxlint.Directive {
	return nginxlint.Directive{
		Name:              "server_tokens",
		Args:              []nginxlint.Argument{{Value: value}},
		Line:              2,
		Column:            5,
		StartOffset:       11,
		EndOffset:         28,
		LeadingWhitespace: "    ",
		Parents:           []string{"http"},
	}
}

func inHTTP(directives ...nginxlint.Directive) nginxlint.Config {
	return nginxlint.Config{
		Path:       "nginx.conf",
		Directives: []nginxlint.Directive{{Name: "http", Block: directives}},
	}
}

func TestReportsServerTokensOn(t *testing.T) {
	errors := serverTokensEnabled{}.Check(inHTTP(serverTokens("on")))

	if len(errors) != 1 {
		t.Fatalf("got %d findings, want 1", len(errors))
	}
	if errors[0].Message != message {
		t.Errorf("Message = %q, want %q", errors[0].Message, message)
	}
	if errors[0].Line != 2 || errors[0].Column != 5 {
		t.Errorf("reported at %d:%d, want 2:5", errors[0].Line, errors[0].Column)
	}
	if errors[0].Severity != nginxlint.SeverityWarning {
		t.Errorf("Severity = %d, want %d", errors[0].Severity, nginxlint.SeverityWarning)
	}
}

func TestFixTurnsItOff(t *testing.T) {
	errors := serverTokensEnabled{}.Check(inHTTP(serverTokens("on")))

	if len(errors) != 1 || len(errors[0].Fixes) != 1 {
		t.Fatalf("got %d findings, want 1 carrying a fix", len(errors))
	}
	fix := errors[0].Fixes[0]
	if fix.NewText != "    server_tokens off;" {
		t.Errorf("NewText = %q, want %q", fix.NewText, "    server_tokens off;")
	}
	if fix.StartOffset != 7 || fix.EndOffset != 28 {
		t.Errorf("range = %d..%d, want 7..28", fix.StartOffset, fix.EndOffset)
	}
}

func TestIgnoresServerTokensOff(t *testing.T) {
	if errors := (serverTokensEnabled{}).Check(inHTTP(serverTokens("off"))); len(errors) != 0 {
		t.Fatalf("got %d findings, want none", len(errors))
	}
}

// `on` is a common argument, so the rule has to match on the directive name
// and not just on the value.
func TestIgnoresOtherDirectivesTurnedOn(t *testing.T) {
	autoindex := nginxlint.Directive{
		Name: "autoindex",
		Args: []nginxlint.Argument{{Value: "on"}},
	}

	if errors := (serverTokensEnabled{}).Check(inHTTP(autoindex)); len(errors) != 0 {
		t.Fatalf("got %d findings, want none", len(errors))
	}
}

func TestSpecCarriesTheExamples(t *testing.T) {
	spec := serverTokensEnabled{}.Spec()

	if spec.Name != "server-tokens-enabled-go" || spec.Category != "security" {
		t.Errorf("Spec() = %q/%q, want security/server-tokens-enabled-go", spec.Category, spec.Name)
	}
	if spec.BadExample == "" || spec.GoodExample == "" {
		t.Error("Spec() left an example empty; go:embed did not run")
	}
}
