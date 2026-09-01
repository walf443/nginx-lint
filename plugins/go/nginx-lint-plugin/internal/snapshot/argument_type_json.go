//go:build !wasm

package snapshot

import (
	"encoding/json"
	"fmt"

	nginxlint "github.com/walf443/nginx-lint/plugins/go/nginx-lint-plugin"
)

// argumentTypeNames are the names serde gives the WIT enum's cases.
var argumentTypeNames = map[string]nginxlint.ArgumentType{
	"Literal":            nginxlint.ArgumentLiteral,
	"QuotedString":       nginxlint.ArgumentQuotedString,
	"SingleQuotedString": nginxlint.ArgumentSingleQuotedString,
	"Variable":           nginxlint.ArgumentVariable,
}

func (t *ArgumentType) UnmarshalJSON(data []byte) error {
	var name string
	if err := json.Unmarshal(data, &name); err != nil {
		return err
	}
	value, ok := argumentTypeNames[name]
	if !ok {
		return fmt.Errorf("unknown argument type %q", name)
	}
	*t = ArgumentType(value)
	return nil
}
