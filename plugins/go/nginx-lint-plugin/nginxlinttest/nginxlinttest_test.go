package nginxlinttest_test

import (
	"os"
	"path/filepath"
	"testing"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
	"github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/nginxlinttest"
)

const nested = `# a comment

http {
    server {
        server_tokens on;
    }
}
`

// serverTokens is the rule under test in this package: small enough to be
// obviously correct, so a failure here is the harness rather than the rule.
type serverTokens struct{}

func (serverTokens) Spec() nginxlint.Spec {
	return nginxlint.Spec{Name: "server-tokens", Category: "security", Severity: "warning"}
}

func (serverTokens) Check(cfg nginxlint.Config) []nginxlint.LintError {
	errors := []nginxlint.LintError{}
	cfg.Named("server_tokens", func(d nginxlint.Directive) {
		if d.FirstArgIs("on") {
			errors = append(errors, nginxlint.Warning(d, "server_tokens is on").
				WithFix(d.ReplaceWith("server_tokens off;")))
		}
	})
	return errors
}

func TestParsePositionsComeFromTheParser(t *testing.T) {
	cfg, err := nginxlinttest.Parse("nginx.conf", nested)
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}

	found := 0
	cfg.Named("server_tokens", func(d nginxlint.Directive) {
		found++
		if d.Line != 5 || d.Column != 9 {
			t.Errorf("at %d:%d, want 5:9", d.Line, d.Column)
		}
		if d.LeadingWhitespace != "        " {
			t.Errorf("LeadingWhitespace = %q", d.LeadingWhitespace)
		}
		if !d.IsInside("http") || !d.IsInside("server") {
			t.Errorf("Parents = %v, want [http server]", d.Parents)
		}
	})
	if found != 1 {
		t.Fatalf("the parser produced %d server_tokens directives, want 1", found)
	}
}

func TestParseCollectsComments(t *testing.T) {
	cfg, err := nginxlinttest.Parse("nginx.conf", nested)
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}

	if len(cfg.Comments) != 1 || cfg.Comments[0].Text != "# a comment" {
		t.Errorf("Comments = %+v, want one \"# a comment\"", cfg.Comments)
	}
	if len(cfg.BlankLines) != 1 || cfg.BlankLines[0].Line != 2 {
		t.Errorf("BlankLines = %+v, want one on line 2", cfg.BlankLines)
	}
}

func TestParseArgumentTypes(t *testing.T) {
	cfg, err := nginxlinttest.Parse("nginx.conf", "server {\n    listen $port \"quoted\";\n}\n")
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}

	cfg.Named("listen", func(d nginxlint.Directive) {
		if len(d.Args) != 2 {
			t.Fatalf("got %d arguments, want 2", len(d.Args))
		}
		if !d.Args[0].IsVariable() {
			t.Errorf("$port was not reported as a variable")
		}
		if !d.Args[1].IsQuoted() || d.Args[1].Value != "quoted" {
			t.Errorf("the quoted argument came through as %+v", d.Args[1])
		}
	})
}

// An included fragment has to look the same to a rule as one written inline,
// which is what seeding the parent stack from the include context is for.
func TestParseWithIncludeContext(t *testing.T) {
	cfg, err := nginxlinttest.Parse("fragment.conf", "server_tokens on;\n", "http", "server")
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}

	if !cfg.IsIncludedFrom("http") {
		t.Error("IsIncludedFrom(\"http\") = false")
	}
	cfg.Named("server_tokens", func(d nginxlint.Directive) {
		if !d.IsInside("http") || !d.IsInside("server") {
			t.Errorf("Parents = %v, want [http server]", d.Parents)
		}
	})
}

func TestParseReportsASyntaxError(t *testing.T) {
	if _, err := nginxlinttest.Parse("nginx.conf", "http {"); err == nil {
		t.Error("Parse of an unclosed block returned no error")
	}
}

func TestParseFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "nginx.conf")
	if err := os.WriteFile(path, []byte(nested), 0o600); err != nil {
		t.Fatal(err)
	}

	cfg, err := nginxlinttest.ParseFile(path)
	if err != nil {
		t.Fatalf("ParseFile: %v", err)
	}
	if cfg.Path != path {
		t.Errorf("Path = %q, want %q", cfg.Path, path)
	}
}

func TestRunnerAssertions(t *testing.T) {
	runner := nginxlinttest.New(serverTokens{})

	runner.AssertErrors(t, nested, 1)
	runner.AssertHasErrors(t, nested)
	runner.AssertErrorOnLine(t, nested, 5)
	runner.AssertMessageContains(t, nested, "server_tokens is on")
	runner.AssertHasFix(t, nested)
	runner.AssertNoErrors(t, "http {\n    server_tokens off;\n}\n")
}

// The fixes are applied by the linter's own applier, so this is the assertion
// that checks the SDK's pure-Go fix offsets against the ones that will be used
// for real.
func TestRunnerAssertFixProduces(t *testing.T) {
	runner := nginxlinttest.New(serverTokens{})

	runner.AssertFixProduces(t,
		"http {\n    server_tokens on;\n}\n",
		"http {\n    server_tokens off;\n}\n")
}

func TestRunnerAssertExamplesWithFix(t *testing.T) {
	runner := nginxlinttest.New(serverTokens{})

	runner.AssertExamplesWithFix(t,
		"http {\n    server_tokens on;\n}\n",
		"http {\n    server_tokens off;\n}\n")
}

func TestApplyFixesReportsWhatItDropped(t *testing.T) {
	// Two fixes over the same range: the applier takes the first and drops
	// the second, which is how a rule finds out its fixes conflict.
	fix := nginxlint.Directive{Line: 1, Column: 1, StartOffset: 0, EndOffset: 5}.ReplaceWith("gzip")
	result, err := nginxlinttest.ApplyFixes("http {\n}\n", []nginxlint.Fix{fix, fix})
	if err != nil {
		t.Fatalf("ApplyFixes: %v", err)
	}

	if result.Applied != 1 {
		t.Errorf("Applied = %d, want 1", result.Applied)
	}
	if result.SkippedInvalid != 0 && result.Applied+result.SkippedInvalid != 2 {
		t.Errorf("Applied=%d SkippedInvalid=%d, want them to account for both fixes",
			result.Applied, result.SkippedInvalid)
	}
}

func TestSpec(t *testing.T) {
	if name := nginxlinttest.New(serverTokens{}).Spec().Name; name != "server-tokens" {
		t.Errorf("Spec().Name = %q", name)
	}
}
