#!/bin/sh
# Build myCommander.app (Apple Silicon) and zip it for a GitHub release.
# Usage: scripts/package.sh   -> dist/myCommander-<version>-macos-arm64.zip
set -eu
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cargo build --release
APP=dist/myCommander.app
rm -rf dist && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp "${CARGO_TARGET_DIR:-target}/release/myCommander" "$APP/Contents/MacOS/myCommander"
cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>myCommander</string>
  <key>CFBundleDisplayName</key><string>myCommander</string>
  <key>CFBundleIdentifier</key><string>io.github.say-kaj.mycommander</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundleExecutable</key><string>myCommander</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
</dict></plist>
EOF
codesign --force --deep --sign - "$APP"
ZIP="dist/myCommander-$VERSION-macos-arm64.zip"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "$ZIP"; shasum -a 256 "$ZIP"
