#!/usr/bin/env bash
# Local dev script: check installer artifact size <= 123 MB.
# Run after `npm run tauri build`.

set -euo pipefail

LIMIT=129331200  # 123 MB in bytes

BUNDLE_DIR="src-tauri/target/release/bundle"

found=0
for dir in "$BUNDLE_DIR/msi" "$BUNDLE_DIR/nsis"; do
  if [ -d "$dir" ]; then
    for f in "$dir"/*.msi "$dir"/*.exe; do
      [ -f "$f" ] || continue
      size=$(stat -c%s "$f" 2>/dev/null || stat -f%z "$f" 2>/dev/null || echo "0")
      found=1
      echo "ARTIFACT: $f ($size bytes)"
      if [ "$size" -gt "$LIMIT" ]; then
        echo "FAIL: $f exceeds 123 MB ($size > $LIMIT)"
        exit 1
      fi
    done
  fi
done

if [ "$found" -eq 0 ]; then
  echo "No installer artifacts found in $BUNDLE_DIR"
  echo "Run 'npm run tauri build' first."
  exit 1
fi

echo "All artifacts are within the 123 MB size limit."
