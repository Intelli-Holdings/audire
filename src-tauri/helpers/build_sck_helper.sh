#!/usr/bin/env bash
# Build the ScreenCaptureKit helper binary for macOS.
# Run this on macOS before building the Tauri app.
#
# The compiled binary (~200KB) should be placed in the app bundle's
# MacOS or Resources directory.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

echo "Building audire_sck_helper..."
swiftc -O \
    -o audire_sck_helper \
    audire_sck_helper.swift \
    -framework ScreenCaptureKit \
    -framework CoreMedia \
    -framework AVFoundation

echo "Built: $SCRIPT_DIR/audire_sck_helper"
ls -la audire_sck_helper
echo "Copy this binary to the Tauri bundle Resources or MacOS directory."
