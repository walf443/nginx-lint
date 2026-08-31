// Package examples carries the rule's bad and good configurations into the
// component. They are embedded at build time, so the plugin reports them
// through `nginx-lint why` without needing filesystem access at runtime —
// the Go counterpart of the Rust SDK's include_str!.
//
// The files live in their own package because go:embed cannot reach outside
// the directory it is written in, and the plugin itself has to sit in
// export_wit_world/.
package examples

import _ "embed"

//go:embed bad.conf
var Bad string

//go:embed good.conf
var Good string
