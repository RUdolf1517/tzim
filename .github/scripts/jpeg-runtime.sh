#!/usr/bin/env bash
# Measure the release-default JPEG V1 cost against the pre-#82 feature set.
#
# The baseline is built from this exact checkout with the JPEG coefficient
# verifier but without `jpeg-v1`, which isolates the independent verifier's
# startup, wall-time and peak-RSS cost from source/compiler changes. The input
# list is all pinned-corpus JPEGs no larger than 200 KiB (the existing benchmark
# size class), selected before either run and retained with SHA-256 provenance.
# No corpus files or binaries are committed.
#
# Usage: jpeg-runtime.sh <default-binary> <work-dir>
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
default_bin="$(realpath "${1:?usage: jpeg-runtime.sh <default-binary> <work-dir>}")"
work="$(realpath -m "${2:?usage: jpeg-runtime.sh <default-binary> <work-dir>}")"
source_work="$work/source"
selection="$work/selection"
mkdir -p "$work" "$selection"

if [[ ! -x "$default_bin" ]]; then
    echo "default binary is not executable: $default_bin" >&2
    exit 2
fi

bash "$root/.github/scripts/corpus-fetch.sh" "$source_work" | tee "$work/corpus-fetch.log"

# Record a fixed selection before measuring either binary. corpus-fetch.sh
# flattens upstream paths into provenance-bearing names, so each name is unique.
mkdir -p "$selection/source"
python3 - "$source_work/corpus/images" "$work/selection.tsv" "$selection/source" <<'PY'
import hashlib
import pathlib
import shutil
import sys

images = pathlib.Path(sys.argv[1])
manifest = pathlib.Path(sys.argv[2])
copy_root = pathlib.Path(sys.argv[3])
limit = 200 * 1024
rows = []
for path in sorted(images.iterdir(), key=lambda p: p.name):
    if not path.is_file() or path.suffix.lower() not in {".jpg", ".jpeg"}:
        continue
    size = path.stat().st_size
    if size > limit:
        continue
    name = path.name
    if "\t" in name or "\n" in name:
        raise SystemExit(f"unsafe corpus name: {name!r}")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    rows.append((name, size, digest, path))

if len(rows) < 20:
    raise SystemExit(f"pre-registered JPEG selection has only {len(rows)} files (need at least 20)")

with manifest.open("w", encoding="utf-8", newline="\n") as out:
    out.write("path\tbytes\tsha256\n")
    for name, size, digest, path in rows:
        shutil.copy2(path, copy_root / name)
        out.write(f"{name}\t{size}\t{digest}\n")
print(f"selected {len(rows)} JPEGs, {sum(row[1] for row in rows)} bytes; limit={limit}")
PY

baseline_target="$work/target-pre82"
baseline_build_log="$work/build-pre82.log"
if ! CARGO_TARGET_DIR="$baseline_target" cargo build --release --locked \
    --no-default-features --features 'png,gzip,zip,ico,jpeg,tiff,zlib,zopfli' \
    >"$baseline_build_log" 2>&1; then
    cat "$baseline_build_log" >&2
    exit 1
fi
baseline_bin="$baseline_target/release/tzim"
if [[ ! -x "$baseline_bin" ]]; then
    echo "baseline build did not produce $baseline_bin" >&2
    exit 1
fi

{
    echo "commit=$(git -C "$root" rev-parse HEAD)"
    echo "source_head_sha=${SOURCE_SHA:-unknown}"
    echo "checkout_sha=$(git -C "$root" rev-parse HEAD)"
    echo "worktree_clean=$(test -z "$(git -C "$root" status --porcelain=v1)" && echo yes || echo no)"
    echo "tree_status_sha256=$(git -C "$root" status --porcelain=v1 | sha256sum | cut -d' ' -f1)"
    echo "platform=$(uname -srm)"
    echo "os=$(grep -m1 '^PRETTY_NAME=' /etc/os-release | cut -d= -f2- | tr -d '"')"
    echo "rustc=$(rustc --version)"
    echo "cargo=$(cargo --version)"
    echo "python=$(python3 --version)"
    echo "time=$( /usr/bin/time --version 2>&1 | sed -n '1p' )"
    echo "runtime_script_sha256=$(sha256sum "$root/.github/scripts/jpeg-runtime.sh" | cut -d' ' -f1)"
    echo "workflow_sha256=$(sha256sum "$root/.github/workflows/ci.yml" | cut -d' ' -f1)"
    echo "cargo_lock_sha256=$(sha256sum "$root/Cargo.lock" | cut -d' ' -f1)"
    echo "cargo_manifest_sha256=$(sha256sum "$root/Cargo.toml" | cut -d' ' -f1)"
    echo "corpus_fetch_script_sha256=$(sha256sum "$root/.github/scripts/corpus-fetch.sh" | cut -d' ' -f1)"
    echo "baseline_features=png,gzip,zip,ico,jpeg,tiff,zlib,zopfli (no jpeg-v1; explicit --formats jpeg)"
    echo "default_features=$(grep -m1 '^default = ' "$root/Cargo.toml" | sed 's/^[[:space:]]*//')"
    echo "selection_rule=all .jpg/.jpeg corpus files <= 204800 bytes, sorted by flattened provenance name"
    echo "baseline_binary_bytes=$(stat -c '%s' "$baseline_bin")"
    echo "baseline_binary_sha256=$(sha256sum "$baseline_bin" | cut -d' ' -f1)"
    echo "default_binary_bytes=$(stat -c '%s' "$default_bin")"
    echo "default_binary_sha256=$(sha256sum "$default_bin" | cut -d' ' -f1)"
} > "$work/versions.txt"

# Alternate order to reduce systematic warm-cache/order bias. Each repetition
# starts from a fresh copy of the identical pre-run inputs.
for trial in baseline-1 default-1 default-2 baseline-2 baseline-3 default-3; do
    case "$trial" in
        baseline-*) binary="$baseline_bin"; formats=(--formats jpeg) ;;
        default-*)  binary="$default_bin"; formats=() ;;
        *) echo "unknown trial: $trial" >&2; exit 2 ;;
    esac
    run_dir="$work/runs/$trial"
    mkdir -p "$run_dir"
    cp -a "$selection/source/." "$run_dir/"
    set +e
    /usr/bin/time -v -o "$work/$trial.time.txt" \
        "$binary" -r -j 1 "${formats[@]}" --json "$run_dir" \
        > "$work/$trial.jsonl" 2> "$work/$trial.stderr"
    status=$?
    set -e
    printf '%s\n' "$status" > "$work/$trial.exit"
    python3 - "$run_dir" "$work/$trial-output.tsv" <<'PY'
import hashlib
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
out = pathlib.Path(sys.argv[2])
with out.open("w", encoding="utf-8", newline="\n") as fh:
    fh.write("path\tbytes\tsha256\n")
    for path in sorted((p for p in root.rglob("*") if p.is_file()), key=lambda p: p.relative_to(root).as_posix()):
        data = path.read_bytes()
        rel = path.relative_to(root).as_posix()
        fh.write(f"{rel}\t{len(data)}\t{hashlib.sha256(data).hexdigest()}\n")
PY
done

python3 - "$root" "$work" <<'PY'
import fnmatch
import hashlib
import json
import pathlib
import re
import statistics
import sys

root = pathlib.Path(sys.argv[1])
work = pathlib.Path(sys.argv[2])
selection = work / "selection.tsv"
with selection.open(encoding="utf-8") as fh:
    next(fh)
    selected = [line.rstrip("\n").split("\t") for line in fh]
selected_names = [row[0] for row in selected]
selected_sizes = {row[0]: int(row[1]) for row in selected}
selected_hashes = {row[0]: row[2] for row in selected}

patterns = []
for line in (root / ".github/corpus-allowlist.txt").read_text(encoding="utf-8").splitlines():
    line = line.strip()
    if line and not line.startswith("#"):
        patterns.append(line)

trials = ["baseline-1", "baseline-2", "baseline-3", "default-1", "default-2", "default-3"]
parsed = {}
problems = []
for trial in trials:
    records = []
    summary = None
    for line in (work / f"{trial}.jsonl").read_text(encoding="utf-8").splitlines():
        item = json.loads(line)
        if item.get("type") == "file":
            records.append(item)
        elif item.get("type") == "summary":
            summary = item
    if summary is None:
        problems.append(f"{trial}: missing summary JSON")
        continue
    if len(records) != len(selected_names) or summary.get("files") != len(selected_names):
        problems.append(f"{trial}: returned {len(records)} rows / summary {summary.get('files')} for {len(selected_names)} selected inputs")
    by_name = {}
    for record in records:
        name = pathlib.Path(record.get("path", "")).name
        if name in by_name:
            problems.append(f"{trial}: duplicate output row for {name}")
        by_name[name] = record
        if record.get("format") != "jpeg":
            problems.append(f"{trial}: {name} was classified as {record.get('format')!r}, not JPEG")
        error = record.get("error")
        if error and error.get("kind") == "verify":
            problems.append(f"{trial}: verification failure on {name}: {error.get('message')}")
        if record.get("status") == "failed" and not any(fnmatch.fnmatchcase(name, pattern) for pattern in patterns):
            problems.append(f"{trial}: unallowlisted failed input {name}: {error}")
    missing = set(selected_names) - set(by_name)
    if missing:
        problems.append(f"{trial}: missing rows for {sorted(missing)[:10]}")
    actual_exit = int((work / f"{trial}.exit").read_text().strip())
    failed = summary.get("failed", 0)
    verify_failed = any(r.get("error", {}).get("kind") == "verify" for r in records if r.get("error"))
    expected_exit = 4 if verify_failed else (1 if failed else 0)
    if actual_exit != expected_exit:
        problems.append(f"{trial}: process exit {actual_exit}, expected report exit {expected_exit}")
    parsed[trial] = (records, by_name, summary)

# Check every post-run file against its JSON row and the selected input hash.
# Refused, failed, skipped, and already-optimal inputs must remain byte-identical;
# accepted rewrites must be strictly smaller under the unchanged policy.
output_manifests = {}
for trial in trials:
    manifest = (work / f"{trial}-output.tsv").read_text(encoding="utf-8").splitlines()
    output_rows = [line.split("\t") for line in manifest[1:]]
    if len(output_rows) != len(selected_names):
        problems.append(f"{trial}: output manifest has {len(output_rows)} files, expected {len(selected_names)}")
    by_name = {}
    for row in output_rows:
        if len(row) != 3:
            problems.append(f"{trial}: malformed output-manifest row {row!r}")
            continue
        name, size_text, digest = row
        if name in by_name:
            problems.append(f"{trial}: duplicate output manifest entry for {name}")
        by_name[name] = (int(size_text), digest)
        if name not in selected_hashes:
            problems.append(f"{trial}: unexpected output file {name}")
        record = parsed.get(trial, ([], {}, {}))[1].get(name)
        if record and int(record.get("after", -1)) != int(size_text):
            problems.append(f"{trial}: JSON after-size and output manifest disagree for {name}")
        if record and name in selected_hashes:
            status = record.get("status")
            if status == "optimized":
                if int(size_text) >= selected_sizes[name]:
                    problems.append(f"{trial}: optimized file is not smaller: {name}")
            elif status in {"already_optimal", "skipped", "failed", "unsupported"}:
                if int(size_text) != selected_sizes[name] or digest != selected_hashes[name]:
                    problems.append(f"{trial}: non-optimized/refused input changed: {name} ({status})")
            else:
                problems.append(f"{trial}: unexpected status {status!r} for {name}")
    missing = set(selected_names) - set(by_name)
    if missing:
        problems.append(f"{trial}: output manifest is missing {sorted(missing)[:10]}")
    output_manifests[trial] = by_name

# Verify deterministic output across repetitions, then compare the two policies
# on files where their acceptance status agrees.
for build, reps in (("baseline", ["baseline-1", "baseline-2", "baseline-3"]),
                    ("default", ["default-1", "default-2", "default-3"])):
    for name in selected_names:
        observations = [
            (parsed.get(trial, ([], {}, {}))[1].get(name, {}).get("status"),
             output_manifests.get(trial, {}).get(name))
            for trial in reps
        ]
        if any(observation != observations[0] for observation in observations[1:]):
            problems.append(f"{build}: status or output changed across repetitions for {name}")

# Verify deterministic output for files where both policy paths actually wrote
# a candidate. V1 refusals are listed rather than treated as a transform change.
divergent_status = []
output_mismatch = []
for name in selected_names:
    base = parsed.get("baseline-1", ([], {}, {}))[1].get(name)
    default = parsed.get("default-1", ([], {}, {}))[1].get(name)
    if not base or not default:
        continue
    if base.get("status") != default.get("status"):
        divergent_status.append((name, base.get("status"), default.get("status"), default.get("note")))
    bline = next((line.split("\t") for line in (work / "baseline-1-output.tsv").read_text(encoding="utf-8").splitlines()[1:] if line.split("\t", 1)[0] == name), None)
    dline = next((line.split("\t") for line in (work / "default-1-output.tsv").read_text(encoding="utf-8").splitlines()[1:] if line.split("\t", 1)[0] == name), None)
    if bline and dline and base.get("status") == default.get("status") and bline[2] != dline[2]:
        output_mismatch.append(name)
if output_mismatch:
    problems.append(f"baseline/default produced different bytes for same-status files: {output_mismatch[:10]}")


def time_metrics(trial):
    text = (work / f"{trial}.time.txt").read_text(encoding="utf-8", errors="replace")
    elapsed = re.search(r"Elapsed \(wall clock\) time .*?:\s*([0-9:.]+)", text)
    rss = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", text)
    if not elapsed or not rss:
        problems.append(f"{trial}: GNU time output lacks elapsed time or peak RSS")
        return 0.0, 0
    parts = elapsed.group(1).split(":")
    seconds = sum(float(part) * 60 ** i for i, part in enumerate(reversed(parts)))
    return seconds, int(rss.group(1))

metrics = {trial: time_metrics(trial) for trial in trials}
base_wall = statistics.median(metrics[f"baseline-{n}"][0] for n in (1, 2, 3))
default_wall = statistics.median(metrics[f"default-{n}"][0] for n in (1, 2, 3))
base_rss = statistics.median(metrics[f"baseline-{n}"][1] for n in (1, 2, 3))
default_rss = statistics.median(metrics[f"default-{n}"][1] for n in (1, 2, 3))

# Count baseline/default statuses from the first, representative repetition.
def counts(trial):
    result = {}
    for record in parsed.get(trial, ([], {}, {}))[0]:
        key = record.get("status", "missing")
        result[key] = result.get(key, 0) + 1
    return result

base_counts = counts("baseline-1")
default_counts = counts("default-1")
manifest_digest = hashlib.sha256(selection.read_bytes()).hexdigest()
version_values = {}
for line in (work / "versions.txt").read_text(encoding="utf-8").splitlines():
    if "=" in line:
        key, value = line.split("=", 1)
        version_values[key] = value

def delta(new, old):
    return "n/a" if old == 0 else f"{(new / old - 1) * 100:+.2f}%"

lines = [
    "# JPEG runtime comparison (raw result summary)",
    "",
    f"- Source head SHA: `{version_values.get('source_head_sha', 'unknown')}`; checkout SHA: `{version_values.get('checkout_sha', version_values.get('commit', 'unknown'))}`.",
    f"- Environment: {version_values.get('os', 'unknown')} / `{version_values.get('platform', 'unknown')}`; {version_values.get('rustc', 'rustc unknown')}; {version_values.get('cargo', 'cargo unknown')}; {version_values.get('time', 'GNU time unknown') }.",
    f"- Measurement script SHA-256 `{version_values.get('runtime_script_sha256', 'unknown')}`; workflow SHA-256 `{version_values.get('workflow_sha256', 'unknown')}`; Cargo.lock SHA-256 `{version_values.get('cargo_lock_sha256', 'unknown')}`.",
    f"- Baseline binary: {version_values.get('baseline_binary_bytes', 'unknown')} bytes, SHA-256 `{version_values.get('baseline_binary_sha256', 'unknown')}`; default binary: {version_values.get('default_binary_bytes', 'unknown')} bytes, SHA-256 `{version_values.get('default_binary_sha256', 'unknown')}`.",
    f"- Selected inputs: {len(selected_names)} JPEGs, {sum(int(row[1]) for row in selected)} bytes; selection manifest SHA-256 `{manifest_digest}`.",
    "- Selection: all pinned corpus `.jpg` / `.jpeg` files no larger than 204,800 bytes, sorted by provenance-bearing corpus name; no outcome-based filtering.",
    "- Baseline: current source with the pre-#82 feature set (JPEG coefficient/structure verifier, no `jpeg-v1`) and explicit `--formats jpeg`.",
    "- Candidate: this commit's release-default binary, with `jpeg-v1` and no `--formats` override.",
    "- Both use one worker. Three repetitions per binary, alternated in order; each starts from a fresh copy of the same inputs.",
    "",
    "## Measurements",
    "",
    "| Build | Wall seconds (median of 3) | Peak RSS KiB (median of 3) | Status counts (first run) |",
    "| --- | ---: | ---: | --- |",
    f"| Pre-#82 feature set | {base_wall:.3f} | {base_rss:.0f} | `{json.dumps(base_counts, sort_keys=True)}` |",
    f"| Release default with JPEG V1 | {default_wall:.3f} | {default_rss:.0f} | `{json.dumps(default_counts, sort_keys=True)}` |",
    f"| Delta | {delta(default_wall, base_wall)} | {delta(default_rss, base_rss)} | — |",
    "",
    "Per-run GNU `/usr/bin/time -v` output, tzim NDJSON, stderr, exit code, and final output SHA-256 manifest are retained beside this report.",
    "",
    f"Policy-status differences between the baseline and V1 on the same inputs: {len(divergent_status)} file(s). See the NDJSON rows for each reason.",
]
for name, old, new, note in divergent_status:
    lines.append(f"- `{name}`: `{old}` → `{new}`{(': ' + str(note)) if note else ''}")
lines.extend(["", "## Validation", ""])
if problems:
    lines.extend(["**FAILED**", ""] + [f"- {problem}" for problem in problems])
else:
    lines.append("Passed: one JSON row per selected input; no verification failures; every failed upstream fixture matched the existing exact corpus allowlist; report exit codes agreed with status rows; output manifests agree with JSON after-sizes; and same-status baseline/default files produced identical bytes.")
lines.append("")
(work / "summary.md").write_text("\n".join(lines), encoding="utf-8")

(work / "validation.txt").write_text(
    "PASS\n" if not problems else "FAIL\n" + "\n".join(problems) + "\n",
    encoding="utf-8",
)
if problems:
    for problem in problems:
        print(f"::error::{problem}", file=sys.stderr)
    raise SystemExit(1)
print(f"baseline median: {base_wall:.3f}s / {base_rss:.0f} KiB; default V1 median: {default_wall:.3f}s / {default_rss:.0f} KiB")
PY

(
    cd "$work"
    find . -maxdepth 1 -type f \
        \( -name '*.md' -o -name '*.txt' -o -name '*.tsv' -o -name '*.jsonl' \
        -o -name '*.stderr' -o -name '*.exit' -o -name '*.log' \) \
        ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
)
cat "$work/summary.md"
