#!/usr/bin/env bash
# Reproducible #107 experiment: compare the original float formatter with the
# production fixed-point formatter from this exact checkout, using identical
# Rust/Cargo settings on the same runner.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
TARGET=x86_64-unknown-linux-musl
OUT="${1:-${RUNNER_TEMP:-/tmp}/tzim-progress-ab}"
BIN="$ROOT/target/$TARGET/release/tzim"
mkdir -p "$OUT"

git rev-parse HEAD > "$OUT/source-sha.txt"
rustc -Vv > "$OUT/rustc.txt"
cargo -V > "$OUT/cargo.txt"
rustup target list --installed > "$OUT/rust-targets.txt"
uname -a > "$OUT/runner.txt"
musl-gcc --version > "$OUT/musl-gcc.txt"
size --version > "$OUT/size.txt"
cargo tree --locked --edges normal > "$OUT/cargo-tree.txt"
cargo tree --locked --no-default-features --edges normal > "$OUT/cargo-tree-minimal.txt"
sha256sum Cargo.lock .github/scripts/measure-progress-ab.sh .github/workflows/ci.yml \
  > "$OUT/build-inputs.sha256"

build_one() {
  local name=$1
  local mode=$2
  if [ "$mode" = default ]; then
    cargo build --release --locked --target "$TARGET" > "$OUT/$name-build.log" 2>&1
  else
    cargo build --release --locked --no-default-features --target "$TARGET" \
      > "$OUT/$name-build.log" 2>&1
  fi
  cp "$BIN" "$OUT/$name"
  stat -c '%s' "$OUT/$name" > "$OUT/$name.bytes"
  sha256sum "$OUT/$name" > "$OUT/$name.sha256"
  size -A "$OUT/$name" > "$OUT/$name.sections.txt"
}

# Preserve the production candidate. The baseline below is derived from this
# same source SHA by changing only the rate-formatting call and removing the
# helper, so unrelated source and dependencies are identical on both sides.
cp src/progress.rs "$OUT/candidate-progress.rs"
restore_candidate() {
  cp "$OUT/candidate-progress.rs" src/progress.rs
}
trap restore_candidate EXIT

python3 - <<'PY'
from pathlib import Path

p = Path("src/progress.rs")
s = p.read_text()
helper_start = "// Format the rate without pulling in the general-purpose float formatter."
helper_end = "/// Everything one frame of the line is built from."
if s.count(helper_start) != 1 or s.count(helper_end) != 1:
    raise SystemExit("could not find the unique #107 formatter implementation")
start = s.index(helper_start)
end = s.index(helper_end, start)
s = s[:start] + s[end:]

candidate = "self.render_with_rate_formatter(width, format_rate_fixed_tenths)"
baseline = 'self.render_with_rate_formatter(width, |rate| format!("  {rate:.1}/s"))'
if s.count(candidate) != 1:
    raise SystemExit("could not find the unique production rate-formatting call")
s = s.replace(candidate, baseline, 1)
p.write_text(s)
PY

build_one baseline-default default
build_one baseline-minimal minimal
restore_candidate

# Differential tests establish byte-for-byte formatter and frame equivalence;
# the ordinary/all-feature suites gate the production candidate as well.
cargo test --locked --lib progress::tests::fixed_rate_formatter_matches_float_formatting_at_rounding_boundaries \
  > "$OUT/candidate-rate-rounding-test.log" 2>&1
cargo test --locked --lib progress::tests::fixed_rate_formatter_keeps_full_frame_bytes_across_layouts \
  > "$OUT/candidate-frame-test.log" 2>&1
cargo test --locked > "$OUT/candidate-default-tests.log" 2>&1
cargo test --locked --all-features > "$OUT/candidate-all-features-tests.log" 2>&1

build_one candidate-default default
build_one candidate-minimal minimal

python3 - "$OUT" <<'PY'
import pathlib
import sys

out = pathlib.Path(sys.argv[1])

def size(name):
    return int((out / f"{name}.bytes").read_text().strip())

def sha(name):
    return (out / f"{name}.sha256").read_text().split()[0]

rows = []
for label, baseline, candidate in [
    ("default features", "baseline-default", "candidate-default"),
    ("no default features", "baseline-minimal", "candidate-minimal"),
]:
    old, new = size(baseline), size(candidate)
    delta = new - old
    rows.append(
        f"| {label} | {old:,} | {new:,} | {delta:+,} | "
        f"`{sha(baseline)}` | `{sha(candidate)}` |"
    )

default_smaller = size("candidate-default") < size("baseline-default")
summary = [
    "# Progress formatter A/B (#107)",
    "",
    f"Source SHA: `{(out / 'source-sha.txt').read_text().strip()}`.",
    "Same checkout, Rust toolchain, Cargo.lock, release profile and x86_64-unknown-linux-musl target.",
    "Baseline is this source with only the rate formatting restored to `format!(\"{r:.1}\")`; candidate is the production fixed-point formatter.",
    "Differential rounding/frame tests and candidate default/all-feature tests run before the size comparison.",
    "",
    "| Build | Baseline bytes | Candidate bytes | Delta (candidate − baseline) | Baseline SHA-256 | Candidate SHA-256 |",
    "| --- | ---: | ---: | ---: | --- | --- |",
    *rows,
    "",
    f"Default release-size gate: {'PASS' if default_smaller else 'FAIL'} (candidate must be smaller).",
    "Section reports, binaries, build logs, differential tests, source SHA, toolchain, dependency trees and input hashes are retained as artifacts.",
]
(out / "summary.md").write_text("\n".join(summary) + "\n")
print("\n".join(summary))
if not default_smaller:
    raise SystemExit("the production rate formatter must reduce the full default release binary")
PY

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  cat "$OUT/summary.md" >> "$GITHUB_STEP_SUMMARY"
fi
