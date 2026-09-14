#!/bin/bash
set -euo pipefail

# Usage: scripts/bump-version.sh <new-version>
# Example: scripts/bump-version.sh 0.2.0

if [ $# -ne 1 ]; then
    echo "Usage: $0 <new-version>"
    echo "Example: $0 0.2.0"
    exit 1
fi

NEW_VERSION="$1"

# Validate version format (semver: X.Y.Z)
if ! echo "$NEW_VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "Error: Invalid version format '$NEW_VERSION'. Expected X.Y.Z (e.g., 0.2.0)"
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# The Go SDK's test-helper modules are rebuilt at the end, which needs the
# wasm32 target; check before touching anything so a missing target does not
# leave a half-applied bump.
if ! rustup target list --installed | grep -qx wasm32-unknown-unknown; then
    echo "Error: the wasm32-unknown-unknown target is not installed (rustup target add wasm32-unknown-unknown)"
    exit 1
fi

# Find all Cargo.toml files in the project
CARGO_FILES=(
    "$ROOT_DIR/Cargo.toml"
    "$ROOT_DIR/crates/nginx-lint-parser/Cargo.toml"
    "$ROOT_DIR/crates/nginx-lint-common/Cargo.toml"
    "$ROOT_DIR/crates/nginx-lint-plugin/Cargo.toml"
    # The Python SDK's crate version is the wheel version (pyproject
    # declares version dynamic), so it bumps with everything else
    "$ROOT_DIR/plugins/python/nginx-lint-plugin/Cargo.toml"
    # The Lua plugin builder is released as a binary next to nginx-lint
    "$ROOT_DIR/plugins/nginx-lint-plugin-sdk/Cargo.toml"
)

# Add all plugin Cargo.toml files
for dir in "$ROOT_DIR"/plugins/builtin/*/*/; do
    if [ -f "$dir/Cargo.toml" ]; then
        CARGO_FILES+=("$dir/Cargo.toml")
    fi
done

echo "Bumping version to $NEW_VERSION in ${#CARGO_FILES[@]} Cargo.toml files..."

sed_inplace() {
    if sed --version 2>/dev/null | grep -q GNU; then
        sed -i "$@"
    else
        sed -i '' "$@"
    fi
}

for file in "${CARGO_FILES[@]}"; do
    relative="${file#$ROOT_DIR/}"
    # Replace the version line in [package] section
    sed_inplace "s/^version = \"[0-9]*\.[0-9]*\.[0-9]*\"/version = \"$NEW_VERSION\"/" "$file"
    # Update internal crate dependency versions
    sed_inplace "s/\(nginx-lint-parser = { version = \"\)[0-9]*\.[0-9]*\.[0-9]*/\1$NEW_VERSION/" "$file"
    sed_inplace "s/\(nginx-lint-common = { version = \"\)[0-9]*\.[0-9]*\.[0-9]*/\1$NEW_VERSION/" "$file"
    sed_inplace "s/\(nginx-lint-plugin = { version = \"\)[0-9]*\.[0-9]*\.[0-9]*/\1$NEW_VERSION/" "$file"
    # The Python SDK's crate depends on the two library crates by version
    # only (see plugins/python/.cargo/config.toml for why), so those lines
    # have no `{ version = ... }` wrapper to match above
    sed_inplace "s/^\(nginx-lint-parser = \"=\)[0-9]*\.[0-9]*\.[0-9]*/\1$NEW_VERSION/" "$file"
    sed_inplace "s/^\(nginx-lint-common = \"=\)[0-9]*\.[0-9]*\.[0-9]*/\1$NEW_VERSION/" "$file"
    echo "  Updated $relative"
done

# Update TypeScript plugin package.json
TS_PLUGIN_PKG="$ROOT_DIR/plugins/typescript/nginx-lint-plugin/package.json"
if [ -f "$TS_PLUGIN_PKG" ]; then
    sed_inplace "s/\"version\": \"[0-9]*\.[0-9]*\.[0-9]*\"/\"version\": \"$NEW_VERSION\"/" "$TS_PLUGIN_PKG"
    echo "  Updated plugins/typescript/nginx-lint-plugin/package.json"
fi

# Update TypeScript plugin README.md
TS_PLUGIN_README="$ROOT_DIR/plugins/typescript/nginx-lint-plugin/README.md"
if [ -f "$TS_PLUGIN_README" ]; then
    sed_inplace "s/\"nginx-lint-plugin\": \"\\^[0-9]*\.[0-9]*\.[0-9]*\"/\"nginx-lint-plugin\": \"\\^$NEW_VERSION\"/" "$TS_PLUGIN_README"
    echo "  Updated plugins/typescript/nginx-lint-plugin/README.md"
fi

# The Python SDK is a separate cargo workspace, so no root cargo command
# refreshes its lockfile; without this it keeps the old version and the next
# build silently rewrites it.
PY_SDK_MANIFEST="$ROOT_DIR/plugins/python/nginx-lint-plugin/Cargo.toml"
if [ -f "$PY_SDK_MANIFEST" ]; then
    (cd "$(dirname "$PY_SDK_MANIFEST")" && cargo update --workspace --quiet)
    echo "  Updated plugins/python/nginx-lint-plugin/Cargo.lock"
fi

# The Go SDK's test helper embeds a committed build of nginx-lint-parser and
# nginx-lint-common, and that build carries the crate version: after a bump
# `make check-testkit-wasm` (and CI) fails until the modules are rebuilt from
# the bumped crates. Needs the wasm32-unknown-unknown target. This also
# refreshes the root Cargo.lock, which nothing above did.
echo "rebuild the Go SDK's test-helper wasm modules"
(cd "$ROOT_DIR" && make build-testkit-wasm)

echo "update Dockerfile image hashes"
dockerfile-pin run --write

echo ""
echo "Done! Updated ${#CARGO_FILES[@]} Cargo.toml files and TypeScript plugin to version $NEW_VERSION."
echo ""
echo "Verify with: grep -r '^version' Cargo.toml crates/*/Cargo.toml plugins/builtin/*/*/Cargo.toml plugins/python/nginx-lint-plugin/Cargo.toml && grep '\"version\"' plugins/typescript/nginx-lint-plugin/package.json && grep 'nginx-lint-plugin' plugins/typescript/nginx-lint-plugin/README.md"
