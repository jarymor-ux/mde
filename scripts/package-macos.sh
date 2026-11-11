#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ]; then
    echo "This script packages the macOS app only." >&2
    exit 1
fi
cargo build --release --locked
mkdir -p target/mde.app/Contents/MacOS
cp target/release/mde target/mde.app/Contents/MacOS/mde
cat > target/mde.app/Contents/Info.plist <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>mde</string>
<key>CFBundleDisplayName</key><string>mde</string>
<key>CFBundleIdentifier</key><string>app.mde.editor</string>
<key>CFBundleExecutable</key><string>mde</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict></plist>
PLIST
codesign --force --sign - target/mde.app
echo "Created target/mde.app"
