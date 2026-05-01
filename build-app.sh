#!/usr/bin/env bash
# Builds yabai-id.app — a macOS app bundle that can be dragged to /Applications.
set -euo pipefail

APP_NAME="yabai-id"
BUNDLE="${APP_NAME}.app"
BINARY_NAME="yabai-id"

echo "Building release binary..."
cargo build --release

echo "Assembling ${BUNDLE}..."
rm -rf "${BUNDLE}"
mkdir -p "${BUNDLE}/Contents/MacOS"
mkdir -p "${BUNDLE}/Contents/Resources"

cp "target/release/${BINARY_NAME}"   "${BUNDLE}/Contents/MacOS/${BINARY_NAME}"
cp "assets/Info.plist"               "${BUNDLE}/Contents/Info.plist"
cp "assets/AppIcon.icns"             "${BUNDLE}/Contents/Resources/AppIcon.icns"

echo "Done: ${BUNDLE}"
echo ""
echo "Drag ${BUNDLE} to /Applications to install."
