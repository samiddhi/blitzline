#!/bin/sh
# Package the local prototype for opening from Finder. Not a signed release.
set -eu
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != "Darwin" ]; then
    echo "This packaging helper is for macOS." >&2
    exit 1
fi
profile=release
if [ "${1:-}" = "--debug" ]; then
    profile=debug
    cargo build --locked -p blitzline-desktop
else
    cargo build --locked --release -p blitzline-desktop
fi
bundle="target/Blitzline Prototype.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "target/$profile/blitzline-desktop" "$bundle/Contents/MacOS/blitzline-desktop"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Blitzline Prototype</string>
  <key>CFBundleDisplayName</key><string>Blitzline Prototype</string>
  <key>CFBundleIdentifier</key><string>org.blitzline.prototype</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleExecutable</key><string>blitzline-desktop</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
</dict></plist>
PLIST
echo "Ready: $bundle"
