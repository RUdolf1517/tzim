#!/usr/bin/env bash
# Reproducible #107 experiment: baseline and an isolated test-only formatter
# candidate are built from this exact checkout with identical Rust/Cargo flags.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
TARGET=x86_64-unknown-linux-musl
OUT="${1:-${RUNNER_TEMP:-/tmp}/tzim-progress-ab}"
BIN="$ROOT/target/$TARGET/release/tzim"
mkdir -p "$OUT"

rustc -Vv > "$OUT/rustc.txt"
cargo -V > "$OUT/cargo.txt"
rustup target list --installed > "$OUT/rust-targets.txt"
uname -a > "$OUT/runner.txt"
musl-gcc --version > "$OUT/musl-gcc.txt"
size --version > "$OUT/size.txt"
cargo tree --locked --edges normal > "$OUT/cargo-tree.txt"
cargo tree --locked --no-default-features --edges normal > "$OUT/cargo-tree-minimal.txt"

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

build_one baseline-default default
build_one baseline-minimal minimal

python3 - <<'PY'
from pathlib import Path
p = Path("src/progress.rs")
s = p.read_text()
marker = "#[cfg(test)]\nfn format_rate_fixed_candidate(rate: f64) -> String {"
replacement = "fn format_rate_fixed_candidate(rate: f64) -> String {"
if s.count(marker) != 1:
    raise SystemExit("could not find the unique test-only #107 candidate formatter")
s = s.replace(marker, replacement, 1)
old = 'let rate_text = rate(&self.recent).map(|r| format!("  {r:.1}/s"));'
new = "let rate_text = rate(&self.recent).map(format_rate_fixed_candidate);"
if s.count(old) != 1:
    raise SystemExit("could not find the unique production rate-formatting call")
s = s.replace(old, new, 1)
p.write_text(s)
PY

# Exact differential checks first, then the ordinary and all-feature suites on
# the candidate checkout. The normal CI matrix independently tests the baseline.
cargo test --locked --lib progress::tests::fixed_rate_candidate_matches_float_formatting_at_rounding_boundaries \
  > "$OUT/candidate-rate-rounding-test.log" 2>&1
cargo test --locked --lib progress::tests::fixed_rate_candidate_keeps_full_frame_bytes_across_layouts \
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

summary = [
    "# Progress formatter A/B (#107)",
    "",
    "Same checkout SHA, Rust toolchain, Cargo.lock, release profile and x86_64-unknown-linux-musl target.",
    "The candidate changes only the displayed rate formatter; `rate` and ETA calculations are unchanged.",
    "",
    "| Build | Baseline bytes | Candidate bytes | Delta (candidate − baseline) | Baseline SHA-256 | Candidate SHA-256 |",
    "| --- | ---: | ---: | ---: | --- | --- |",
    *rows,
    "",
    "Section reports are retained as `*-sections.txt`; the dependency tree and toolchain are retained too.",
    "The candidate is adopted only if the differential tests pass and the full default release binary shrinks.",
]
(out / "summary.md").write_text("\n".join(summary) + "\n")
print("\n".join(summary))
PY

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  cat "$OUT/summary.md" >> "$GITHUB_STEP_SUMMARY"
fi
