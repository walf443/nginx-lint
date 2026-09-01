package nginxlinttest

import (
	"context"
	_ "embed"
	"fmt"
	"sync"

	"github.com/tetratelabs/wazero"
	"github.com/tetratelabs/wazero/api"
)

// The parser and the fix applier, compiled from the same crates the linter
// itself uses, as core wasm modules with no imports and a JSON entry point.
// They are committed because a Go module is consumed as source; `make
// build-testkit-wasm` at the repository root regenerates them.
//
//go:embed parser.wasm
var parserWasm []byte

//go:embed fixer.wasm
var fixerWasm []byte

// Compiling is the expensive part and the modules are stateless between
// calls, so each is compiled once for the whole test binary and instantiated
// per call. wazero is a pure-Go runtime: no cgo, and no component-model
// runtime, which Go does not have — which is the whole reason these are core
// modules rather than the components the other SDKs use.
// module names which of the two to run. They are selected after compiling
// rather than passed in, because the compiled modules do not exist until the
// first call.
type module int

const (
	parserModule module = iota
	fixerModule
)

var (
	once       sync.Once
	runtime    wazero.Runtime
	parser     wazero.CompiledModule
	fixer      wazero.CompiledModule
	compileErr error
)

func compile(ctx context.Context) error {
	once.Do(func() {
		runtime = wazero.NewRuntime(ctx)
		if parser, compileErr = runtime.CompileModule(ctx, parserWasm); compileErr != nil {
			compileErr = fmt.Errorf("compiling the parser: %w", compileErr)
			return
		}
		if fixer, compileErr = runtime.CompileModule(ctx, fixerWasm); compileErr != nil {
			compileErr = fmt.Errorf("compiling the fix applier: %w", compileErr)
		}
	})
	return compileErr
}

// call runs one JSON entry point on one of the embedded modules.
func call(which module, name string, args ...[]byte) ([]byte, error) {
	ctx := context.Background()
	if err := compile(ctx); err != nil {
		return nil, err
	}

	compiled := parser
	if which == fixerModule {
		compiled = fixer
	}
	return invoke(ctx, compiled, name, args...)
}

// compileBytes compiles a module that is not one of the embedded pair. It
// exists for the test that checks the committed modules against a fresh build
// of the crates.
func compileBytes(ctx context.Context, wasm []byte) (wazero.CompiledModule, error) {
	if err := compile(ctx); err != nil {
		return nil, err
	}
	return runtime.CompileModule(ctx, wasm)
}

// invoke runs one JSON entry point: it writes each argument into the module's
// memory and reads back the single JSON string the export returns.
func invoke(ctx context.Context, compiled wazero.CompiledModule, name string, args ...[]byte) ([]byte, error) {
	// A fresh instance per call, so one parse cannot see what a previous one
	// left in the module's memory.
	instance, err := runtime.InstantiateModule(ctx, compiled, wazero.NewModuleConfig().WithName(""))
	if err != nil {
		return nil, fmt.Errorf("instantiating %s: %w", name, err)
	}
	defer instance.Close(ctx)

	// A module that does not export what is asked for gives a nil function,
	// and calling that panics rather than failing. The likeliest way to get
	// here is pointing the freshness check at the wrong artifact — a
	// component build rather than a wasm-json one — so name the export.
	alloc := instance.ExportedFunction("alloc")
	if alloc == nil {
		return nil, fmt.Errorf("the module does not export alloc")
	}
	entry := instance.ExportedFunction(name)
	if entry == nil {
		return nil, fmt.Errorf("the module does not export %s", name)
	}

	params := make([]uint64, 0, len(args)*2)
	for _, arg := range args {
		if len(arg) == 0 {
			params = append(params, 0, 0)
			continue
		}
		result, err := alloc.Call(ctx, uint64(len(arg)))
		if err != nil {
			return nil, fmt.Errorf("allocating in %s: %w", name, err)
		}
		if !instance.Memory().Write(uint32(result[0]), arg) {
			return nil, fmt.Errorf("writing an argument into %s", name)
		}
		params = append(params, result[0], uint64(len(arg)))
	}

	result, err := entry.Call(ctx, params...)
	if err != nil {
		return nil, fmt.Errorf("calling %s: %w", name, err)
	}
	return read(instance, result[0], name)
}

// read unpacks the (pointer, length) pair the entry points return as one u64,
// which is how a wasm export returns two numbers.
func read(instance api.Module, packed uint64, name string) ([]byte, error) {
	out, ok := instance.Memory().Read(uint32(packed>>32), uint32(packed&0xffffffff))
	if !ok {
		return nil, fmt.Errorf("reading the result of %s", name)
	}
	// The bytes belong to an instance that is about to close.
	return append([]byte(nil), out...), nil
}
