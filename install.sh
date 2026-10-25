#!/bin/sh
# tzim — one-command install:
#   curl -fsSL https://raw.githubusercontent.com/RUdolf1517/tzim/main/install.sh | sh
#
# Takes the prebuilt binary from the latest GitHub release when one exists for
# this platform, and falls back to building from source with cargo when it does
# not. Never installs anything else, never touches a file it did not create.
#
# Every download is checked against the release's SHA256SUMS before it is
# unpacked. A `curl | sh` installer that does not verify what it downloads is
# asking the user to trust the network path as much as the project, and this
# program spends its whole documentation arguing that trust should be earned by
# a check. A hash that does not match aborts the install; it never falls back to
# a build that would hide the mismatch.
#
# Environment:
#   TZIM_REPO        owner/name (default RUdolf1517/tzim)
#   TZIM_VERSION     pin a release, e.g. 0.11.0 (default: the latest release)
#   TZIM_INSTALL_DIR destination (default ~/.local/bin)
#   TZIM_BRANCH      branch for the cargo fallback (default main)
#   TZIM_BASE_URL    download from a mirror instead of GitHub, e.g.
#                    https://mirror.example/tzim — the layout is
#                    <base>/<asset> and <base>/SHA256SUMS. Mirrored releases are
#                    how air-gapped machines install this; the checksum check is
#                    not optional there either.
#   TZIM_INSECURE=1  install without checking SHA256SUMS — for a mirror that
#                    does not publish checksums. Prints a warning.
set -eu

REPO="${TZIM_REPO:-RUdolf1517/tzim}"
BRANCH="${TZIM_BRANCH:-main}"
INSTALL_DIR="${TZIM_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
err() { printf 'tzim-install: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1; }

os=$(uname -s)
arch=$(uname -m)

# Asset names are the Rust target triples and carry no version, so the URL for
# the latest release is stable. Platforms without a published build fall through
# to cargo rather than to a guessed binary.
case "$os/$arch" in
  Linux/x86_64|Linux/amd64)    target=x86_64-unknown-linux-musl;  ext=tar.gz ;;
  Linux/aarch64|Linux/arm64)   target=aarch64-unknown-linux-musl; ext=tar.gz ;;
  Darwin/arm64|Darwin/aarch64) target=aarch64-apple-darwin;       ext=tar.gz ;;
  Darwin/x86_64|Darwin/amd64)  target=x86_64-apple-darwin;        ext=tar.gz ;;
  *)                           target="" ;;
esac

download() {
  if need curl; then
    curl -fsSL "$1" -o "$2"
  elif need wget; then
    wget -qO "$2" "$1"
  else
    return 1
  fi
}

# One line, because the two implementations disagree about nothing else: the
# hash is lowercase hex in both, and `-c`-style input would need the exact file
# layout of the release rather than one entry.
sha256_of() {
  if need sha256sum; then
    sha256sum "$1" | cut -d' ' -f1
  elif need shasum; then
    shasum -a 256 "$1" | cut -d' ' -f1
  elif need openssl; then
    openssl dgst -sha256 -r "$1" | cut -d' ' -f1
  else
    return 1
  fi
}

# The entry for one asset out of a SHA256SUMS file written by
# `sha256sum ./*`, which spells the path as ./name.
expected_hash() {
  awk -v want="$2" '$2 == want || $2 == "./" want { print $1 }' "$1"
}

install_binary() {
  tmp=$(mktemp -d) || err "cannot create a temporary directory"
  trap 'rm -rf "$tmp"' EXIT

  asset="tzim-${target}.${ext}"
  if [ -n "${TZIM_BASE_URL:-}" ]; then
    base="${TZIM_BASE_URL%/}"
  elif [ -n "${TZIM_VERSION:-}" ]; then
    base="https://github.com/${REPO}/releases/download/v${TZIM_VERSION}"
  else
    base="https://github.com/${REPO}/releases/latest/download"
  fi

  download "$base/$asset" "$tmp/asset" 2>/dev/null || return 1

  if [ "${TZIM_INSECURE:-0}" = 1 ]; then
    say "tzim-install: WARNING — checksum verification skipped (TZIM_INSECURE=1)"
  else
    if ! download "$base/SHA256SUMS" "$tmp/SHA256SUMS" 2>/dev/null; then
      err "cannot download SHA256SUMS from $base
Refusing to install an unverified binary. If you are using a mirror that does
not publish checksums, set TZIM_INSECURE=1 and accept that risk explicitly."
    fi
    want=$(expected_hash "$tmp/SHA256SUMS" "$asset")
    if [ -z "$want" ]; then
      err "$asset is not listed in SHA256SUMS — refusing to install it"
    fi
    if ! have_hash=$(sha256_of "$tmp/asset"); then
      err "no sha256sum, shasum or openssl available to verify the download
Install one of them, or set TZIM_INSECURE=1 to skip the check."
    fi
    if [ "$have_hash" != "$want" ]; then
      err "SHA-256 mismatch for $asset
  expected $want
  got      $have_hash
Do not run the downloaded binary. The download was corrupted or tampered with."
    fi
  fi

  tar -C "$tmp" -xzf "$tmp/asset" || return 1
  bin=$(find "$tmp" -type f -name tzim | head -n 1)
  [ -n "$bin" ] || return 1
  chmod +x "$bin"

  # Installed before it is run, and run before it is reported as installed: a
  # binary that cannot execute on this machine is not a successful install.
  mkdir -p "$INSTALL_DIR"
  mv "$bin" "$INSTALL_DIR/tzim"
  version=$("$INSTALL_DIR/tzim" --version) || err "the installed binary does not run"
  say "installed $INSTALL_DIR/tzim ($version)"
  if [ -n "${TZIM_VERSION:-}" ] && [ "$version" != "tzim ${TZIM_VERSION}" ]; then
    err "requested v$TZIM_VERSION but the archive reports '$version'"
  fi
}

if [ -n "$target" ]; then
  if install_binary; then
    case ":$PATH:" in
      *":$INSTALL_DIR:"*) ;;
      *) say "add to PATH: export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
    esac
    exit 0
  fi
  say "no published archive for $os/$arch in this release — will build from source"
fi

if need cargo; then
  say "building from source (cargo)…"
  cargo install --git "https://github.com/${REPO}" --branch "$BRANCH" --force
  exit 0
fi

err "no installable build for this platform, and cargo is not available.
Get Rust: https://rustup.rs — or take an archive from https://github.com/${REPO}/releases"
