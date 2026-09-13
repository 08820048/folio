#!/bin/sh
# Create the Folio Sparkle EdDSA key once. The private key stays in the
# login keychain under SPARKLE_KEYCHAIN_ACCOUNT. The public key is written
# to packaging/sparkle_public.txt so Info.plist can embed it.
set -eu
cd "$(dirname "$0")/.."
# shellcheck disable=SC1091
. packaging/release.env

sparkle_dir=$(sh scripts/sparkle.sh)
if [ -f packaging/sparkle_public.txt ]; then
    printf 'Public key already at packaging/sparkle_public.txt\n'
    cat packaging/sparkle_public.txt
    exit 0
fi

# generate_keys prints the public key after saving the private one.
# --account isolates Folio from other apps on this Mac.
output=$("$sparkle_dir/bin/generate_keys" --account "$SPARKLE_KEYCHAIN_ACCOUNT")
printf '%s\n' "$output"
key=$(printf '%s\n' "$output" | sed -n 's/.*<string>\([^<]*\)<\/string>.*/\1/p' | head -n 1)
if [ -z "$key" ]; then
    key=$(printf '%s\n' "$output" | awk '/SUPublicEDKey/{getline; gsub(/^[ \t<string>|<\/string>]/,""); print; exit}')
fi
if [ -z "$key" ]; then
    printf 'Could not read the public key from generate_keys\n' >&2
    exit 1
fi
printf '%s\n' "$key" > packaging/sparkle_public.txt
printf 'Wrote packaging/sparkle_public.txt\n'
