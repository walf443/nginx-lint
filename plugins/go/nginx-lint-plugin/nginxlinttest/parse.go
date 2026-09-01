// Package nginxlinttest runs a plugin against the real nginx-lint parser and
// the real fix applier, from a plain `go test`.
//
// The other SDKs' test runners call the linter's own parser in process. Go has
// no component-model runtime, so this one runs the same two crates compiled as
// core wasm modules under wazero instead. What a rule sees here is what it
// sees at run time: the same parse output, converted by the same code the
// component adapter uses, and fixes applied by the function `--fix` applies.
package nginxlinttest

import (
	"encoding/json"
	"fmt"
	"os"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
	"github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin/internal/snapshot"
)

type parseResponse struct {
	Output *snapshot.ParseOutput `json:"output"`
	Error  string                `json:"error"`
}

// Parse runs the real parser over source and returns what a check receives.
//
// includeContext names the blocks the file would have been included from, as
// `--context` does on the command line; leave it out for a file linted
// directly.
func Parse(path, source string, includeContext ...string) (nginxlint.Config, error) {
	context := []byte(nil)
	if len(includeContext) > 0 {
		encoded, err := json.Marshal(includeContext)
		if err != nil {
			return nginxlint.Config{}, err
		}
		context = encoded
	}

	out, err := call(parserModule, "parse_config_json", []byte(source), context)
	if err != nil {
		return nginxlint.Config{}, err
	}

	var response parseResponse
	if err := json.Unmarshal(out, &response); err != nil {
		return nginxlint.Config{}, fmt.Errorf("decoding the parse output: %w", err)
	}
	if response.Error != "" {
		return nginxlint.Config{}, fmt.Errorf("%s", response.Error)
	}
	if response.Output == nil {
		return nginxlint.Config{}, fmt.Errorf("the parser returned neither an output nor an error")
	}

	return response.Output.Config(path), nil
}

// ParseFile parses a configuration file from disk.
func ParseFile(path string, includeContext ...string) (nginxlint.Config, error) {
	source, err := os.ReadFile(path)
	if err != nil {
		return nginxlint.Config{}, err
	}
	return Parse(path, string(source), includeContext...)
}
