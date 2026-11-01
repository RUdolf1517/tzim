#!/usr/bin/env bash
# Reproducible #105 before/after measurement for the STORE-only ZIP early return.
# Builds the checked-in issue baseline and current candidate with the same pinned
# Rust/Cargo environment, then alternates dry-run measurements on identical data.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
BASELINE_SHA="${TZIM_ZIP_BASELINE_SHA:-f1e0acab9cc95f4ed32721a79ea4fd96f4a71a00}"
OUT="${1:-${RUNNER_TEMP:-/tmp}/tzim-zip-store-ab}"
WORKTREE="$OUT/baseline-source"
TARGET_DIR="${CARGO_TARGET_DIR:-${RUNNER_TEMP:-/tmp}/tzim-zip-store-target}"
ZIP="$OUT/stored-512MiB.zip"

mkdir -p "$OUT"
[ ! -e "$WORKTREE" ] || { echo "baseline worktree path already exists: $WORKTREE" >&2; exit 2; }
cleanup() {
  git -C "$ROOT" worktree remove --force "$WORKTREE" >/dev/null 2>&1 || true
}
trap cleanup EXIT

rustc -Vv > "$OUT/rustc.txt"
cargo -V > "$OUT/cargo.txt"
uname -a > "$OUT/runner.txt"
git rev-parse HEAD > "$OUT/candidate-head.txt"
printf '%s\n' "$BASELINE_SHA" > "$OUT/baseline-head.txt"
git status --short > "$OUT/candidate-status.txt"
printf '%s\n' 'tzim --dry-run --no-progress --json --jobs 1 --formats zip INPUT' > "$OUT/command.txt"
git worktree add --detach "$WORKTREE" "$BASELINE_SHA" > "$OUT/worktree.log" 2>&1
cmp "$WORKTREE/Cargo.lock" "$ROOT/Cargo.lock"

(
  cd "$WORKTREE"
  CARGO_TARGET_DIR="$TARGET_DIR/baseline" cargo build --release --locked
) > "$OUT/baseline-build.log" 2>&1
cp "$TARGET_DIR/baseline/release/tzim" "$OUT/tzim-baseline"
CARGO_TARGET_DIR="$TARGET_DIR/candidate" cargo build --release --locked \
  > "$OUT/candidate-build.log" 2>&1
cp "$TARGET_DIR/candidate/release/tzim" "$OUT/tzim-candidate"
sha256sum "$OUT/tzim-baseline" "$OUT/tzim-candidate" > "$OUT/binaries.sha256"

python3 - "$ZIP" <<'PY'
import sys
import zipfile

path = sys.argv[1]
block = bytes(range(256)) * 4096  # 1 MiB, incompressible as a STORE entry
with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_STORED, allowZip64=False) as archive:
    with archive.open("payload.bin", "w", force_zip64=False) as entry:
        for _ in range(512):
            entry.write(block)
PY

cp "$ZIP" "$OUT/baseline-input.zip"
cp "$ZIP" "$OUT/candidate-input.zip"
sha256sum "$ZIP" "$OUT/baseline-input.zip" "$OUT/candidate-input.zip" > "$OUT/inputs.sha256"
stat -c '%n %s bytes' "$ZIP" > "$OUT/input-size.txt"

run_one() {
  local variant=$1
  local repetition=$2
  local binary="$OUT/tzim-$variant"
  local input="$OUT/$variant-input.zip"
  /usr/bin/time -v -o "$OUT/$variant-$repetition.time" \
    "$binary" --dry-run --no-progress --json --jobs 1 --formats zip "$input" \
    > "$OUT/$variant-$repetition.json" 2> "$OUT/$variant-$repetition.stderr"
}

for repetition in 1 2 3 4 5; do
  if (( repetition % 2 == 1 )); then
    run_one baseline "$repetition"
    run_one candidate "$repetition"
  else
    run_one candidate "$repetition"
    run_one baseline "$repetition"
  fi
done

python3 - "$OUT" <<'PY'
import json
import pathlib
import re
import statistics
import subprocess
import sys

out = pathlib.Path(sys.argv[1])
input_size = int((out / "input-size.txt").read_text().split()[-2])

def parse_time(path):
    text = path.read_text()
    wall = re.search(r"Elapsed \(wall clock\) time .*: (.+)", text)
    user = re.search(r"User time \(seconds\): ([0-9.]+)", text)
    system = re.search(r"System time \(seconds\): ([0-9.]+)", text)
    rss = re.search(r"Maximum resident set size \(kbytes\): ([0-9]+)", text)
    if not all((wall, user, system, rss)):
        raise SystemExit(f"missing GNU time fields in {path}")
    pieces = [float(piece) for piece in wall.group(1).split(":")]
    seconds = sum(piece * (60 ** (len(pieces) - index - 1)) for index, piece in enumerate(pieces))
    return wall.group(1), seconds, float(user.group(1)), float(system.group(1)), int(rss.group(1))

def parse_record(path):
    lines = [json.loads(line) for line in path.read_text().splitlines()]
    records = [line for line in lines if line.get("type") == "file"]
    if len(records) != 1:
        raise SystemExit(f"expected one file JSON record in {path}")
    record = records[0]
    expected = {
        "format": "zip",
        "status": "already_optimal",
        "reason_code": "no_smaller_candidate",
        "before": input_size,
        "after": input_size,
        "saved": 0,
        "guarantee": "byte_identical",
    }
    for key, value in expected.items():
        if record.get(key) != value:
            raise SystemExit(f"{path}: {key}={record.get(key)!r}, expected {value!r}")
    return record

summary_rows = []
records = {}
for variant in ("baseline", "candidate"):
    samples = []
    for repetition in range(1, 6):
        records[(variant, repetition)] = parse_record(out / f"{variant}-{repetition}.json")
        samples.append(parse_time(out / f"{variant}-{repetition}.time"))
    summary_rows.append(
        f"| {variant} | {statistics.median(sample[1] for sample in samples):.3f} | "
        f"{statistics.median(sample[2] for sample in samples):.3f} | "
        f"{statistics.median(sample[3] for sample in samples):.3f} | "
        f"{statistics.median(sample[4] for sample in samples):,} | "
        + ", ".join(sample[0] for sample in samples)
        + " |"
    )

for repetition in range(1, 6):
    left = records[("baseline", repetition)]
    right = records[("candidate", repetition)]
    for key in ("format", "status", "reason_code", "before", "after", "saved", "guarantee"):
        if left.get(key) != right.get(key):
            raise SystemExit(f"run {repetition}: baseline/candidate JSON differs for {key}")

hashes = {}
for line in (out / "inputs.sha256").read_text().splitlines():
    digest, path = line.split(maxsplit=1)
    hashes[path.strip()] = digest
for name in ("stored-512MiB.zip", "baseline-input.zip", "candidate-input.zip"):
    path = out / name
    digest = subprocess.run(["sha256sum", str(path)], check=True, capture_output=True, text=True).stdout.split()[0]
    if digest != hashes[str(path)]:
        raise SystemExit(f"dry-run changed input {path}")
if len({hashes[str(out / name)] for name in ("stored-512MiB.zip", "baseline-input.zip", "candidate-input.zip")}) != 1:
    raise SystemExit("baseline/candidate input copies do not have identical initial hashes")

summary = [
    "# STORE-only ZIP early-return A/B (#105)",
    "",
    f"Baseline source: `{(out / 'baseline-head.txt').read_text().strip()}`.",
    f"Candidate source: `{(out / 'candidate-head.txt').read_text().strip()}`.",
    "Both release binaries use the same pinned Rust toolchain, Cargo.lock, runner, input bytes, and flags.",
    "The immutable input is a 512 MiB STORE-only ZIP; each side is run five times in alternating order.",
    "Every JSON record was checked for the same `already_optimal` status, `no_smaller_candidate` reason, sizes, saved bytes, and guarantee.",
    "",
    f"Input: `{(out / 'input-size.txt').read_text().strip()}`.",
    f"SHA-256: `{hashes[str(out / 'stored-512MiB.zip')]}` (baseline/candidate copies are separately recorded).",
    "",
    "| Build | Median wall seconds | Median user seconds | Median system seconds | Median max RSS (KiB) | Wall-clock samples (GNU time) |",
    "| --- | ---: | ---: | ---: | ---: | --- |",
    *summary_rows,
    "",
    "Raw build logs, JSON output, stderr, all five GNU `time -v` reports, toolchain, source SHAs and input hashes are retained alongside this summary.",
    "",
]
(out / "summary.md").write_text("\n".join(summary))
print("\n".join(summary))
PY

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  cat "$OUT/summary.md" >> "$GITHUB_STEP_SUMMARY"
fi
