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
icon_source="assets/app-icon/folio-03-code-bookmark-1024.png"
iconset="target/Folio.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$icon_source" \
        --out "$iconset/icon_${size}x${size}.png" >/dev/null
    retina=$((size * 2))
    sips -z "$retina" "$retina" "$icon_source" \
        --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/Folio.icns"
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
<key>CFBundleIconFile</key><string>Folio.icns</string>
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
