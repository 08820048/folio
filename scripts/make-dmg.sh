#!/bin/sh
# Build a drag-to-Applications DMG with the Folio installer background.
# Usage: make-dmg.sh <Folio.app> <volume-name> <out.dmg>
set -eu
cd "$(dirname "$0")/.."

app=$1
volname=$2
out=$3
src=assets/app-icon/back.jpg

# back.jpg is 1808×1104. Treat as 2× of a 904×552 Finder window.
window_w=904
window_h=552
icon_size=128
app_x=238
# Finder reports icon Y ~45pt lower than the value we set (title bar +
# label). 264 is the pad center on the 1× background; 219 lands there.
app_y=219
apps_x=665
apps_y=219

if [ ! -d "$app" ]; then
    printf 'Missing app bundle: %s\n' "$app" >&2
    exit 1
fi
if [ ! -f "$src" ]; then
    printf 'Missing DMG background: %s\n' "$src" >&2
    exit 1
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/folio-dmg.XXXXXX")
trap 'rm -rf "$work"' EXIT

staging="$work/staging"
mkdir -p "$staging/.background"
/usr/bin/ditto "$app" "$staging/Folio.app"
ln -s /Applications "$staging/Applications"

# Finder wants a hidpi TIFF so the forest stays sharp on Retina.
sips -s format png -z 1104 1808 "$src" --out "$work/background@2x.png" >/dev/null
sips -s format png -z 552 904 "$src" --out "$work/background@1x.png" >/dev/null
tiffutil -cathidpicheck \
    "$work/background@1x.png" \
    "$work/background@2x.png" \
    -out "$staging/.background/background.tiff"

rw="$work/rw.dmg"
hdiutil create \
    -volname "$volname" \
    -srcfolder "$staging" \
    -ov \
    -format UDRW \
    -fs HFS+ \
    "$rw" >/dev/null

# Attach read-write so Finder can write the .DS_Store (icon positions).
# The mount path has spaces; take everything from /Volumes/ to the end.
mount=$(hdiutil attach -readwrite -noverify -noautoopen "$rw" \
    | sed -n 's/.*\(\/Volumes\/.*\)$/\1/p' | tail -n 1)
if [ -z "$mount" ] || [ ! -d "$mount" ]; then
    printf 'Could not mount the writable DMG\n' >&2
    exit 1
fi
# Finder names a colliding volume "Name 1"; talk to the name we actually got.
volname=$(basename "$mount")
device=$(df "$mount" | awk 'NR==2{print $1}')

cleanup_mount() {
    if [ -n "${device:-}" ]; then
        hdiutil detach "$device" -quiet >/dev/null 2>&1 || \
            hdiutil detach "$mount" -force -quiet >/dev/null 2>&1 || true
    fi
    rm -rf "$work"
}
trap cleanup_mount EXIT

# Give Finder a moment to see the volume before talking to it.
sleep 1
osascript - "$volname" "$window_w" "$window_h" "$icon_size" \
    "$app_x" "$app_y" "$apps_x" "$apps_y" <<'APPLESCRIPT'
on run argv
    set volName to item 1 of argv
    set winW to (item 2 of argv) as integer
    set winH to (item 3 of argv) as integer
    set iconSz to (item 4 of argv) as integer
    set appX to (item 5 of argv) as integer
    set appY to (item 6 of argv) as integer
    set appsX to (item 7 of argv) as integer
    set appsY to (item 8 of argv) as integer
    tell application "Finder"
        tell disk volName
            open
            set w to container window
            set current view of w to icon view
            set toolbar visible of w to false
            set statusbar visible of w to false
            set bounds of w to {120, 120, 120 + winW, 120 + winH}
            set opts to icon view options of w
            set arrangement of opts to not arranged
            set icon size of opts to iconSz
            set background picture of opts to file ".background:background.tiff"
            set position of item "Folio.app" to {appX, appY}
            set position of item "Applications" to {appsX, appsY}
            close
            open
            update without registering applications
            delay 1
            close
        end tell
    end tell
end run
APPLESCRIPT

# Detach before converting; retries if Finder still has the volume.
i=0
while [ "$i" -lt 8 ]; do
    if hdiutil detach "$device" -quiet >/dev/null 2>&1; then
        device=
        break
    fi
    sleep 1
    i=$((i + 1))
done
if [ -n "$device" ]; then
    hdiutil detach "$mount" -force -quiet >/dev/null 2>&1 || true
    device=
fi

rm -f "$out"
hdiutil convert "$rw" -format UDZO -imagekey zlib-level=9 -o "$out" >/dev/null
printf 'Wrote %s\n' "$out"
