#!/bin/sh
# Build .deb and .rpm from the Linux release tarball. Used by
# .github/workflows/release.yml; can be run by hand against a release download.
#
# The packaged binary is the static musl build, so the packages declare no
# runtime dependency at all and work on any distribution with a 3.2+ kernel.
#
# Usage: packaging/linux-packages.sh VERSION DIST_DIR
set -eu

VERSION="${1:?usage: linux-packages.sh VERSION DIST_DIR}"
DIST="${2:?usage: linux-packages.sh VERSION DIST_DIR}"
NFPM_VER="${NFPM_VER:-2.48.0}"

if ! ls "$DIST"/tzim-*-unknown-linux-musl.tar.gz >/dev/null 2>&1; then
  echo "linux-packages: no Linux tarball in $DIST, skip"
  exit 0
fi

need() { command -v "$1" >/dev/null 2>&1; }

workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT

# nfpm turns one YAML description into both package formats, so the .deb and the
# .rpm cannot drift apart.
nfpm="$workdir/nfpm"
url="https://github.com/goreleaser/nfpm/releases/download/v${NFPM_VER}/nfpm_${NFPM_VER}_Linux_x86_64.tar.gz"
if need curl; then
  curl -fsSL "$url" | tar -xz -C "$workdir" nfpm
elif need wget; then
  wget -qO- "$url" | tar -xz -C "$workdir" nfpm
else
  echo "linux-packages: need curl or wget to fetch nfpm" >&2
  exit 1
fi
chmod +x "$nfpm"

pkg="$workdir/pkg"
mkdir -p "$pkg"
tar -xzf "$DIST"/tzim-x86_64-unknown-linux-musl.tar.gz -C "$pkg"
bin=$(find "$pkg" -type f -name tzim | head -n 1)
if [ -z "$bin" ]; then
  echo "linux-packages: no tzim binary inside the tarball" >&2
  exit 1
fi
chmod +x "$bin"

yaml="$workdir/nfpm.yaml"
{
  echo "name: tzim"
  echo "arch: amd64"
  echo "platform: linux"
  echo "version: $VERSION"
  echo "release: \"1\""
  echo "section: utils"
  echo "priority: optional"
  echo "maintainer: RUdolf1517 <rudolfzinovev@protonmail.com>"
  echo "description: |"
  echo "  Lossless file optimizer: shrink the file, never the information."
  echo "  PNG, APNG, gzip, ZIP, ICO/CUR and baseline JPEG, verified before replacement."
  echo "homepage: https://github.com/RUdolf1517/tzim"
  echo "license: MIT"
  echo "contents:"
  echo "  - src: $bin"
  echo "    dst: /usr/bin/tzim"
  echo "    file_info:"
  echo "      mode: 0755"
  if [ -f LICENSE ]; then
    echo "  - src: LICENSE"
    echo "    dst: /usr/share/doc/tzim/copyright"
  fi
} >"$yaml"

# nfpm writes the file name itself when --target is a directory; naming both
# outputs explicitly keeps the release assets predictable.
"$nfpm" pkg --packager deb --config "$yaml" --target "$DIST/tzim_amd64.deb"
"$nfpm" pkg --packager rpm --config "$yaml" --target "$DIST/tzim.x86_64.rpm"
ls -l "$DIST/tzim_amd64.deb" "$DIST/tzim.x86_64.rpm"
