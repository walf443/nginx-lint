// Package nginxlint is the SDK for writing nginx-lint plugins in Go.
//
// A plugin implements [Plugin], registers it from an init function, and blank
// imports the export package so the component's exports get linked:
//
//	package main
//
//	import (
//		nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
//		_ "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/export"
//	)
//
//	type myRule struct{}
//
//	func (myRule) Spec() nginxlint.Spec { ... }
//	func (myRule) Check(cfg nginxlint.Config) []nginxlint.LintError { ... }
//
//	func init() { nginxlint.Register(myRule{}) }
//	func main()  {}
//
// This package imports none of the generated bindings, and everything a rule
// sees is plain Go data. That is what lets a plugin be tested with a plain
// `go test` on the host: the bindings only build for wasm, so a rule that
// touched them could only be tested inside a component runtime.
package nginxlint

// Severity is how serious a finding is. The host has two levels.
type Severity uint8

const (
	// SeverityError marks a configuration that will not work, or a critical
	// security issue.
	SeverityError Severity = 0
	// SeverityWarning marks a discouraged setting or a potential problem.
	SeverityWarning Severity = 1
)

// ArgumentType is how an argument was written in the source.
type ArgumentType uint8

const (
	ArgumentLiteral ArgumentType = iota
	ArgumentQuotedString
	ArgumentSingleQuotedString
	ArgumentVariable
)

// Argument is one directive argument.
type Argument struct {
	// Value is the argument with any quotes removed.
	Value string
	// Raw is the argument exactly as it appears in the source.
	Raw         string
	Type        ArgumentType
	Line        uint32
	Column      uint32
	StartOffset uint32
	EndOffset   uint32
}

// IsVariable reports whether the argument is a $variable reference.
func (a Argument) IsVariable() bool { return a.Type == ArgumentVariable }

// IsQuoted reports whether the argument was written in quotes.
func (a Argument) IsQuoted() bool {
	return a.Type == ArgumentQuotedString || a.Type == ArgumentSingleQuotedString
}

// IsLiteral reports whether the argument is an unquoted literal.
func (a Argument) IsLiteral() bool { return a.Type == ArgumentLiteral }

// Directive is one nginx directive. It is plain data: the SDK takes a single
// snapshot of the configuration when a check starts, so a rule never holds a
// handle on the host and a test can build one with a struct literal.
type Directive struct {
	Name string
	Args []Argument

	Line        uint32
	Column      uint32
	StartOffset uint32
	EndOffset   uint32
	EndLine     uint32
	EndColumn   uint32

	// LeadingWhitespace is the indentation in front of the directive, and
	// TrailingWhitespace what follows its terminator on the same line.
	LeadingWhitespace  string
	TrailingWhitespace string
	// SpaceBeforeTerminator is what sits between the last argument and the
	// `;` or `{` — usually empty, and the reason a style rule can spot
	// `server_tokens off ;`.
	SpaceBeforeTerminator string

	HasBlock   bool
	BlockIsRaw bool
	// BlockRawContent is the body of a raw block (lua code, for instance),
	// empty when the block is parsed normally or there is no block.
	BlockRawContent string
	// TrailingComment is the text of the comment on the directive's own line,
	// empty when there is none.
	TrailingComment string

	// Parents names the blocks enclosing this directive, outermost first. It
	// starts from the include context, so a rule sees the same nesting
	// whether the directive is written inline or pulled in by `include`.
	Parents []string
	// Block holds the directives nested inside this one.
	Block []Directive
}

// Is reports whether the directive has this name.
func (d Directive) Is(name string) bool { return d.Name == name }

// FirstArg returns the first argument's value, and whether there was one.
func (d Directive) FirstArg() (string, bool) { return d.ArgAt(0) }

// FirstArgIs reports whether the first argument has this value.
func (d Directive) FirstArgIs(value string) bool {
	arg, ok := d.FirstArg()
	return ok && arg == value
}

// LastArg returns the last argument's value, and whether there was one.
func (d Directive) LastArg() (string, bool) {
	if len(d.Args) == 0 {
		return "", false
	}
	return d.Args[len(d.Args)-1].Value, true
}

// ArgAt returns the value of the argument at index, and whether it exists.
func (d Directive) ArgAt(index int) (string, bool) {
	if index < 0 || index >= len(d.Args) {
		return "", false
	}
	return d.Args[index].Value, true
}

// HasArg reports whether any argument has this value.
func (d Directive) HasArg(value string) bool {
	for _, arg := range d.Args {
		if arg.Value == value {
			return true
		}
	}
	return false
}

// ArgCount returns how many arguments the directive has.
func (d Directive) ArgCount() int { return len(d.Args) }

// ArgValues returns every argument's value.
func (d Directive) ArgValues() []string {
	values := make([]string, 0, len(d.Args))
	for _, arg := range d.Args {
		values = append(values, arg.Value)
	}
	return values
}

// IsInside reports whether the directive sits anywhere inside the named block.
func (d Directive) IsInside(block string) bool {
	for _, parent := range d.Parents {
		if parent == block {
			return true
		}
	}
	return false
}

// Parent returns the name of the block immediately containing the directive,
// and whether there is one.
func (d Directive) Parent() (string, bool) {
	if len(d.Parents) == 0 {
		return "", false
	}
	return d.Parents[len(d.Parents)-1], true
}

// Comment is a standalone comment line.
type Comment struct {
	Text               string
	Line               uint32
	Column             uint32
	LeadingWhitespace  string
	TrailingWhitespace string
	StartOffset        uint32
	EndOffset          uint32
}

// BlankLine is an empty line in the source.
type BlankLine struct {
	Line        uint32
	Content     string
	StartOffset uint32
}

// Config is the parsed configuration a check receives.
//
// Directives keeps the block nesting; Comments and BlankLines are flat,
// because Go has no comfortable way to model the parser's item union. Every
// one of them carries its line and offset, so a rule that needs them
// interleaved with the directives can order them by position.
type Config struct {
	// Path is the file being linted.
	Path string
	// Directives are the top-level directives, each with its own Block.
	Directives []Directive
	// Comments and BlankLines are collected from the whole file.
	Comments   []Comment
	BlankLines []BlankLine
	// IncludeContext names the blocks this file was included from, outermost
	// first, and is empty for a file linted directly.
	IncludeContext []string
}

// All calls visit for every directive at any depth, in source order.
func (c Config) All(visit func(Directive)) {
	var walk func([]Directive)
	walk = func(directives []Directive) {
		for _, directive := range directives {
			visit(directive)
			walk(directive.Block)
		}
	}
	walk(c.Directives)
}

// Named calls visit for every directive with this name, at any depth.
func (c Config) Named(name string, visit func(Directive)) {
	c.All(func(directive Directive) {
		if directive.Is(name) {
			visit(directive)
		}
	})
}

// IsIncludedFrom reports whether the file was included from the named block.
func (c Config) IsIncludedFrom(block string) bool {
	for _, context := range c.IncludeContext {
		if context == block {
			return true
		}
	}
	return false
}

// Spec is the rule's metadata, as `nginx-lint why` renders it. Optional fields
// are left empty rather than wrapped in an option, and the plugin API version
// is filled in by the SDK.
type Spec struct {
	// Name is the rule name the CLI reports and `--rule-only` selects.
	Name string
	// Category groups the rule, e.g. "security" or "best-practices".
	Category    string
	Description string
	// Severity is the default severity, "error" or "warning".
	Severity string
	// Why explains the reasoning behind the rule.
	Why string
	// BadExample and GoodExample are configuration snippets, usually embedded
	// with go:embed and passed through Trim.
	BadExample  string
	GoodExample string
	References  []string
	// MinNginxVersion and MaxNginxVersion bound the versions the rule applies
	// to, and are empty when it applies to all of them.
	MinNginxVersion string
	MaxNginxVersion string
}

// LintError is one finding. The rule name and category are filled in from the
// spec, so a check does not repeat them.
type LintError struct {
	Message  string
	Severity Severity
	Line     uint32
	Column   uint32
	Fixes    []Fix
}

// Warning reports a warning at the directive's position.
func Warning(d Directive, message string) LintError {
	return LintError{
		Message:  message,
		Severity: SeverityWarning,
		Line:     d.Line,
		Column:   d.Column,
	}
}

// Error reports an error at the directive's position.
func Error(d Directive, message string) LintError {
	return LintError{
		Message:  message,
		Severity: SeverityError,
		Line:     d.Line,
		Column:   d.Column,
	}
}

// At moves the finding to an explicit position, for a rule that reports
// somewhere other than the start of a directive.
func (e LintError) At(line, column uint32) LintError {
	e.Line, e.Column = line, column
	return e
}

// WithFix attaches a fix. A finding may carry more than one.
func (e LintError) WithFix(fixes ...Fix) LintError {
	e.Fixes = append(e.Fixes, fixes...)
	return e
}

// Plugin is what a rule implements.
type Plugin interface {
	// Spec returns the rule's metadata. It is called on its own by
	// `nginx-lint why`, so it must not depend on Check having run.
	Spec() Spec
	// Check reports every finding in cfg.
	Check(cfg Config) []LintError
}

var registered Plugin

// Register makes p the plugin this component exports. Call it from an init
// function: the host calls the exports directly and main never runs.
func Register(p Plugin) { registered = p }

// Registered returns the plugin passed to Register. It exists for the export
// glue in the exports package and is not needed by a plugin.
func Registered() Plugin { return registered }
