#!/bin/sh
# Build Folio.app locally. Release signing is Developer ID + hardened runtime
# when CODESIGN_IDENTITY is set (scripts/release.sh does that). A plain
# invocation still ad-hoc signs, so cargo-less local runs keep working.
set -eu
cd "$(dirname "$0")/.."
# shellcheck disable=SC1091
. packaging/release.env

# Pin the artifact directory. Cursor/sandbox builds often set
# CARGO_TARGET_DIR somewhere else; copying `target/$profile/folio`
# would then ship yesterday's binary (and reopen GitHub Releases).
profile=release
if [ "${1:-}" = "--debug" ]; then
    profile=debug
    cargo build --locked --target-dir target
else
    cargo build --locked --release --target-dir target
fi

cargo_version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
if [ -z "$cargo_version" ]; then
    printf 'Could not read version from Cargo.toml\n' >&2
    exit 1
fi
if [ "$cargo_version" != "$VERSION" ]; then
    printf 'Cargo.toml is %s, packaging/release.env is %s\n' "$cargo_version" "$VERSION" >&2
    exit 1
fi

sparkle_dir=$(sh scripts/sparkle.sh)

bundle="target/Folio.app"
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" "$bundle/Contents/Frameworks"
mkdir -p "$bundle/Contents/Resources/Licenses"
cp assets/OFL.txt "$bundle/Contents/Resources/Licenses/JetBrainsMono-OFL.txt"
cp assets/fonts/*-LICENSE.txt "$bundle/Contents/Resources/Licenses/"

icon_source="assets/app-icon/logo-1024.png"
iconset="target/Folio.iconset"
rm -rf "$iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$icon_source" \
        --out "$iconset/icon_${size}x${size}.png" >/dev/null
    retina=$((size * 2))
    sips -z "$retina" "$retina" "$icon_source" \
        --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/Folio.icns"

bin="target/$profile/folio"
if [ ! -x "$bin" ]; then
    printf 'Missing %s after cargo build\n' "$bin" >&2
    exit 1
fi
# Old builds opened GitHub Releases. Refuse to package one of those.
if strings "$bin" | grep -q 'github.com/08820048/folio/releases'; then
    printf '%s still opens GitHub Releases; rebuild after the Sparkle change.\n' "$bin" >&2
    exit 1
fi
if ! strings "$bin" | grep -q 'Updates ship in the packaged Mac app'; then
    printf '%s has no Sparkle fallback string; refusing to package a stale binary.\n' "$bin" >&2
    exit 1
fi
cp "$bin" "$bundle/Contents/MacOS/Folio.new"
mv "$bundle/Contents/MacOS/Folio.new" "$bundle/Contents/MacOS/Folio"
chmod +x "$bundle/Contents/MacOS/Folio"

/usr/bin/ditto "$sparkle_dir/Sparkle.framework" "$bundle/Contents/Frameworks/Sparkle.framework"

sed \
    -e "s/<string>VERSION<\\/string>/<string>${VERSION}<\\/string>/" \
    -e "s/<string>BUILD<\\/string>/<string>${BUILD}<\\/string>/" \
    packaging/Info.plist > "$bundle/Contents/Info.plist"

identity="${CODESIGN_IDENTITY:-}"
# release.env names the Developer ID but daily `package.sh` stays ad-hoc
# unless the caller exported CODESIGN_IDENTITY.
if [ -z "$identity" ] || [ "$identity" = "-" ]; then
    codesign --force --deep --sign - "$bundle"
else
    sign_component() {
        codesign --force --options runtime --timestamp --sign "$identity" \
            --entitlements packaging/Folio.entitlements "$@"
    }
    framework="$bundle/Contents/Frameworks/Sparkle.framework"
    if [ -f "$framework/Versions/B/Autoupdate" ]; then
        codesign --force --options runtime --timestamp --sign "$identity" \
            "$framework/Versions/B/Autoupdate"
    fi
    find "$framework" \( -name "*.xpc" -o -name "*.app" \) -type d -print \
        | awk '{ depth = gsub("/", "/"); print depth " " $0 }' \
        | sort -rn \
        | cut -d' ' -f2- \
        | while IFS= read -r component; do
            codesign --force --options runtime --timestamp --sign "$identity" \
                "$component"
        done
    codesign --force --options runtime --timestamp --sign "$identity" "$framework"
    sign_component "$bundle"
fi

printf 'Built %s (%s build %s)\n' "$bundle" "$VERSION" "$BUILD"
