#!/bin/sh
# Check the winget manifest that ships with this repository against the release
# it names.
#
# `winget validate` (run in the `winget` CI job on Windows) checks the manifest
# as a document: schema, required fields, hash *format*. It cannot check that the
# hash belongs to the file at that URL, because it does not download it. That is
# the failure this script exists for: a pinned version whose hash was copied from
# the wrong release installs nothing, and the first person to find out is a user.
#
# Usage: packaging/winget/verify.sh [VERSION_DIR]
#   default: the highest version directory in packaging/winget/
set -eu

root=$(cd "$(dirname "$0")" && pwd)
dir="${1:-$(ls -d "$root"/*/ | sort -V | tail -n 1)}"
dir="${dir%/}"

say() { printf '%s\n' "$*"; }
err() { printf 'winget-verify: %s\n' "$*" >&2; exit 1; }

manifest="$dir/RUdolf1517.tzim.installer.yaml"
[ -f "$manifest" ] || err "no installer manifest in $dir"

field() {
    sed -n "s/^ *$1: *//p" "$manifest" | head -n 1 | tr -d '\r'
}

version=$(sed -n 's/^PackageVersion: *//p' "$manifest" | head -n 1 | tr -d '\r')
url=$(field InstallerUrl)
want=$(field InstallerSha256 | tr 'A-F' 'a-f')

[ -n "$version" ] || err "PackageVersion is missing"
[ -n "$url" ] || err "InstallerUrl is missing"
[ -n "$want" ] || err "InstallerSha256 is missing"

case "$version" in
    "$(basename "$dir")") ;;
    *) err "PackageVersion $version does not match the directory $(basename "$dir")" ;;
esac

case "$url" in
    *"/v$version/"*) ;;
    *) err "InstallerUrl does not point at v$version: $url" ;;
esac

tmp=$(mktemp -d) || err "cannot create a temporary directory"
trap 'rm -rf "$tmp"' EXIT

say "downloading $url"
if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$tmp/asset"
else
    wget -qO "$tmp/asset" "$url"
fi

if command -v sha256sum >/dev/null 2>&1; then
    got=$(sha256sum "$tmp/asset" | cut -d' ' -f1)
else
    got=$(shasum -a 256 "$tmp/asset" | cut -d' ' -f1)
fi

[ "$got" = "$want" ] || err "SHA-256 mismatch
  manifest $want
  download $got
The manifest would install a different file than the one it describes."

# The manifest declares `NestedInstallerType: portable` with
# `RelativeFilePath: tzim.exe`, which is only true if the zip has the binary at
# its root. A zip with a top-level directory would install nothing.
if command -v unzip >/dev/null 2>&1; then
    unzip -l "$tmp/asset" | grep -q " tzim\.exe$" \
        || err "tzim.exe is not at the root of the archive"
fi

say "ok: RUdolf1517.tzim $version, hash matches, tzim.exe at the archive root"
