#!/usr/bin/env bash
# Builds a universal yabai-id.app (Intel + Apple Silicon).
set -euo pipefail

APP="yabai-id.app"

echo "Adding Rust targets..."
rustup target add x86_64-apple-darwin aarch64-apple-darwin

echo "Building x86_64 (Intel)..."
cargo build --release --target x86_64-apple-darwin

echo "Building aarch64 (Apple Silicon)..."
cargo build --release --target aarch64-apple-darwin

echo "Creating universal binary..."
lipo -create \
    target/x86_64-apple-darwin/release/yabai-id \
    target/aarch64-apple-darwin/release/yabai-id \
    -output target/release-universal-yabai-id

echo "Assembling ${APP}..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
mkdir -p "$APP/Contents/Resources"
cp target/release-universal-yabai-id "$APP/Contents/MacOS/yabai-id"
cp assets/Info.plist                  "$APP/Contents/Info.plist"
cp assets/AppIcon.icns                "$APP/Contents/Resources/AppIcon.icns"

echo "Done: ${APP}"
echo ""
echo "Drag ${APP} to /Applications to install."
