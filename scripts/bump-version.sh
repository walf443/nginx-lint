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

# Find all Cargo.toml files in the project
CARGO_FILES=(
    "$ROOT_DIR/Cargo.toml"
    "$ROOT_DIR/crates/nginx-lint-parser/Cargo.toml"
    "$ROOT_DIR/crates/nginx-lint-common/Cargo.toml"
    "$ROOT_DIR/crates/nginx-lint-plugin/Cargo.toml"
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
    echo "  Updated $relative"
done

# Update TypeScript plugin package.json
TS_PLUGIN_PKG="$ROOT_DIR/plugins/typescript/nginx-lint-plugin/package.json"
if [ -f "$TS_PLUGIN_PKG" ]; then
    sed_inplace "s/\"version\": \"[0-9]*\.[0-9]*\.[0-9]*\"/\"version\": \"$NEW_VERSION\"/" "$TS_PLUGIN_PKG"
    echo "  Updated plugins/typescript/nginx-lint-plugin/package.json"
fi

echo ""
echo "Done! Updated ${#CARGO_FILES[@]} Cargo.toml files and TypeScript plugin to version $NEW_VERSION."
echo ""
echo "Verify with: grep -r '^version' Cargo.toml crates/*/Cargo.toml plugins/builtin/*/*/Cargo.toml && grep '\"version\"' plugins/typescript/nginx-lint-plugin/package.json"
