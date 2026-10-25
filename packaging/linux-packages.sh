#!/bin/sh
# Build .deb and .rpm from the Linux release tarballs. Used by
# .github/workflows/release.yml; can be run by hand against a release download.
#
# The packaged binary is the static musl build, so the packages declare no
# runtime dependency at all and work on any distribution with a 3.2+ kernel.
# Every architecture that has a tarball in DIST gets a package: amd64 and arm64
# today. A missing tarball is skipped rather than fatal, so the script still
# works on a partial download.
#
# The packages carry what a Linux user expects to find in a package rather than
# in a tarball: the manual page and the three completions. They are unpacked
# from the release archive, which ships them (release.yml packages `man/` and
# `completions/` next to the binary), so the .deb, the .rpm, the Homebrew
# formula and the repository all describe the same program.
#
# Usage: packaging/linux-packages.sh VERSION DIST_DIR
set -eu

VERSION="${1:?usage: linux-packages.sh VERSION DIST_DIR}"
DIST="${2:?usage: linux-packages.sh VERSION DIST_DIR}"
NFPM_VER="${NFPM_VER:-2.48.0}"

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

# deb_arch:rpm_arch:rust_target
ARCHES="amd64:x86_64:x86_64-unknown-linux-musl arm64:aarch64:aarch64-unknown-linux-musl"

built=0
for entry in $ARCHES; do
  deb_arch=${entry%%:*}
  rest=${entry#*:}
  rpm_arch=${rest%%:*}
  target=${rest#*:}
  tarball="$DIST/tzim-$target.tar.gz"

  if [ ! -f "$tarball" ]; then
    echo "linux-packages: no $tarball, skipping $deb_arch"
    continue
  fi

  pkg="$workdir/pkg-$deb_arch"
  mkdir -p "$pkg"
  tar -xzf "$tarball" -C "$pkg"
  bin=$(find "$pkg" -type f -name tzim | head -n 1)
  if [ -z "$bin" ]; then
    echo "linux-packages: no tzim binary inside $tarball" >&2
    exit 1
  fi
  chmod +x "$bin"

  # The archive ships man/ and completions/. Older archives do not; fall back to
  # the checkout the script is run from, so the packages never lose the
  # documentation because a tarball predates it.
  man="$pkg/man/tzim.1"
  [ -f "$man" ] || man="man/tzim.1"
  compl="$pkg/completions"
  [ -d "$compl" ] || compl="completions"
  for f in "$man" "$compl/tzim.bash" "$compl/_tzim" "$compl/tzim.fish" LICENSE; do
    if [ ! -f "$f" ]; then
      echo "linux-packages: missing $f — the packages would be incomplete" >&2
      exit 1
    fi
  done

  emit_yaml() {
    arch="$1"
    out="$2"
    {
      echo "name: tzim"
      echo "arch: $arch"
      echo "platform: linux"
      echo "version: $VERSION"
      echo "release: \"1\""
      echo "section: utils"
      echo "priority: optional"
      echo "maintainer: RUdolf1517 <rudolfzinovev@protonmail.com>"
      echo "description: |"
      echo "  Lossless file optimizer: shrink the file, never the information."
      echo "  PNG, APNG, gzip, the ZIP family, ICO/CUR, baseline JPEG, TIFF and"
      echo "  zlib streams. Every candidate is verified against a per-format"
      echo "  contract before it replaces anything."
      echo "homepage: https://github.com/RUdolf1517/tzim"
      echo "license: MIT"
      echo "contents:"
      echo "  - src: $bin"
      echo "    dst: /usr/bin/tzim"
      echo "    file_info:"
      echo "      mode: 0755"
      echo "  - src: $man"
      echo "    dst: /usr/share/man/man1/tzim.1"
      echo "    file_info:"
      echo "      mode: 0644"
      echo "  - src: $compl/tzim.bash"
      echo "    dst: /usr/share/bash-completion/completions/tzim"
      echo "    file_info:"
      echo "      mode: 0644"
      echo "  - src: $compl/_tzim"
      echo "    dst: /usr/share/zsh/site-functions/_tzim"
      echo "    file_info:"
      echo "      mode: 0644"
      echo "  - src: $compl/tzim.fish"
      echo "    dst: /usr/share/fish/vendor_completions.d/tzim.fish"
      echo "    file_info:"
      echo "      mode: 0644"
      echo "  - src: LICENSE"
      echo "    dst: /usr/share/doc/tzim/copyright"
      echo "    file_info:"
      echo "      mode: 0644"
    } >"$out"
  }

  # nfpm wants the Debian spelling for one packager and the RPM spelling for the
  # other; asking it to translate is one more table to trust.
  emit_yaml "$deb_arch" "$workdir/nfpm-deb-$deb_arch.yaml"
  emit_yaml "$rpm_arch" "$workdir/nfpm-rpm-$rpm_arch.yaml"

  # nfpm writes the file name itself when --target is a directory; naming each
  # output explicitly keeps the release assets predictable.
  "$nfpm" pkg --packager deb --config "$workdir/nfpm-deb-$deb_arch.yaml" \
    --target "$DIST/tzim_${deb_arch}.deb"
  "$nfpm" pkg --packager rpm --config "$workdir/nfpm-rpm-$rpm_arch.yaml" \
    --target "$DIST/tzim.${rpm_arch}.rpm"
  built=$((built + 1))
done

if [ "$built" -eq 0 ]; then
  echo "linux-packages: no Linux tarball in $DIST, skip"
  exit 0
fi

ls -l "$DIST"/tzim_*.deb "$DIST"/tzim.*.rpm
