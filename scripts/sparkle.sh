#!/bin/sh
# Fetch the Sparkle tools and framework into target/sparkle, once per version.
set -eu
cd "$(dirname "$0")/.."
# shellcheck disable=SC1091
. packaging/release.env

dest="target/sparkle/$SPARKLE_VERSION"
if [ -x "$dest/bin/sign_update" ] && [ -d "$dest/Sparkle.framework" ]; then
    printf '%s\n' "$dest"
    exit 0
fi

mkdir -p "$dest"
archive="target/sparkle/Sparkle-${SPARKLE_VERSION}.tar.xz"
url="https://github.com/sparkle-project/Sparkle/releases/download/${SPARKLE_VERSION}/Sparkle-${SPARKLE_VERSION}.tar.xz"
if [ ! -f "$archive" ]; then
    curl -fsSL "$url" -o "$archive"
fi
tar -xJf "$archive" -C "$dest"
if [ ! -x "$dest/bin/sign_update" ] || [ ! -d "$dest/Sparkle.framework" ]; then
    printf 'Sparkle %s did not unpack the expected tools\n' "$SPARKLE_VERSION" >&2
    exit 1
fi
printf '%s\n' "$dest"
