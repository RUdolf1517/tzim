#!/usr/bin/env bash
# Regression tests for install.sh. All network, archive and cargo operations are
# stubbed so this suite is deterministic and never downloads or installs a build.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
INSTALLER="$ROOT/install.sh"
WORK="$(mktemp -d)"
FAKE_BIN="$WORK/fake-bin"
mkdir -p "$FAKE_BIN"
trap 'rm -rf "$WORK"' EXIT

fail() {
  printf 'test-install: %s\n' "$*" >&2
  exit 1
}

assert_contains() {
  grep -Fq -- "$2" "$1" || fail "expected '$2' in $1"
}

cat > "$FAKE_BIN/uname" <<'SH'
#!/bin/sh
case "$1" in
  -s) echo Linux ;;
  -m) echo x86_64 ;;
  *) exit 2 ;;
esac
SH

cat > "$FAKE_BIN/curl" <<'SH'
#!/bin/sh
set -eu
out=
url=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) out=$2; shift 2 ;;
    --write-out) shift 2 ;;
    -*) shift ;;
    *) url=$1; shift ;;
  esac
done
case "${FAKE_CURL_MODE:-success}:$url" in
  network:*) exit 7 ;;
  *:*/SHA256SUMS)
    if [ "${FAKE_CURL_MODE:-success}" = missing-sums ]; then
      printf '404'
      exit 0
    fi
    printf '%s  ./tzim-x86_64-unknown-linux-musl.tar.gz\n' \
      "${FAKE_EXPECTED_HASH:-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef}" > "$out"
    printf '200'
    ;;
  asset-404:*|mirror-404:*) printf '404' ;;
  *) printf 'fake archive bytes\n' > "$out"; printf '200' ;;
esac
SH

cat > "$FAKE_BIN/sha256sum" <<'SH'
#!/bin/sh
case "${FAKE_SHA_MODE:-match}" in
  mismatch) printf '%s  %s\n' ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff "$1" ;;
  *) printf '%s  %s\n' "${FAKE_EXPECTED_HASH:-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef}" "$1" ;;
esac
SH

cat > "$FAKE_BIN/tar" <<'SH'
#!/bin/sh
set -eu
if [ "${FAKE_TAR_MODE:-success}" = fail ]; then
  exit 2
fi
root=
while [ "$#" -gt 0 ]; do
  if [ "$1" = -C ]; then root=$2; shift 2; else shift; fi
done
[ -n "$root" ] || exit 2
if [ "${FAKE_TAR_MODE:-success}" != empty ]; then
  cat > "$root/tzim" <<'BIN'
#!/bin/sh
if [ "${1:-}" = --version ]; then
  printf 'tzim %s\n' "${FAKE_BINARY_VERSION:-0.16.0}"
  exit 0
fi
exit 0
BIN
  chmod +x "$root/tzim"
fi
SH

cat > "$FAKE_BIN/cargo" <<'SH'
#!/bin/sh
set -eu
printf '%s\n' "$@" > "$CARGO_ARGS_FILE"
if [ "${FAKE_CARGO_MODE:-success}" = fail ]; then
  exit 1
fi
root=
tag=
branch=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --root) root=$2; shift 2 ;;
    --tag) tag=$2; shift 2 ;;
    --branch) branch=$2; shift 2 ;;
    *) shift ;;
  esac
done
[ -n "$root" ] || exit 2
mkdir -p "$root/bin"
version=${FAKE_CARGO_VERSION:-0.16.0}
cat > "$root/bin/tzim" <<BIN
#!/bin/sh
if [ "\${1:-}" = --version ]; then
  printf 'tzim %s\\n' '$version'
  exit 0
fi
exit 0
BIN
chmod +x "$root/bin/tzim"
SH

chmod +x "$FAKE_BIN"/*

run_installer() {
  local name=$1
  shift
  local case_dir="$WORK/$name"
  mkdir -p "$case_dir/home"
  local log="$case_dir/install.log"
  local args_file="$case_dir/cargo-args.txt"
  rm -f "$args_file"
  if env \
    PATH="$FAKE_BIN:$PATH" \
    HOME="$case_dir/home" \
    TZIM_REPO=owner/project \
    TZIM_INSTALL_DIR="$case_dir/prefix" \
    TZIM_VERSION=0.16.0 \
    FAKE_CURL_MODE=success \
    FAKE_SHA_MODE=match \
    FAKE_TAR_MODE=success \
    FAKE_BINARY_VERSION=0.16.0 \
    FAKE_CARGO_MODE=success \
    FAKE_CARGO_VERSION=0.16.0 \
    CARGO_ARGS_FILE="$args_file" \
    "$@" sh "$INSTALLER" > "$log" 2>&1; then
    RUN_STATUS=0
  else
    RUN_STATUS=$?
  fi
  RUN_DIR=$case_dir
  RUN_LOG=$log
  RUN_CARGO_ARGS=$args_file
}

assert_success() {
  [ "$RUN_STATUS" -eq 0 ] || { cat "$RUN_LOG" >&2; fail "$1 should succeed (status $RUN_STATUS)"; }
}

assert_failure() {
  local name=$1
  local cargo_expected=${2:-no}
  [ "$RUN_STATUS" -ne 0 ] || { cat "$RUN_LOG" >&2; fail "$name should fail"; }
  [ ! -e "$RUN_DIR/prefix/tzim" ] || fail "$name installed a binary after failing"
  if [ "$cargo_expected" = no ]; then
    [ ! -e "$RUN_CARGO_ARGS" ] || fail "$name unexpectedly fell back to cargo"
  else
    [ -e "$RUN_CARGO_ARGS" ] || fail "$name did not exercise the expected cargo fallback"
  fi
}

# A release binary is installed only after its exact requested version is run
# and checked.
run_installer binary-success
assert_success binary-success
assert_contains "$RUN_LOG" 'installed '
assert_contains "$RUN_LOG" '(tzim 0.16.0)'
[ -x "$RUN_DIR/prefix/tzim" ] || fail 'release binary was not installed'
[ ! -e "$RUN_CARGO_ARGS" ] || fail 'successful binary install invoked cargo'

# A failed binary version check is fatal and cannot be disguised as an absent
# asset or a source fallback.
run_installer binary-wrong-version FAKE_BINARY_VERSION=0.15.0
assert_failure binary-wrong-version
assert_contains "$RUN_LOG" 'requested v0.16.0'

# An HTTP 404 for the GitHub asset is the one release-download failure that
# permits fallback. A pin must use the matching Git tag, never the branch tip.
run_installer pinned-tag-fallback FAKE_CURL_MODE=asset-404
assert_success pinned-tag-fallback
assert_contains "$RUN_LOG" 'building from source'
assert_contains "$RUN_CARGO_ARGS" '--tag'
assert_contains "$RUN_CARGO_ARGS" 'v0.16.0'
! grep -Fxq -- '--branch' "$RUN_CARGO_ARGS" || fail 'pinned fallback selected a branch'
assert_contains "$RUN_LOG" '(tzim 0.16.0)'

# Without TZIM_VERSION, the documented branch setting is still honored and
# the source-built binary must successfully answer --version.
run_installer unpinned-branch-fallback FAKE_CURL_MODE=asset-404 TZIM_VERSION= TZIM_BRANCH=stable
assert_success unpinned-branch-fallback
assert_contains "$RUN_CARGO_ARGS" '--branch'
assert_contains "$RUN_CARGO_ARGS" 'stable'
! grep -Fxq -- '--tag' "$RUN_CARGO_ARGS" || fail 'unpinned fallback selected a release tag'

# A tag build that reports a different package version is rejected before the
# installed path can be reported as successful.
run_installer source-wrong-version FAKE_CURL_MODE=asset-404 FAKE_CARGO_VERSION=0.15.0
assert_failure source-wrong-version yes
assert_contains "$RUN_LOG" 'requested v0.16.0'
assert_contains "$RUN_CARGO_ARGS" 'v0.16.0'

# A mirror miss (including a non-working mirror), transport error, absent
# checksums, hash mismatch, corrupt archive and missing executable are all fatal.
run_installer mirror-404 FAKE_CURL_MODE=mirror-404 TZIM_BASE_URL=https://mirror.invalid/tzim
assert_failure mirror-404
assert_contains "$RUN_LOG" 'refusing source fallback'

run_installer network-failure FAKE_CURL_MODE=network
assert_failure network-failure
assert_contains "$RUN_LOG" 'refusing source fallback'

run_installer missing-checksums FAKE_CURL_MODE=missing-sums
assert_failure missing-checksums
assert_contains "$RUN_LOG" 'cannot download SHA256SUMS'

run_installer hash-mismatch FAKE_SHA_MODE=mismatch
assert_failure hash-mismatch
assert_contains "$RUN_LOG" 'SHA-256 mismatch'

run_installer corrupt-archive FAKE_TAR_MODE=fail
assert_failure corrupt-archive
assert_contains "$RUN_LOG" 'cannot unpack'

run_installer missing-executable FAKE_TAR_MODE=empty
assert_failure missing-executable
assert_contains "$RUN_LOG" 'did not contain a tzim executable'

printf 'install.sh regression tests passed\n'
