MODULE := github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin
WIT_DIR := ../../../wit

.PHONY: copy-wit bindings test vet check clean

# The WIT is committed here, not copied at build time like the TypeScript and
# Python SDKs do, because componentize-go.toml points a consuming plugin at
# it: someone who runs `go get` on this module has no other copy. CI diffs it
# against the root the same way it does for the Rust crates.
copy-wit:
	@mkdir -p wit
	@if ! cmp -s $(WIT_DIR)/nginx-lint-plugin.wit wit/nginx-lint-plugin.wit; then \
		cp $(WIT_DIR)/nginx-lint-plugin.wit wit/nginx-lint-plugin.wit; \
		echo "  Refreshed wit/nginx-lint-plugin.wit"; \
	fi

# The generated bindings are committed too, for the same reason: a Go module
# is consumed as source and cannot run a code generator on the way in. Rerun
# this whenever the WIT changes; CI fails if the result differs from what is
# checked in.
bindings: copy-wit
	@command -v componentize-go >/dev/null || { \
		echo "componentize-go not found: go install github.com/bytecodealliance/componentize-go@v0.4.0"; \
		exit 1; \
	}
	rm -rf bindings
	componentize-go -d wit -w plugin bindings --format \
		--output bindings \
		--pkg-name $(MODULE)/bindings \
		--export-pkg-name $(MODULE)/exports

# Only the root package: everything else in the module imports the generated
# bindings, which have assembly-only functions that do not build for the host.
test:
	go test .

vet:
	go vet .
	GOOS=wasip1 GOARCH=wasm go vet ./export/ ./exports/...

check: vet test

clean:
	rm -rf bindings
