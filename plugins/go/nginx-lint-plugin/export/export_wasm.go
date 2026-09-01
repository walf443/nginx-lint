//go:build wasm

// Package export links the component's exports into the binary. Blank import
// it from the plugin's main package:
//
//	import _ "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/export"
//
// The generated glue carries the //go:wasmexport directives and is reachable
// from nothing else, so without the import the component builds with no
// exports and the host rejects it. On the host this package is empty, which
// is what keeps the plugin's own package testable with a plain `go test`
// instead of needing a build tag of its own.
package export

import _ "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/bindings/wit_exports"
