#!/bin/sh
# tzim — one-command install:
#   curl -fsSL https://raw.githubusercontent.com/RUdolf1517/tzim/main/install.sh | sh
#
# Takes the prebuilt binary from the latest GitHub release when one exists for
# this platform, and falls back to building from source with cargo when it does
# not. Never installs anything else, never touches a file it did not create.
#
# Environment:
#   TZIM_REPO        owner/name (default RUdolf1517/tzim)
#   TZIM_VERSION     pin a release, e.g. 0.4.2 (default: the latest release)
#   TZIM_INSTALL_DIR destination (default ~/.local/bin)
#   TZIM_BRANCH      branch for the cargo fallback (default main)
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
  Linux/x86_64|Linux/amd64)   target=x86_64-unknown-linux-musl; ext=tar.gz ;;
  Darwin/arm64|Darwin/aarch64) target=aarch64-apple-darwin;      ext=tar.gz ;;
  Darwin/x86_64|Darwin/amd64)  target=x86_64-apple-darwin;       ext=tar.gz ;;
  Linux/aarch64|Linux/arm64)
    target=""
    say "no prebuilt binary for $os/$arch — will build from source"
    ;;
  *)
    target=""
    say "no prebuilt binary for $os/$arch — will build from source"
    ;;

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

install_binary() {
  tmp=$(mktemp -d) || err "cannot create a temporary directory"
  trap 'rm -rf "$tmp"' EXIT

  if [ -n "${TZIM_VERSION:-}" ]; then
    url="https://github.com/${REPO}/releases/download/v${TZIM_VERSION}/tzim-${target}.${ext}"
  else
    url="https://github.com/${REPO}/releases/latest/download/tzim-${target}.${ext}"
  fi

  download "$url" "$tmp/asset" 2>/dev/null || return 1

  if [ "$ext" = "tar.gz" ]; then
    tar -C "$tmp" -xzf "$tmp/asset" || return 1
  else
    return 1
  fi

  bin=$(find "$tmp" -type f -name tzim | head -n 1)
  [ -n "$bin" ] || return 1
  chmod +x "$bin"
  mkdir -p "$INSTALL_DIR"
  mv "$bin" "$INSTALL_DIR/tzim"
}

if [ -n "$target" ] && install_binary; then
  say "installed $INSTALL_DIR/tzim"
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) say "add to PATH: export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
  esac
  "$INSTALL_DIR/tzim" --version || true
  exit 0
fi

if need cargo; then
  say "building from source (cargo)…"
  cargo install --git "https://github.com/${REPO}" --branch "$BRANCH" --force
  exit 0
fi

err "no installable build for this platform, and cargo is not available.
Get Rust: https://rustup.rs — or take an archive from https://github.com/${REPO}/releases"
