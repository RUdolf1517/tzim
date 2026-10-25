#!/usr/bin/env bash
# Real-file corpus run.
#
# Downloads pinned upstream archives, verifies their SHA-256, assembles a corpus
# of real PNG and JPEG files, adds gzip and ZIP containers made from them with
# the standard tools, then runs the built binary over the whole tree twice.
#
# What the run proves:
#   - no valid real file fails to parse or to be replaced;
#   - no candidate produced on real data is rejected by verification — a
#     verification failure is never allowed, not even for a corrupt input;
#   - the second pass changes nothing: the rewrite is idempotent;
#   - a file that cannot be handled fails cleanly (a typed error, never a panic
#     or a silent truncation).
#
# What it does not prove: content equality. That is the job of the unit and
# integration tests, which decode each result through code paths independent of
# the handler that produced it.
#
# Files that are corrupt *by design* upstream (Pillow and libjpeg-turbo both
# ship such fixtures) are listed in .github/corpus-allowlist.txt, so that a new
# failure is always an error and a listed file that starts succeeding is
# reported too.
#
# Usage: corpus.sh <tzim-binary> [work-dir]

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
bin="${1:?usage: corpus.sh <tzim-binary> [work-dir]}"
work="${2:-${RUNNER_TEMP:-/tmp}/tzim-corpus}"
corpus="$work/corpus"
allowlist="$repo_root/.github/corpus-allowlist.txt"
baseline="$repo_root/.github/corpus-refusals.txt"

rm -rf "$work"
bash "$repo_root/.github/scripts/corpus-fetch.sh" "$work"

manifest() {
    find "$corpus" -type f -print0 | sort -z | xargs -0 sha256sum
}

manifest > "$work/manifest-before.txt"

# JPEG is opt-in by design, so the run names every format it wants.
run() {
    local out="$1" err="$2"
    set +e
    "$bin" --formats png,gzip,zip,ico,jpeg,tiff,zlib --jobs 2 --json -r "$corpus" \
        > "$out" 2> "$err"
    local code=$?
    set -e
    printf '%s' "$code"
}

echo "== pass 1 =="
first_code="$(run "$work/run1.json" "$work/run1.err")"
manifest > "$work/manifest-pass1.txt"

echo "== pass 2 (idempotence) =="
second_code="$(run "$work/run2.json" "$work/run2.err")"
manifest > "$work/manifest-pass2.txt"

if [ -s "$work/run1.err" ]; then
    echo "-- stderr, first pass --"
    tail -n 20 "$work/run1.err"
fi

echo "== gate =="
python3 - "$work" "$first_code" "$second_code" "$allowlist" "$baseline" <<'PY'
import collections, fnmatch, json, os, sys

work, first_code, second_code, allow_path, baseline_path = (
    sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4], sys.argv[5]
)

allow = []
if os.path.exists(allow_path):
    for number, line in enumerate(open(allow_path, encoding="utf-8"), 1):
        line = line.split("#", 1)[0].strip()
        if line:
            allow.append(line)

def allowed(name):
    return any(fnmatch.fnmatchcase(name, pattern) for pattern in allow)

def load(name):
    records = []
    with open(os.path.join(work, name), encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                records.append(json.loads(line))
    return records

first, second = load("run1.json"), load("run2.json")
files1 = [r for r in first if r.get("type") == "file"]
files2 = [r for r in second if r.get("type") == "file"]
summary1 = next((r for r in first if r.get("type") == "summary"), {})
summary2 = next((r for r in second if r.get("type") == "summary"), {})

problems = []
def problem(text):
    problems.append(text)

if first_code not in (0, 1):
    problem(f"first pass exited {first_code}; expected 0, or 1 when a corrupt fixture fails")
if second_code not in (0, 1):
    problem(f"second pass exited {second_code}")

failed = [r for r in files1 if r.get("status") == "failed"]
unexpected = [r for r in failed if not allowed(os.path.basename(r["path"]))]
for record in unexpected:
    err = record.get("error") or {}
    problem(f"failed: {record['path']}: {err.get('kind')}: {err.get('message')}")

for record in failed:
    kind = (record.get("error") or {}).get("kind")
    if kind in ("verify", "internal"):
        problem(f"{kind} error on {record['path']}: a corrupt input must be rejected, not mishandled")

# A pattern that no longer matches a failure means either that an upstream
# fixture was fixed or that the pattern is too broad. Both are worth a look, so
# neither is silent.
all_names = [os.path.basename(r["path"]) for r in files1]
failed_names = set(os.path.basename(r["path"]) for r in failed)
listed = [name for name in all_names if allowed(name)]
healed = sorted(set(listed) - failed_names)
if healed:
    print(
        f"::warning::{len(healed)} allowlisted file(s) did not fail, so the entry can go: "
        + ", ".join(healed[:10])
    )

# A file that is still corrupt on the second pass fails again; that is not a
# change on disk, it is the same refusal. Only a file that is rewritten twice
# means the result is not a fixed point.
changed_again = [r for r in files2 if r.get("status") in ("optimized", "would_optimize")]
if changed_again:
    problem(f"{len(changed_again)} file(s) changed on the second pass, e.g. {changed_again[0]['path']}")

m1 = open(os.path.join(work, "manifest-pass1.txt"), encoding="utf-8").read()
m2 = open(os.path.join(work, "manifest-pass2.txt"), encoding="utf-8").read()
if m1 != m2:
    problem("the second pass modified files on disk")

statuses = collections.Counter(r.get("status") for r in files1)
formats = collections.Counter(r.get("format") or "unknown" for r in files1 if r.get("status") == "optimized")
kinds = collections.Counter(os.path.basename(os.path.dirname(r["path"])) for r in files1)

before, after = summary1.get("before", 0), summary1.get("after", 0)
saved = summary1.get("saved", 0)
optimized = summary1.get("optimized", 0)

lines = []
lines.append("## Real-file corpus run")
lines.append("")
lines.append("| Field | Value |")
lines.append("| --- | --- |")
lines.append(f"| Files | {len(files1)} |")
lines.append(f"| Optimized | {optimized} |")
lines.append(f"| Already optimal | {summary1.get('already_optimal', 0)} |")
lines.append(f"| Unsupported | {summary1.get('unsupported', 0)} |")
lines.append(f"| Skipped (refused by contract: progressive/arithmetic/limit/unsupported layout) | {summary1.get('skipped', 0)} |")
lines.append(f"| Failed | {len(failed)} ({len(failed) - len(unexpected)} expected) |")
lines.append(f"| Bytes before | {before} |")
lines.append(f"| Bytes after | {after} |")
lines.append(f"| Saved | {saved} ({100.0 * saved / (before or 1):.2f} %) |")
lines.append(f"| Second pass, files changed | {len(changed_again)} |")
lines.append("")
lines.append("Optimized files, by format: " +
             ", ".join(f"{name} {count}" for name, count in sorted(formats.items())))
lines.append("")
lines.append("By container: " + ", ".join(f"{name} {count}" for name, count in sorted(kinds.items())))
lines.append("")
skipped = [r for r in files1 if r.get("status") == "skipped"]

def reason(record):
    note = record.get("note") or (record.get("error") or {}).get("message") or "no reason given"
    # "tiff: tiled layout is not supported; only strips are rewritten" becomes
    # "tiled layout is not supported": the handler prefix and the aside go.
    _, _, rest = note.partition(": ")
    clause = (rest or note).split(";")[0].split(" (")[0]
    if len(clause) > 58:
        clause = clause[:58].rsplit(" ", 1)[0] + "..."
    return clause

refused = [r for r in files1 if r.get("status") in ("skipped", "failed", "unsupported")]
by_reason = collections.Counter(reason(r) for r in refused)

if skipped:
    lines.append("Skipped, by reason: " +
                 "; ".join(f"{text} ({count})" for text, count in by_reason.most_common(6)))
    lines.append("")

# The full refusal set, one `<path>\t<reason>` per line: it is the artifact a
# human diffs when a baseline row has to change (#36).
with open(os.path.join(work, "refusals.txt"), "w", encoding="utf-8") as fh:
    root = os.path.join(work, "corpus")
    for record in sorted(refused, key=lambda r: r["path"]):
        try:
            name = os.path.relpath(record["path"], root)
        except ValueError:
            name = os.path.basename(record["path"])
        fh.write(f"{name}\t{reason(record)}\n")

# The same table in the report, collapsed and **bounded**: `ci-report.sh`
# publishes the last 150 lines of the report, so a section that grows without a
# limit silently pushes the numbers out of the comment (it did, on the first run
# of this block). Thirty lines is enough to recognise a reason, and the full list
# is in the `corpus-run` artifact.
if refused:
    with open(os.path.join(work, "refusals.txt"), encoding="utf-8") as fh:
        entries = [line.rstrip("\n") for line in fh]
    lines.append(
        f"<details><summary>The refused set, first {min(len(entries), 30)} of "
        f"{len(entries)} (all of it in the `corpus-run` artifact)</summary>"
    )
    lines.append("")
    lines.append("```")
    lines.extend(entries[:30])
    lines.append("```")
    lines.append("")
    lines.append("</details>")
    lines.append("")
if failed:
    lines.append("### Files that failed")
    lines.append("")
    for record in sorted(failed, key=lambda r: r["path"]):
        err = record.get("error") or {}
        mark = "" if record in unexpected else " _(allowlisted)_"
        lines.append(f"- `{os.path.basename(record['path'])}` — {err.get('kind')}{mark}")
    lines.append("")
if unexpected:
    lines.append("### Unexpected failures")
    for record in unexpected:
        err = record.get("error") or {}
        lines.append(f"- `{record['path']}` — {err.get('kind')}: {err.get('message')}")
    lines.append("")

text = "\n".join(lines)
print(text)
with open(os.path.join(work, "summary.md"), "w", encoding="utf-8") as fh:
    fh.write(text + "\n")

# The refused set has a baseline (#36). `docs/testing.md` promised that "the set
# of refused files is unchanged" fails the run, and nothing checked it: a
# construct that silently stopped being refused was invisible. The baseline is a
# count per reason, not a file list, because the reason is what a regression
# changes (`tiled layout is not supported` becoming `optimized` is the defect);
# the per-file list is written to refusals.txt and uploaded with the report.
expected = {}
if os.path.exists(baseline_path):
    with open(baseline_path, encoding="utf-8") as fh:
        for number, line in enumerate(fh, 1):
            line = line.split("#", 1)[0].rstrip("\n")
            if not line.strip():
                continue
            count, _, text = line.partition("\t")
            try:
                expected[text.strip()] = int(count)
            except ValueError:
                problem(f"corpus-refusals.txt:{number}: expected `<count><TAB><reason>`")
else:
    print("::warning:: no .github/corpus-refusals.txt; the refused set is not checked")

for text, count in sorted(expected.items()):
    actual = by_reason.get(text, 0)
    if actual != count:
        problem(
            f"refused set changed: `{text}` was {count}, now {actual}. "
            "If that is intended, update .github/corpus-refusals.txt in this pull request "
            "and say why in the description."
        )

new_reasons = {text: count for text, count in by_reason.items() if text not in expected}
for text, count in sorted(new_reasons.items()):
    print(f"::warning:: a refusal reason missing from the baseline ({count}): {text}")

if problems:
    print()
    for text in problems:
        print(f"::error::{text}")
    sys.exit(1)
print("corpus gate: OK")
PY

echo "corpus gate passed"
