#!/bin/sh
# Local Mac release: Developer ID app, notarized DMG, Sparkle zip + appcast,
# then upload to the folio-releases R2 bucket. Does not create a GitHub Release.
set -eu
cd "$(dirname "$0")/.."
# shellcheck disable=SC1091
. packaging/release.env

notes="${RELEASE_NOTES:-Improvements and fixes.}"
skip_upload=0
while [ $# -gt 0 ]; do
    case "$1" in
        --skip-upload) skip_upload=1 ;;
        --notes) notes=$2; shift ;;
        *)
            printf 'Usage: %s [--skip-upload] [--notes "html or text"]\n' "$0" >&2
            exit 64
            ;;
    esac
    shift
done

export CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-$RELEASE_CODESIGN_IDENTITY}"
export CLOUDFLARE_ACCOUNT_ID
sh scripts/package.sh

app="target/Folio.app"
sparkle_dir=$(sh scripts/sparkle.sh)
artifacts="target/release-artifacts/$VERSION"
mkdir -p "$artifacts"

html_name="Folio-$VERSION.html"
if printf '%s' "$notes" | grep -q '<'; then
    printf '%s\n' "$notes" > "$artifacts/$html_name"
else
    printf '<ul>\n  <li>%s</li>\n</ul>\n' "$notes" > "$artifacts/$html_name"
fi

dmg_path="$artifacts/Folio-$VERSION.dmg"
dmg_upload="Folio-$VERSION-$BUILD.dmg"
rm -f "$dmg_path"
sh scripts/make-dmg.sh "$app" "Install Folio" "$dmg_path"
codesign --force --sign "$CODESIGN_IDENTITY" --timestamp "$dmg_path"
xcrun notarytool submit "$dmg_path" --keychain-profile "$NOTARY_PROFILE" --wait
xcrun stapler staple "$dmg_path"
xcrun stapler validate "$dmg_path"
xcrun stapler staple "$app"
xcrun stapler validate "$app"
spctl --assess --type open --context context:primary-signature --verbose=4 "$dmg_path"
spctl --assess --type execute --verbose=4 "$app"

# Sparkle zip after the app is stapled, so the update users install is notarized.
zip_name="Folio-$VERSION.zip"
zip_path="$artifacts/$zip_name"
rm -f "$zip_path"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$app" "$zip_path"
sig_line=$("$sparkle_dir/bin/sign_update" --account "$SPARKLE_KEYCHAIN_ACCOUNT" "$zip_path")
sig=$(printf '%s' "$sig_line" | sed -n 's/.*sparkle:edSignature="\([^"]*\)".*/\1/p')
if [ -z "$sig" ]; then
    printf 'sign_update did not print an edSignature\n%s\n' "$sig_line" >&2
    exit 1
fi
length=$(stat -f '%z' "$zip_path")
pub_date=$(LC_ALL=C TZ=Asia/Shanghai date '+%a, %d %b %Y %H:%M:%S %z')

cat > "$artifacts/appcast.xml" <<EOF
<?xml version="1.0" standalone="yes"?>
<rss xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle" version="2.0">
    <channel>
        <title>Folio</title>
        <item>
            <title>$VERSION</title>
            <pubDate>$pub_date</pubDate>
            <sparkle:version>$BUILD</sparkle:version>
            <sparkle:shortVersionString>$VERSION</sparkle:shortVersionString>
            <sparkle:minimumSystemVersion>12.0</sparkle:minimumSystemVersion>
            <description><![CDATA[$(cat "$artifacts/$html_name")
]]></description>
            <enclosure url="$PUBLIC_BASE/$zip_name" length="$length" type="application/octet-stream" sparkle:edSignature="$sig"/>
        </item>
    </channel>
</rss>
EOF
xmllint --noout "$artifacts/appcast.xml"

cp "$dmg_path" "$artifacts/$dmg_upload"

if [ "$skip_upload" -eq 1 ]; then
    printf 'Artifacts in %s (upload skipped)\n' "$artifacts"
    exit 0
fi

put() {
    key=$1
    file=$2
    type=$3
    cache=$4
    npx --yes wrangler r2 object put "$R2_BUCKET/$key" \
        --remote \
        --file="$file" \
        --content-type="$type" \
        --cache-control="$cache"
}

put "$zip_name" "$zip_path" "application/zip" "public, max-age=31536000, immutable"
put "$dmg_upload" "$artifacts/$dmg_upload" "application/x-apple-diskimage" "public, max-age=31536000, immutable"
put "Folio-latest.dmg" "$artifacts/$dmg_upload" "application/x-apple-diskimage" "no-cache"
put "$html_name" "$artifacts/$html_name" "text/html; charset=utf-8" "no-cache"
put "appcast.xml" "$artifacts/appcast.xml" "application/xml" "no-cache"

printf '\nPublished Folio %s (build %s)\n' "$VERSION" "$BUILD"
printf '  Sparkle  %s/appcast.xml\n' "$PUBLIC_BASE"
printf '  Download %s/%s\n' "$PUBLIC_BASE" "$dmg_upload"
printf '  Latest   %s/Folio-latest.dmg\n' "$PUBLIC_BASE"
printf 'No GitHub Release was created.\n'
