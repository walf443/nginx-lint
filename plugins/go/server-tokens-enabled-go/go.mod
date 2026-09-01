module github.com/walf443/nginx-lint/plugins/go/server-tokens-enabled-go

go 1.25.0

require github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin v0.0.0

require (
	github.com/tetratelabs/wazero v1.12.0 // indirect
	go.bytecodealliance.org/pkg v0.2.3 // indirect
	golang.org/x/sys v0.44.0 // indirect
)

// The SDK is in this repository and is released with it, so the example
// builds against the working tree rather than a published version — the same
// reason the Rust plugins use a path dependency.
replace github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin => ../nginx-lint-plugin
