#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MAC="$(cd "$(dirname "$0")" && pwd)"
DIST="$ROOT/dist/wallet"
APP="$DIST/Phoenix — GRID Wallet.app"
SWIFT_CACHE="$ROOT/target/swift-cache"

cd "$MAC"
mkdir -p "$SWIFT_CACHE"
CLANG_MODULE_CACHE_PATH="$SWIFT_CACHE/clang" \
  SWIFTPM_MODULECACHE_OVERRIDE="$SWIFT_CACHE/swiftpm" \
  swift build -c release

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp ".build/release/GRIDWallet" "$APP/Contents/MacOS/GRIDWallet"
cp "$ROOT/target/release/grid" "$APP/Contents/Resources/grid"
chmod 755 "$APP/Contents/MacOS/GRIDWallet" "$APP/Contents/Resources/grid"

cp "$MAC/Info.plist" "$APP/Contents/Info.plist"
codesign --force --deep --sign - "$APP"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$DIST/GRID-Wallet-macOS-x86_64.zip"
echo "$APP"
