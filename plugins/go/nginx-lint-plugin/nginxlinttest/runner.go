package nginxlinttest

import (
	"fmt"
	"strings"
	"testing"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
)

// Runner exercises one plugin against the real parser.
//
// The assertions take a [testing.TB] rather than storing one, so a single
// runner can be shared across subtests.
type Runner struct {
	plugin nginxlint.Plugin
}

// New returns a runner for the plugin.
func New(plugin nginxlint.Plugin) *Runner {
	return &Runner{plugin: plugin}
}

// Spec returns the plugin's metadata.
func (r *Runner) Spec() nginxlint.Spec { return r.plugin.Spec() }

// Check parses source and runs the plugin over it.
//
// includeContext names the blocks the file would have been included from, as
// `--context` does on the command line.
func (r *Runner) Check(source string, includeContext ...string) ([]nginxlint.LintError, error) {
	config, err := Parse("nginx.conf", source, includeContext...)
	if err != nil {
		return nil, err
	}
	return r.plugin.Check(config), nil
}

// CheckFile parses a file from disk and runs the plugin over it.
func (r *Runner) CheckFile(path string, includeContext ...string) ([]nginxlint.LintError, error) {
	config, err := ParseFile(path, includeContext...)
	if err != nil {
		return nil, err
	}
	return r.plugin.Check(config), nil
}

// mustCheck fails the test rather than returning a parse error, which is what
// every assertion below wants.
func (r *Runner) mustCheck(t testing.TB, source string) []nginxlint.LintError {
	t.Helper()
	errors, err := r.Check(source)
	if err != nil {
		t.Fatalf("parsing the configuration: %v", err)
	}
	return errors
}

// AssertErrors checks how many findings the plugin reports.
func (r *Runner) AssertErrors(t testing.TB, source string, want int) {
	t.Helper()
	errors := r.mustCheck(t, source)
	if len(errors) != want {
		t.Errorf("got %d findings, want %d:%s", len(errors), want, describe(errors))
	}
}

// AssertNoErrors checks that the plugin reports nothing.
func (r *Runner) AssertNoErrors(t testing.TB, source string) {
	t.Helper()
	r.AssertErrors(t, source, 0)
}

// AssertHasErrors checks that the plugin reports at least one finding.
func (r *Runner) AssertHasErrors(t testing.TB, source string) {
	t.Helper()
	if errors := r.mustCheck(t, source); len(errors) == 0 {
		t.Error("got no findings, want at least one")
	}
}

// AssertErrorOnLine checks that some finding is reported on this line.
func (r *Runner) AssertErrorOnLine(t testing.TB, source string, line uint32) {
	t.Helper()
	errors := r.mustCheck(t, source)
	for _, found := range errors {
		if found.Line == line {
			return
		}
	}
	t.Errorf("no finding on line %d:%s", line, describe(errors))
}

// AssertMessageContains checks that some finding's message contains this text.
func (r *Runner) AssertMessageContains(t testing.TB, source, substring string) {
	t.Helper()
	errors := r.mustCheck(t, source)
	for _, found := range errors {
		if strings.Contains(found.Message, substring) {
			return
		}
	}
	t.Errorf("no finding whose message contains %q:%s", substring, describe(errors))
}

// AssertHasFix checks that some finding carries a fix.
func (r *Runner) AssertHasFix(t testing.TB, source string) {
	t.Helper()
	errors := r.mustCheck(t, source)
	for _, found := range errors {
		if len(found.Fixes) > 0 {
			return
		}
	}
	t.Errorf("no finding carries a fix:%s", describe(errors))
}

// AssertFixProduces applies every fix the plugin reports and compares the
// result with what was expected.
//
// The fixes are applied by the linter's own applier, so this fails for the
// same reasons `--fix` would: an offset that lands in the wrong place, a range
// that splits a character, two fixes that overlap.
func (r *Runner) AssertFixProduces(t testing.TB, source, expected string) {
	t.Helper()
	errors := r.mustCheck(t, source)

	fixes := []nginxlint.Fix{}
	for _, found := range errors {
		fixes = append(fixes, found.Fixes...)
	}
	if len(fixes) == 0 {
		t.Fatalf("the plugin reported no fixes to apply:%s", describe(errors))
	}

	result, err := ApplyFixes(source, fixes)
	if err != nil {
		t.Fatalf("applying the fixes: %v", err)
	}
	// Comparing counts rather than reading SkippedInvalid: a fix dropped for
	// overlapping one already applied is not counted anywhere, so a rule that
	// reports the same edit twice would otherwise slip through whenever the
	// surviving fix happens to produce the expected text.
	if result.Applied != len(fixes) {
		t.Errorf("the applier used %d of the %d fixes reported (%d were invalid, "+
			"the rest overlapped one already applied)",
			result.Applied, len(fixes), result.SkippedInvalid)
	}
	// The applier guarantees a trailing newline, so compare on those terms
	// rather than making every caller remember to add one.
	if got, want := trailingNewline(result.Content), trailingNewline(expected); got != want {
		t.Errorf("applying the fixes produced:\n%s\nwant:\n%s", got, want)
	}
}

// AssertExamples checks the pair of configurations a rule documents: the bad
// one has to be reported and the good one has to be clean. Passing a rule's
// own examples/bad.conf and examples/good.conf keeps `nginx-lint why` honest.
func (r *Runner) AssertExamples(t testing.TB, bad, good string) {
	t.Helper()
	if errors := r.mustCheck(t, bad); len(errors) == 0 {
		t.Error("the bad example was reported clean")
	}
	if errors := r.mustCheck(t, good); len(errors) != 0 {
		t.Errorf("the good example was reported:%s", describe(errors))
	}
}

// AssertExamplesWithFix additionally checks that fixing the bad example
// produces the good one, which is what the CLI does with `--fix`.
func (r *Runner) AssertExamplesWithFix(t testing.TB, bad, good string) {
	t.Helper()
	r.AssertExamples(t, bad, good)
	r.AssertFixProduces(t, bad, good)
}

func trailingNewline(content string) string {
	if strings.HasSuffix(content, "\n") {
		return content
	}
	return content + "\n"
}

// describe renders the findings for a failure message, so a test that fails
// says what the plugin actually reported.
func describe(errors []nginxlint.LintError) string {
	if len(errors) == 0 {
		return " (none reported)"
	}
	var out strings.Builder
	for _, found := range errors {
		fmt.Fprintf(&out, "\n  %d:%d %s", found.Line, found.Column, found.Message)
	}
	return out.String()
}
