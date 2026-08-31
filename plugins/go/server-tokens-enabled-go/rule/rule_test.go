package rule

import "testing"

// fakeDirective stands in for the host resource. The real one answers the
// same four questions, so a rule written against the interface is exercised
// here exactly as it runs in the component.
type fakeDirective struct {
	name   string
	arg    string
	line   uint32
	column uint32
}

func (d fakeDirective) Is(name string) bool          { return d.name == name }
func (d fakeDirective) FirstArgIs(value string) bool { return d.arg == value }
func (d fakeDirective) Line() uint32                 { return d.line }
func (d fakeDirective) Column() uint32               { return d.column }

func TestReportsServerTokensOn(t *testing.T) {
	findings := Check([]Directive{
		fakeDirective{name: "listen", arg: "80", line: 2, column: 5},
		fakeDirective{name: "server_tokens", arg: "on", line: 3, column: 5},
	})

	if len(findings) != 1 {
		t.Fatalf("got %d findings, want 1", len(findings))
	}
	if findings[0] != (Finding{Line: 3, Column: 5}) {
		t.Errorf("got %+v, want line 3 column 5", findings[0])
	}
}

func TestAcceptsServerTokensOff(t *testing.T) {
	findings := Check([]Directive{
		fakeDirective{name: "server_tokens", arg: "off", line: 3, column: 5},
	})

	if len(findings) != 0 {
		t.Errorf("got %+v, want none", findings)
	}
}

func TestIgnoresOtherDirectivesWithOn(t *testing.T) {
	// `on` is a common argument; the rule keys on the directive name too
	findings := Check([]Directive{
		fakeDirective{name: "autoindex", arg: "on", line: 4, column: 5},
	})

	if len(findings) != 0 {
		t.Errorf("got %+v, want none", findings)
	}
}
