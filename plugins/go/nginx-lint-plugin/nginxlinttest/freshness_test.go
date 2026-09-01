package nginxlinttest

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
)

// The committed wasm modules are built from crates that live in the same
// repository, so they go stale the moment someone changes the parser or the
// fix applier and does not rebuild them. A byte comparison cannot catch that:
// rustc does not produce the same wasm across toolchains, so a fresh build on
// CI differs from a fresh build on a contributor's machine even with
// identical source.
//
// What can be compared is behaviour. This test runs the committed modules and
// a fresh build of the crates over every configuration in the repository and
// requires them to agree, byte for byte, on the output. It is skipped unless
// the two environment variables point at a fresh build, so an ordinary
// `go test` needs no Rust toolchain; `make check-testkit-wasm` at the
// repository root builds them and runs it.
const (
	freshParserEnv = "NGINX_LINT_FRESH_PARSER_WASM"
	freshFixerEnv  = "NGINX_LINT_FRESH_FIXER_WASM"
)

func TestCommittedModulesStillMatchTheCrates(t *testing.T) {
	freshParser, freshFixer := os.Getenv(freshParserEnv), os.Getenv(freshFixerEnv)
	if freshParser == "" || freshFixer == "" {
		t.Skipf("set %s and %s to a fresh build of the crates to run this",
			freshParserEnv, freshFixerEnv)
	}

	corpus := corpus(t)
	if len(corpus) == 0 {
		t.Fatal("found no configurations to compare over")
	}
	t.Logf("comparing over %d configurations", len(corpus))

	ctx := context.Background()
	committedParser, err := compileBytes(ctx, parserWasm)
	if err != nil {
		t.Fatalf("compiling the committed parser: %v", err)
	}
	rebuiltParser, err := compileBytes(ctx, readFile(t, freshParser))
	if err != nil {
		t.Fatalf("compiling the fresh parser: %v", err)
	}
	committedFixer, err := compileBytes(ctx, fixerWasm)
	if err != nil {
		t.Fatalf("compiling the committed fix applier: %v", err)
	}
	rebuiltFixer, err := compileBytes(ctx, readFile(t, freshFixer))
	if err != nil {
		t.Fatalf("compiling the fresh fix applier: %v", err)
	}

	for _, path := range corpus {
		source, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("reading %s: %v", path, err)
		}

		committed, err := invoke(ctx, committedParser, "parse_config_json", source, nil)
		if err != nil {
			t.Fatalf("parsing %s with the committed module: %v", path, err)
		}
		rebuilt, err := invoke(ctx, rebuiltParser, "parse_config_json", source, nil)
		if err != nil {
			t.Fatalf("parsing %s with the fresh module: %v", path, err)
		}
		if !bytes.Equal(committed, rebuilt) {
			t.Fatalf("the committed parser and a fresh build of nginx-lint-parser "+
				"disagree on %s — rebuild and commit the modules with "+
				"`make build-testkit-wasm`\ncommitted: %s\nfresh:     %s",
				path, excerpt(committed, rebuilt), excerpt(rebuilt, committed))
		}

		// Applying every directive's own replacement exercises the applier's
		// sorting and its overlap handling, not just one edit in isolation.
		request := fixRequest(t, source, committed)
		committed, err = invoke(ctx, committedFixer, "apply_fixes_json", request)
		if err != nil {
			t.Fatalf("fixing %s with the committed module: %v", path, err)
		}
		rebuilt, err = invoke(ctx, rebuiltFixer, "apply_fixes_json", request)
		if err != nil {
			t.Fatalf("fixing %s with the fresh module: %v", path, err)
		}
		if !bytes.Equal(committed, rebuilt) {
			t.Fatalf("the committed fix applier and a fresh build of nginx-lint-common "+
				"disagree on %s — rebuild and commit the modules with "+
				"`make build-testkit-wasm`\ncommitted: %s\nfresh:     %s",
				path, excerpt(committed, rebuilt), excerpt(rebuilt, committed))
		}
	}
}

// fixRequest asks for one replacement per directive, built from the parse the
// committed module produced so both appliers get identical input.
func fixRequest(t *testing.T, source, parsed []byte) []byte {
	t.Helper()

	var response parseResponse
	if err := json.Unmarshal(parsed, &response); err != nil {
		t.Fatalf("decoding the parse output: %v", err)
	}
	request := applyRequest{Content: string(source), Fixes: []jsonFix{}}
	if response.Output == nil {
		return encode(t, request)
	}

	response.Output.Config("nginx.conf").All(func(directive nginxlint.Directive) {
		request.Fixes = append(request.Fixes, toJSONFix(directive.ReplaceWith("# replaced")))
	})
	return encode(t, request)
}

func encode(t *testing.T, request applyRequest) []byte {
	t.Helper()
	encoded, err := json.Marshal(request)
	if err != nil {
		t.Fatalf("encoding the fix request: %v", err)
	}
	return encoded
}

// corpus is every nginx configuration in the repository, which is a far wider
// range of syntax than this package's own fixtures.
func corpus(t *testing.T) []string {
	t.Helper()

	root := filepath.Join("..", "..", "..", "..")
	if _, err := os.Stat(filepath.Join(root, "Cargo.toml")); err != nil {
		t.Skip("not a repository checkout, so there is nothing to compare over")
	}

	var paths []string
	for _, dir := range []string{
		filepath.Join("crates", "nginx-lint-parser", "tests", "fixtures"),
		filepath.Join("tests", "fixtures"),
		filepath.Join("plugins", "builtin"),
	} {
		err := filepath.WalkDir(filepath.Join(root, dir), func(path string, entry os.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if !entry.IsDir() && strings.HasSuffix(path, ".conf") {
				paths = append(paths, path)
			}
			return nil
		})
		if err != nil {
			t.Fatalf("walking %s: %v", dir, err)
		}
	}
	return paths
}

func readFile(t *testing.T, path string) []byte {
	t.Helper()
	content, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("reading %s: %v", path, err)
	}
	return content
}

// excerpt shows where two outputs first differ, since they are long enough
// that printing both whole would bury the difference.
func excerpt(value, other []byte) string {
	at := 0
	for at < len(value) && at < len(other) && value[at] == other[at] {
		at++
	}
	start := max(at-40, 0)
	end := min(at+80, len(value))
	return "…" + string(value[start:end]) + "…"
}
