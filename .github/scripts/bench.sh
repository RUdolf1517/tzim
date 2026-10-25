#!/usr/bin/env bash
# Comparative benchmarks: Tzim against the established tools for the same format.
#
# Everything runs on one machine, on one corpus, in the same job, on copies of
# the same files. Ratios are therefore comparable; absolute seconds are not
# comparable with any other run and are reported only to show the order of
# magnitude.
#
# Two honesty constraints shape this script:
#
#   1. `oxipng -o max` needs minutes per megabyte, so the timed comparison is
#      capped at files of 200 KiB or less. The ratio of the *whole* real corpus
#      is reported by the corpus run instead, for Tzim alone.
#   2. Contract differences are real and are stated rather than hidden: oxipng
#      and optipng optimize PNG only, jpegtran handles JPEG only, `gzip -9`
#      recompresses a payload but drops the original header (which Tzim will not
#      do) and no widely available tool rewrites a ZIP the way Tzim does. Only
#      like-for-like comparisons appear below.
#
# Usage: bench.sh <tzim-binary> [work-dir]

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
bin="${1:?usage: bench.sh <tzim-binary> [work-dir]}"
# Absolute, and exported: each measurement runs in a fresh `bash -c`, which does
# not inherit shell variables unless they are in the environment.
bin="$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")"
export bin
work="${2:-${RUNNER_TEMP:-/tmp}/tzim-bench}"
max_bytes=204800

rm -rf "$work"
bash "$repo_root/.github/scripts/corpus-fetch.sh" "$work"

# One set per format, so every tool is measured against files it can handle.
mkdir -p "$work/sets/png" "$work/sets/jpeg" "$work/sets/tiff" "$work/sets/zlib" "$work/run"
find "$work/corpus/images" -type f -iname '*.png' -size "-${max_bytes}c" -exec cp {} "$work/sets/png/" \;
find "$work/corpus/images" -type f \( -iname '*.jpg' -o -iname '*.jpeg' \) -size "-${max_bytes}c" -exec cp {} "$work/sets/jpeg/" \;
find "$work/corpus/images" -type f \( -iname '*.tif' -o -iname '*.tiff' \) -size "-${max_bytes}c" -exec cp {} "$work/sets/tiff/" \;
# No corpus ships raw zlib streams, and a zlib stream is DEFLATE with a header:
# wrapping real payloads at the default level is what a .zz file in the wild
# looks like. Written with the same python3 the report below already needs.
python3 - "$work/corpus/images" "$work/sets/zlib" <<'ZT'
import os
import sys
import zlib

src, dst = sys.argv[1], sys.argv[2]
names = sorted(os.listdir(src))
payloads = [n for n in names if n.lower().endswith((".png", ".jpg", ".jpeg"))][:12]
for name in payloads:
    with open(os.path.join(src, name), "rb") as fh:
        data = fh.read()
    with open(os.path.join(dst, name.rsplit(".", 1)[0] + ".zlib"), "wb") as fh:
        fh.write(zlib.compress(data, 6))
ZT
echo "png set:  $(find "$work/sets/png" -type f | wc -l) files under $((max_bytes / 1024)) KiB"
echo "jpeg set: $(find "$work/sets/jpeg" -type f | wc -l) files under $((max_bytes / 1024)) KiB"
echo "tiff set: $(find "$work/sets/tiff" -type f | wc -l) files under $((max_bytes / 1024)) KiB"
echo "zlib set: $(find "$work/sets/zlib" -type f | wc -l) files under $((max_bytes / 1024)) KiB"

results="$work/results.tsv"
: > "$results"

total_bytes() {
    find "$1" -type f -printf '%s\n' | awk '{s += $1} END {printf "%d", s + 0}'
}

measure() {
    local tool="$1" set="$2" fn="$3"
    local dir="$work/run/$tool-$set"
    rm -rf "$dir"
    cp -r "$work/sets/$set" "$dir"

    local before after code
    before="$(total_bytes "$dir")"
    set +e
    /usr/bin/time -v -o "$work/time-$tool-$set.txt" \
        bash -c "$(declare -f "$fn"); $fn \"\$1\"" _ "$dir" \
        > "$work/log-$tool-$set.txt" 2>&1
    code=$?
    set -e
    after="$(total_bytes "$dir")"

    local seconds rss
    seconds="$(awk -F': ' '/Elapsed \(wall clock\)/{print $2}' "$work/time-$tool-$set.txt")"
    seconds="${seconds% (h:mm:ss or m:ss)}"
    rss="$(awk -F': ' '/Maximum resident set size/{print $2}' "$work/time-$tool-$set.txt")"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$tool" "$set" "${seconds:-?}" "${rss:-?}" "$before" "$after" "$code" >> "$results"
    echo "  $tool on $set: $before -> $after bytes in ${seconds:-?}, exit $code"
}

# --- Tzim -------------------------------------------------------------------
tzim_png() {
    "$bin" -r --formats png --jobs 1 --json "$1" > /dev/null || true
}
tzim_png_parallel() {
    "$bin" -r --formats png --json "$1" > /dev/null || true
}
tzim_png_zopfli() {
    "$bin" -r --formats png --zopfli --jobs 1 --json "$1" > /dev/null || true
}
tzim_jpeg() {
    "$bin" -r --formats jpeg --jobs 1 --json "$1" > /dev/null || true
}
tzim_tiff() {
    "$bin" -r --formats tiff --jobs 1 --json "$1" > /dev/null || true
}
tzim_zlib() {
    "$bin" -r --formats zlib --jobs 1 --json "$1" > /dev/null || true
}

# --- oxipng -----------------------------------------------------------------
# `--strip none` is explicit: metadata preservation is the point of comparison.
oxipng_max() {
    oxipng -o max --strip none --quiet -r "$1" || true
}

# --- optipng ----------------------------------------------------------------
optipng_o7() {
    find "$1" -type f -name '*.png' -print0 \
        | xargs -0 -r -n 32 optipng -o7 -quiet -preserve || true
}

# --- jpegtran ---------------------------------------------------------------
# jpegtran writes to stdout, so each file is rewritten through a temporary next
# to it and moved back into place. The rewrite is skipped when it would not
# shrink the file, matching the rule Tzim applies.
jpegtran_optimize() {
    find "$1" -type f \( -iname '*.jpg' -o -iname '*.jpeg' \) -print0 \
        | xargs -0 -r -n 1 -I '{}' sh -c '
            tmp="$1.tzim-bench"
            before=$(wc -c < "$1")
            if jpegtran -copy all -optimize -outfile "$tmp" "$1" >/dev/null 2>&1; then
                after=$(wc -c < "$tmp")
                if [ "$after" -lt "$before" ]; then mv "$tmp" "$1"; else rm -f "$tmp"; fi
            else
                rm -f "$tmp"
            fi' _ '{}'
}

{
    echo "tzim: $("$bin" --version)"
    echo "oxipng: $(oxipng --version 2>/dev/null | head -1 || echo 'not installed')"
    echo "optipng: $(optipng -v 2>/dev/null | head -1 || echo 'not installed')"
    echo "jpegtran: $(jpegtran -version 2>&1 | head -1 || echo 'not installed')"
    echo "host: $(uname -srm)"
    echo "cores: $(nproc)"
} | tee "$work/versions.txt"

echo "== png =="
measure tzim png tzim_png
measure tzim-jobs png tzim_png_parallel
measure tzim-zopfli png tzim_png_zopfli
measure oxipng png oxipng_max
measure optipng png optipng_o7

echo "== jpeg =="
measure tzim jpeg tzim_jpeg
measure jpegtran jpeg jpegtran_optimize

# TIFF and zlib have no packaged reference recompressor with the same contract:
# `tiffcp -c deflate` rewrites the tag layout and the strip geometry, which Tzim
# does not, and `advdef` handles gzip and ZIP but not a bare zlib stream. The
# rows are therefore Tzim alone, and the report says so rather than pairing it
# with a tool that would be measuring a different job.
echo "== tiff =="
measure tzim tiff tzim_tiff

echo "== zlib =="
measure tzim zlib tzim_zlib

echo "== report =="
python3 - "$results" "$work" <<'PY'
import sys

tsv, work = sys.argv[1], sys.argv[2]
rows = []
with open(tsv, encoding="utf-8") as fh:
    for line in fh:
        parts = line.rstrip("\n").split("\t")
        if len(parts) == 7:
            rows.append(parts)

def number(text):
    try:
        return int(float(text))
    except ValueError:
        return 0

labels = {
    "tzim": "tzim (--jobs 1)",
    "tzim-jobs": "tzim (default jobs)",
    "tzim-zopfli": "tzim --zopfli (--jobs 1)",
    "oxipng": "oxipng -o max --strip none",
    "optipng": "optipng -o7 -preserve",
    "jpegtran": "jpegtran -copy all -optimize",
}
order = ["tzim", "tzim-jobs", "tzim-zopfli", "oxipng", "optipng", "jpegtran"]

def table(title, set_name, note=None):
    lines = [
        f"### {title}",
        "",
    ]
    if note:
        lines += [note, ""]
    lines += [
        "| Tool | Wall clock | Peak RSS | Bytes before | Bytes after | Saved |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for tool in order:
        for row in rows:
            if row[1] != set_name or row[0] != tool:
                continue
            _, _, seconds, rss, before, after, code = row
            b, a = number(before), number(after)
            percent = 100.0 * (b - a) / b if b else 0.0
            note = "" if code == "0" else f" (exit {code})"
            lines.append(
                f"| {labels[tool]} | {seconds} | {number(rss) // 1024} MiB | {b} | {a} | "
                f"{b - a} ({percent:.2f} %){note} |"
            )
    lines.append("")
    return lines

versions = open(f"{work}/versions.txt", encoding="utf-8").read().strip()
out = ["## Comparative benchmarks", "", "```", versions, "```", ""]
out.append("Files larger than 200 KiB are excluded from the timed comparison, because")
out.append("`oxipng -o max` needs minutes per megabyte. Copies of the same files per tool.")
out.append("")
out += table("PNG", "png")
out += table("JPEG", "jpeg")
out += table(
    "TIFF",
    "tiff",
    "No like-for-like reference tool: `tiffcp -c deflate` rewrites the tag layout and the strip geometry.",
)
out += table(
    "zlib",
    "zlib",
    "No like-for-like reference tool: `advdef` covers gzip and ZIP, not a bare zlib stream.",
)
text = "\n".join(out)
print(text)
with open(f"{work}/summary.md", "w", encoding="utf-8") as fh:
    fh.write(text + "\n")
PY

echo
echo "results: $results"
