#!/bin/sh
# tzim — one-command install:
#   curl -fsSL https://raw.githubusercontent.com/RUdolf1517/tzim/main/install.sh | sh
#
# Takes the prebuilt binary from the latest GitHub release when one exists for
# this platform, and falls back to building from source with cargo only when no
# release asset is published. Download, integrity, unpacking and version errors
# are fatal; they never masquerade as an absent platform asset.
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
#   TZIM_BRANCH      source branch when TZIM_VERSION is unset (default main)
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

VERSION=""
TAG=""
if [ -n "${TZIM_VERSION:-}" ]; then
  VERSION=${TZIM_VERSION#v}
  case "$VERSION" in
    ''|*[!A-Za-z0-9.+_-]*) err "invalid TZIM_VERSION '$TZIM_VERSION'" ;;
  esac
  TAG="v$VERSION"
fi

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

tmp=$(mktemp -d) || err "cannot create a temporary directory"
trap 'rm -rf "$tmp"' EXIT

# Fetch one file and distinguish HTTP 404 from transport/server errors. Return
# 0 for a successful download, 4 only for HTTP 404, and 1 for every other error.
# The caller may interpret a 404 for a GitHub release asset as an unavailable
# platform asset; a 404 for checksums or a mirror is always fatal.
fetch() {
  url=$1
  dest=$2
  FETCH_HTTP_STATUS=""
  if need curl; then
    if FETCH_HTTP_STATUS=$(curl -sSL --output "$dest" --write-out '%{http_code}' "$url"); then
      case "$FETCH_HTTP_STATUS" in
        2[0-9][0-9]) return 0 ;;
        404) rm -f "$dest"; return 4 ;;
        *) rm -f "$dest"; return 1 ;;
      esac
    fi
    rm -f "$dest"
    return 1
  fi
  if need wget; then
    status_file="$tmp/wget-response-$$.txt"
    if wget --server-response -O "$dest" "$url" 2>"$status_file"; then
      FETCH_HTTP_STATUS=$(awk '$1 ~ /^HTTP\/[0-9.]+$/ { code = $2 } END { print code }' "$status_file")
      return 0
    fi
    FETCH_HTTP_STATUS=$(awk '$1 ~ /^HTTP\/[0-9.]+$/ { code = $2 } END { print code }' "$status_file")
    rm -f "$dest"
    [ "$FETCH_HTTP_STATUS" = 404 ] && return 4
    return 1
  fi
  return 1
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

# Run and validate a candidate before installation, and validate the installed
# path again after the move. This applies to release binaries and cargo builds.
check_version() {
  CHECKED_VERSION=$("$1" --version) || err "the candidate binary does not run: $1"
  case "$CHECKED_VERSION" in
    "tzim "*) ;;
    *) err "the candidate returned an unexpected --version value: '$CHECKED_VERSION'" ;;
  esac
  if [ -n "$VERSION" ] && [ "$CHECKED_VERSION" != "tzim $VERSION" ]; then
    err "requested $TAG but the candidate reports '$CHECKED_VERSION'"
  fi
}

install_binary() {
  asset="tzim-${target}.${ext}"
  if [ -n "${TZIM_BASE_URL:-}" ]; then
    base="${TZIM_BASE_URL%/}"
  elif [ -n "$TAG" ]; then
    base="https://github.com/${REPO}/releases/download/${TAG}"
  else
    base="https://github.com/${REPO}/releases/latest/download"
  fi

  if fetch "$base/$asset" "$tmp/asset"; then
    :
  else
    fetch_status=$?
    if [ "$fetch_status" -eq 4 ] && [ -z "${TZIM_BASE_URL:-}" ]; then
      return 4
    fi
    err "cannot download $asset from $base (HTTP ${FETCH_HTTP_STATUS:-unknown}); refusing source fallback"
  fi

  if [ "${TZIM_INSECURE:-0}" = 1 ]; then
    say "tzim-install: WARNING — checksum verification skipped (TZIM_INSECURE=1)"
  else
    if fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS"; then
      :
    else
      err "cannot download SHA256SUMS from $base (HTTP ${FETCH_HTTP_STATUS:-unknown})
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

  if ! tar -C "$tmp" -xzf "$tmp/asset"; then
    err "cannot unpack $asset; refusing source fallback"
  fi
  bin=$(find "$tmp" -type f -name tzim -print | sed -n '1p')
  [ -n "$bin" ] || err "$asset did not contain a tzim executable; refusing source fallback"
  chmod +x "$bin" || err "cannot make the downloaded binary executable"
  check_version "$bin"

  mkdir -p "$INSTALL_DIR" || err "cannot create install directory $INSTALL_DIR"
  mv "$bin" "$INSTALL_DIR/tzim" || err "cannot install the verified binary at $INSTALL_DIR/tzim"
  check_version "$INSTALL_DIR/tzim"
  say "installed $INSTALL_DIR/tzim ($CHECKED_VERSION)"
}

install_from_source() {
  need cargo || err "no published release asset for $os/$arch, and cargo is not available.
Get Rust: https://rustup.rs — or take an archive from https://github.com/${REPO}/releases"
  source_root="$tmp/source-root"
  say "building from source (cargo)…"
  if [ -n "$TAG" ]; then
    if ! cargo install --git "https://github.com/${REPO}" --tag "$TAG" --force --root "$source_root" tzim; then
      err "cargo could not build pinned tag $TAG"
    fi
  else
    if ! cargo install --git "https://github.com/${REPO}" --branch "$BRANCH" --force --root "$source_root" tzim; then
      err "cargo could not build source branch $BRANCH"
    fi
  fi
  bin="$source_root/bin/tzim"
  [ -f "$bin" ] || err "cargo did not install a tzim executable"
  check_version "$bin"

  mkdir -p "$INSTALL_DIR" || err "cannot create install directory $INSTALL_DIR"
  mv "$bin" "$INSTALL_DIR/tzim" || err "cannot install the source build at $INSTALL_DIR/tzim"
  check_version "$INSTALL_DIR/tzim"
  say "installed $INSTALL_DIR/tzim ($CHECKED_VERSION)"
}

finish_path_hint() {
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) say "add to PATH: export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
  esac
}

if [ -n "$target" ]; then
  if install_binary; then
    finish_path_hint
    exit 0
  else
    install_status=$?
    if [ "$install_status" -ne 4 ]; then
      err "release asset installation failed; refusing source fallback"
    fi
    say "no published release asset for $os/$arch at the selected release; will build the matching source"
  fi
else
  say "no prebuilt binary target for $os/$arch; will build from source"
fi

install_from_source
finish_path_hint
