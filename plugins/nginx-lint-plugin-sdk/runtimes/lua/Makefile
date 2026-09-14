# Builds the Lua plugin runtime: a core wasm module implementing the
# nginx-lint `plugin` world by delegating to a Lua script that is not part
# of the module. `nginx-lint-plugin-sdk build` writes the script into the finished
# module (see the slot in shim.c), so this build happens once, here, and
# plugin authors never need the toolchain below.
#
# The result, runtime.core.wasm, is committed and embedded into the
# nginx-lint-plugin-sdk binary. `make toolchain` fetches the pinned wasi-sdk and
# Lua sources into .toolchain/; wit-bindgen and wasm-tools are expected on
# PATH. Run `make` after changing anything under this directory or the WIT,
# then commit the result: the crate's tests compare the module's stamp (a
# hash of those inputs, recorded in its producers section) against the
# tree, so a stale module fails CI without CI needing wasi-sdk.

WASI_SDK_VERSION := 34.0
LUA_VERSION := 5.4.8

ROOT := ../../../..
WIT := $(ROOT)/wit/nginx-lint-plugin.wit
SDK_CRATE := $(ROOT)/crates/nginx-lint-plugin

UNAME_S := $(shell uname -s)
UNAME_M := $(shell uname -m)
ifeq ($(UNAME_S),Darwin)
  WASI_SDK_OS := macos
else
  WASI_SDK_OS := linux
endif
ifeq ($(UNAME_M),x86_64)
  WASI_SDK_ARCH := x86_64
else
  WASI_SDK_ARCH := arm64
endif
WASI_SDK_DIR := wasi-sdk-$(WASI_SDK_VERSION)-$(WASI_SDK_ARCH)-$(WASI_SDK_OS)
WASI_SDK ?= .toolchain/$(WASI_SDK_DIR)
LUA_SRC  ?= .toolchain/lua-$(LUA_VERSION)/src

CC := $(WASI_SDK)/bin/clang
# What the compile rules wait for (order-only), so that under `make -j` on
# a fresh checkout nothing compiles before the download has finished.
TOOLCHAIN := $(WASI_SDK)/bin/clang $(LUA_SRC)/lua.h

# The plugin API version the runtime declares, read from the SDK crate so
# the two cannot drift silently: the crate's tests check the stamped value
# against the crate.
API_VERSION := $(shell sed -n 's/^pub const API_VERSION: &str = "\(.*\)";/\1/p' $(SDK_CRATE)/src/types.rs)

# Everything the module is built from other than the toolchain itself.
# Their hash is the module's stamp.
STAMP_INPUTS := shim.c stubs.c nginx_lint.lua Makefile $(WIT)

LUA_CORE := lapi lcode lctype ldebug ldo ldump lfunc lgc llex lmem lobject lopcodes lparser lstate lstring ltable ltm lundump lvm lzio
# No io, os, package or debug: the sandbox has no file system, clock or
# environment, and linit.c is replaced by the shim's own library list.
LUA_LIBS := lauxlib lbaselib lcorolib lmathlib lstrlib ltablib lutf8lib
LUA_OBJS := $(addprefix build/,$(addsuffix .o,$(LUA_CORE) $(LUA_LIBS)))

# Lua's error handling is longjmp-based, which wasm only has through the
# exception-handling proposal: LLVM lowers setjmp/longjmp onto it
# (-wasm-enable-sjlj) and must emit the standardized try_table form, not the
# legacy one wasmtime rejects (-wasm-use-legacy-eh=false).
#
# l_signalT: lstate.h reaches for <signal.h>, which wasi-libc refuses to
# provide; the hook flag only needs to be an int here.
# lua_write*: print() and the panic handler write to stdout/stderr, which
# would pull fd_write in. There is nowhere for the output to go anyway.
CFLAGS := --target=wasm32-wasip1 -O2 \
  -mllvm -wasm-enable-sjlj -mllvm -wasm-use-legacy-eh=false \
  -Dl_signalT=int \
  '-Dlua_writestring(s,l)=((void)0)' '-Dlua_writeline()=((void)0)' '-Dlua_writestringerror(s,p)=((void)0)' \
  -DAPI_VERSION='"$(API_VERSION)"' \
  -I$(LUA_SRC) -Ibindings

# --export of the two slot symbols: see shim.c; a data symbol is exported as
# a global holding its address.
# -lc-printscan-long-double: the default vfprintf handles %Lf by writing a
# diagnostic to stderr and aborting, and that one reference to stderr keeps
# fd_write and fd_close in the link. The long-double variant has no such
# path. -lsetjmp provides the sjlj runtime; the stack is raised well above
# the 64 KiB default for the Lua parser's recursion.
LDFLAGS := -mexec-model=reactor -lsetjmp -lc-printscan-long-double \
  -Wl,--no-entry \
  -Wl,--export=nginx_lint_lua_script_slot -Wl,--export=nginx_lint_lua_script_slot_max \
  -Wl,--stack-first -Wl,-z,stack-size=1048576

.PHONY: all toolchain bindings check-imports stamp clean

all: runtime.core.wasm

# The pinned toolchain, fetched once. wasi-sdk publishes per-platform
# tarballs of one LLVM and one wasi-libc revision (see its VERSION file).
toolchain: $(TOOLCHAIN)

.toolchain/$(WASI_SDK_DIR)/bin/clang:
	mkdir -p .toolchain
	curl -sSfL "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-$(basename $(WASI_SDK_VERSION))/$(WASI_SDK_DIR).tar.gz" \
		| tar xz -C .toolchain

.toolchain/lua-$(LUA_VERSION)/src/lua.h:
	mkdir -p .toolchain
	curl -sSfL "https://www.lua.org/ftp/lua-$(LUA_VERSION).tar.gz" | tar xz -C .toolchain

# One rule produces all three files: with a target per file, `make -j`
# would run wit-bindgen once for each into the same directory.
bindings/.generated: $(WIT)
	mkdir -p bindings
	wit-bindgen c --world plugin --out-dir bindings $(dir $(WIT))
	touch $@

bindings/plugin.h bindings/plugin.c bindings/plugin_component_type.o: bindings/.generated

bindings: bindings/.generated

build:
	mkdir -p build

build/%.o: $(LUA_SRC)/%.c | build $(TOOLCHAIN)
	$(CC) $(CFLAGS) -c $< -o $@

# luaL_loadfilex is the one function in lauxlib.c that touches the file
# system. Renaming its definition lets stubs.c define the real name; the
# renamed original is unreachable and dropped by --gc-sections.
build/lauxlib.o: $(LUA_SRC)/lauxlib.c | build $(TOOLCHAIN)
	$(CC) $(CFLAGS) -DluaL_loadfilex=luaL_loadfilex_unused -c $< -o $@

build/%.o: %.c bindings/plugin.h | build $(TOOLCHAIN)
	$(CC) $(CFLAGS) -c $< -o $@

build/plugin.o: bindings/plugin.c | build $(TOOLCHAIN)
	$(CC) $(CFLAGS) -c $< -o $@

nginx_lint_lua.h: nginx_lint.lua
	xxd -i -n nginx_lint_lua $< > $@

build/shim.o: shim.c nginx_lint_lua.h Makefile

build/stubs.o: stubs.c Makefile

# Linked, checked for wasi:* imports, then stamped: the inputs hash and the
# versions go into the producers section, where the crate's tests read them.
# wit-bindgen is recorded too, since the bindings it generates from the WIT
# are an input the hash does not see.
runtime.core.wasm: $(LUA_OBJS) build/stubs.o build/shim.o build/plugin.o bindings/plugin_component_type.o $(STAMP_INPUTS)
	$(CC) $(CFLAGS) $(filter %.o,$^) $(LDFLAGS) -o build/runtime.linked.wasm
	@$(MAKE) --no-print-directory check-imports MODULE=build/runtime.linked.wasm
	wasm-tools metadata add build/runtime.linked.wasm \
		--sdk nginx-lint-lua-runtime-inputs=$$(cat $(STAMP_INPUTS) | $(SHA256)) \
		--sdk nginx-lint-plugin-api=$(API_VERSION) \
		--sdk lua=$(LUA_VERSION) \
		--sdk wasi-sdk=$(WASI_SDK_VERSION) \
		--processed-by wit-bindgen=$$(wit-bindgen --version | cut -d' ' -f2) \
		--licenses "MIT AND Apache-2.0 WITH LLVM-exception AND BSD-2-Clause" \
		-o $@

SHA256 := $(shell command -v sha256sum >/dev/null 2>&1 && echo "sha256sum" || echo "shasum -a 256") | cut -d' ' -f1

# Prints the stamp the tree currently calls for, for comparison with what
# `wasm-tools metadata show runtime.core.wasm` reports.
stamp:
	@cat $(STAMP_INPUTS) | $(SHA256)

# The runtime must run under the default plugin sandbox, which denies
# every wasi:* import.
MODULE ?= runtime.core.wasm
check-imports:
	@if wasm-tools print $(MODULE) | grep -q '(import "wasi'; then \
		echo "$(MODULE) imports WASI:"; wasm-tools print $(MODULE) | grep '(import "wasi'; exit 1; \
	else echo "ok: $(MODULE) has no wasi imports"; fi

clean:
	rm -rf build bindings runtime.core.wasm nginx_lint_lua.h
