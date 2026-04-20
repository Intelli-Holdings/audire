#!/bin/bash
# bump-version.sh - Bump the patch version across all project files
# Usage:
#   ./scripts/bump-version.sh          # bump patch (0.1.0 → 0.1.1)
#   ./scripts/bump-version.sh minor    # bump minor (0.1.5 → 0.2.0)
#   ./scripts/bump-version.sh major    # bump major (0.1.5 → 1.0.0)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Files that contain the version
PACKAGE_JSON="$ROOT/package.json"
CARGO_TOML="$ROOT/src-tauri/Cargo.toml"
TAURI_CONF="$ROOT/src-tauri/tauri.conf.json"

# Read current version from package.json
CURRENT=$(grep '"version"' "$PACKAGE_JSON" | head -1 | sed 's/.*"version": *"\([^"]*\)".*/\1/')

if [[ -z "$CURRENT" ]]; then
  echo "Error: could not read current version from package.json"
  exit 1
fi

# Parse semver components
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

# Determine bump type (default: patch)
BUMP="${1:-patch}"

case "$BUMP" in
  patch)
    PATCH=$((PATCH + 1))
    ;;
  minor)
    MINOR=$((MINOR + 1))
    PATCH=0
    ;;
  major)
    MAJOR=$((MAJOR + 1))
    MINOR=0
    PATCH=0
    ;;
  *)
    echo "Usage: $0 [patch|minor|major]"
    exit 1
    ;;
esac

NEW_VERSION="$MAJOR.$MINOR.$PATCH"

echo "Bumping version: $CURRENT → $NEW_VERSION"

# Update package.json
sed -i "s/\"version\": \"$CURRENT\"/\"version\": \"$NEW_VERSION\"/" "$PACKAGE_JSON"
echo "  Updated package.json"

# Update Cargo.toml (only the package version line, not dependency versions)
sed -i "0,/^version = \"$CURRENT\"/s//version = \"$NEW_VERSION\"/" "$CARGO_TOML"
echo "  Updated src-tauri/Cargo.toml"

# Update tauri.conf.json
sed -i "s/\"version\": \"$CURRENT\"/\"version\": \"$NEW_VERSION\"/" "$TAURI_CONF"
echo "  Updated src-tauri/tauri.conf.json"

echo ""
echo "Version bumped to $NEW_VERSION"
