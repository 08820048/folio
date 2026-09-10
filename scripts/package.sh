#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
profile=release
if [ "${1:-}" = "--debug" ]; then
    profile=debug
    cargo build --locked
else
    cargo build --locked --release
fi
bundle="target/Folio.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "target/$profile/folio" "$bundle/Contents/MacOS/Folio.new"
mv "$bundle/Contents/MacOS/Folio.new" "$bundle/Contents/MacOS/Folio"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Folio</string>
<key>CFBundleIdentifier</key><string>app.folio.editor</string>
<key>CFBundleName</key><string>Folio</string>
<key>CFBundleDisplayName</key><string>Folio</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>LSMultipleInstancesProhibited</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$bundle"
printf 'Built %s\n' "$bundle"
