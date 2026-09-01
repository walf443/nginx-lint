package nginxlint_test

import (
	"reflect"
	"testing"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
)

// config is the shape the SDK hands a rule for
//
//	http {
//	    server {
//	        server_tokens on;
//	    }
//	    gzip on;
//	}
func config() nginxlint.Config {
	return nginxlint.Config{
		Path: "nginx.conf",
		Directives: []nginxlint.Directive{{
			Name: "http",
			Block: []nginxlint.Directive{
				{
					Name:    "server",
					Parents: []string{"http"},
					Block: []nginxlint.Directive{{
						Name:    "server_tokens",
						Args:    []nginxlint.Argument{{Value: "on"}},
						Parents: []string{"http", "server"},
					}},
				},
				{
					Name:    "gzip",
					Args:    []nginxlint.Argument{{Value: "on"}},
					Parents: []string{"http"},
				},
			},
		}},
	}
}

func TestAllVisitsEveryDepthInSourceOrder(t *testing.T) {
	names := []string{}
	config().All(func(d nginxlint.Directive) { names = append(names, d.Name) })

	want := []string{"http", "server", "server_tokens", "gzip"}
	if !reflect.DeepEqual(names, want) {
		t.Errorf("All() = %v, want %v", names, want)
	}
}

func TestNamedVisitsOnlyMatchingDirectives(t *testing.T) {
	found := 0
	config().Named("server_tokens", func(nginxlint.Directive) { found++ })

	if found != 1 {
		t.Errorf("Named() visited %d directives, want 1", found)
	}
}

func TestIsInsideWalksTheWholeParentStack(t *testing.T) {
	directive := nginxlint.Directive{Parents: []string{"http", "server", "location"}}

	for _, block := range []string{"http", "server", "location"} {
		if !directive.IsInside(block) {
			t.Errorf("IsInside(%q) = false, want true", block)
		}
	}
	if directive.IsInside("stream") {
		t.Error("IsInside(\"stream\") = true, want false")
	}
}

func TestParentIsTheInnermostBlock(t *testing.T) {
	directive := nginxlint.Directive{Parents: []string{"http", "server"}}

	parent, ok := directive.Parent()
	if !ok || parent != "server" {
		t.Errorf("Parent() = (%q, %v), want (\"server\", true)", parent, ok)
	}

	if _, ok := (nginxlint.Directive{}).Parent(); ok {
		t.Error("Parent() of a top-level directive reported a parent")
	}
}

func TestArgumentAccessors(t *testing.T) {
	directive := nginxlint.Directive{
		Name: "listen",
		Args: []nginxlint.Argument{{Value: "443"}, {Value: "ssl"}, {Value: "http2"}},
	}

	if !directive.Is("listen") || directive.Is("server") {
		t.Error("Is() did not match on the name alone")
	}
	if arg, ok := directive.FirstArg(); !ok || arg != "443" {
		t.Errorf("FirstArg() = (%q, %v), want (\"443\", true)", arg, ok)
	}
	if !directive.FirstArgIs("443") || directive.FirstArgIs("ssl") {
		t.Error("FirstArgIs() matched an argument other than the first")
	}
	if arg, ok := directive.LastArg(); !ok || arg != "http2" {
		t.Errorf("LastArg() = (%q, %v), want (\"http2\", true)", arg, ok)
	}
	if arg, ok := directive.ArgAt(1); !ok || arg != "ssl" {
		t.Errorf("ArgAt(1) = (%q, %v), want (\"ssl\", true)", arg, ok)
	}
	if _, ok := directive.ArgAt(3); ok {
		t.Error("ArgAt() past the end reported an argument")
	}
	if !directive.HasArg("ssl") || directive.HasArg("quic") {
		t.Error("HasArg() did not match on value")
	}
	if directive.ArgCount() != 3 {
		t.Errorf("ArgCount() = %d, want 3", directive.ArgCount())
	}
	if want := []string{"443", "ssl", "http2"}; !reflect.DeepEqual(directive.ArgValues(), want) {
		t.Errorf("ArgValues() = %v, want %v", directive.ArgValues(), want)
	}
}

func TestAccessorsOnADirectiveWithoutArguments(t *testing.T) {
	directive := nginxlint.Directive{Name: "gzip"}

	if _, ok := directive.FirstArg(); ok {
		t.Error("FirstArg() reported an argument")
	}
	if _, ok := directive.LastArg(); ok {
		t.Error("LastArg() reported an argument")
	}
	if directive.FirstArgIs("on") {
		t.Error("FirstArgIs() matched with no arguments present")
	}
}

func TestArgumentType(t *testing.T) {
	variable := nginxlint.Argument{Value: "host", Type: nginxlint.ArgumentVariable}
	quoted := nginxlint.Argument{Value: "a b", Type: nginxlint.ArgumentQuotedString}
	literal := nginxlint.Argument{Value: "on"}

	if !variable.IsVariable() || variable.IsLiteral() || variable.IsQuoted() {
		t.Error("a variable argument was not reported as one")
	}
	if !quoted.IsQuoted() || quoted.IsLiteral() {
		t.Error("a quoted argument was not reported as one")
	}
	if !literal.IsLiteral() || literal.IsQuoted() {
		t.Error("a literal argument was not reported as one")
	}
}

func TestIsIncludedFrom(t *testing.T) {
	cfg := nginxlint.Config{IncludeContext: []string{"http", "server"}}

	if !cfg.IsIncludedFrom("http") || !cfg.IsIncludedFrom("server") {
		t.Error("IsIncludedFrom() did not match the include context")
	}
	if cfg.IsIncludedFrom("stream") {
		t.Error("IsIncludedFrom(\"stream\") = true, want false")
	}
}

func TestFindingConstructors(t *testing.T) {
	directive := nginxlint.Directive{Name: "server_tokens", Line: 3, Column: 5}

	warning := nginxlint.Warning(directive, "exposes the version")
	if warning.Severity != nginxlint.SeverityWarning {
		t.Errorf("Warning() severity = %d, want %d", warning.Severity, nginxlint.SeverityWarning)
	}
	if warning.Line != 3 || warning.Column != 5 {
		t.Errorf("Warning() at %d:%d, want 3:5", warning.Line, warning.Column)
	}

	if severity := nginxlint.Error(directive, "broken").Severity; severity != nginxlint.SeverityError {
		t.Errorf("Error() severity = %d, want %d", severity, nginxlint.SeverityError)
	}

	moved := warning.At(9, 1)
	if moved.Line != 9 || moved.Column != 1 {
		t.Errorf("At() = %d:%d, want 9:1", moved.Line, moved.Column)
	}
	if warning.Line != 3 {
		t.Error("At() modified the finding it was called on")
	}

	fixed := warning.WithFix(directive.DeleteLine(), directive.InsertAfter("gzip on;"))
	if len(fixed.Fixes) != 2 {
		t.Errorf("WithFix() left %d fixes, want 2", len(fixed.Fixes))
	}
	if len(warning.Fixes) != 0 {
		t.Error("WithFix() modified the finding it was called on")
	}
}

type stubPlugin struct{}

func (stubPlugin) Spec() nginxlint.Spec                         { return nginxlint.Spec{Name: "stub"} }
func (stubPlugin) Check(nginxlint.Config) []nginxlint.LintError { return nil }

func TestRegister(t *testing.T) {
	nginxlint.Register(stubPlugin{})
	if nginxlint.Registered() == nil {
		t.Fatal("Registered() returned nil after Register()")
	}
	if name := nginxlint.Registered().Spec().Name; name != "stub" {
		t.Errorf("Registered().Spec().Name = %q, want \"stub\"", name)
	}
}
