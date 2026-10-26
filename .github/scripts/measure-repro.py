#!/usr/bin/env python3
"""#72: re-run the two published measurements and say whether they reproduce.

The audits' numbers (`docs/fits-audit.md` §8.2, `docs/pdf-audit.md` §9.2) were
produced by `fits-measure.py` and `pdf-measure.py` on a corpus that was fetched by
hand and recorded only as repository plus commit. A commit pins the exact bytes of a
*checkout*; it does not pin the bytes of a `codeload` tarball, which GitHub builds
on demand. So the run below is anchored differently:

1. `measure-fetch.py` downloads seven archives **at full commit SHAs**, checks each
   against a recorded SHA-256, extracts them, and writes a manifest with a size and
   a SHA-256 for every one of the 2 386 corpus files. Those hashes are a **new
   baseline**: the first ones anybody recorded for these URLs. They are not claimed
   to be the archives the original runs used, and no check here would detect it if
   GitHub's tarball generation changed its mind, because the first checksum is the
   only one that exists.
2. `measure-fetch.py --verify` re-hashes the tree immediately before the run. The
   measurement scripts take `--manifest` and refuse to read a file it does not name.
3. This script runs both instruments, computes the aggregates **again from their
   JSONL**, and compares them with the published table. A re-run is a new
   measurement: the comparison says whether the published numbers are still what
   the instruments produce, and every refusal, exclusion and verification failure is
   kept in the evidence either way. Nothing is dropped to make a number match.
4. Everything — environment, versions, script hashes, command lines, both JSONLs,
   both logs, the aggregates, this script's own verdict — is written under `--out`
   with a `SHA256SUMS` over it.

Usage (inside a venv that has `numpy`, `astropy`, `pikepdf`, `pymupdf`):

    python3 .github/scripts/measure-repro.py --root /tmp/corpus --out /tmp/evidence

Exit status is 0 only when the manifest verified, both instruments ran, every
aggregate matched the publication and no new class of verification failure appeared.
A red result is the interesting outcome; it is a question for the owner, not
something to wave through.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))

#: The published aggregates, transcribed from the audits. Percentages are rounded
#: to the precision the audit prints them at, and are compared with a tolerance of
#: half of the last printed digit — the instrument is not asked to agree more
#: precisely with a rounded table than the table can express.
PUBLISHED = {
    "fits": dict(
        where="docs/fits-audit.md §8.2",
        files=178,
        tier2=153,
        accepted=151,
        median=21.9,
        q1=17.4,
        q3=27.3,
        mean=19.1,
        share_positive=83,
        maximum=72.7,
        bytes_before=17827200,
        bytes_after=13343040,
        by_corpus={"reproject": 41.9, "cfitsio": 42.1, "drizzlepac": 21.9, "astropy": -7.1},
        exact_failures=2,
    ),
    "pdf": dict(
        where="docs/pdf-audit.md §9.2",
        files=2208,
        accepted=1009,
        median=0.08,
        q1=0.00,
        q3=0.09,
        mean=0.39,
        share_positive=68,
        maximum=29.7,
        qpdf_median=0.59,
        qpdf_q1=-0.07,
        qpdf_q3=1.94,
        qpdf_share_positive=60,
        repack_median=20.1,
        repack_mean=17.0,
        qpdf_check_passed=1009,
        render_files=140,
        render_pages=173,
        render_differences=0,
    ),
}

#: Metrics whose disagreement is recorded and explained rather than failed: either
#: the publication prints them more coarsely than the run can, or the number depends
#: on a library version the run does not pin to the audit's.
SOFT = {
    "pdf": {
        "render_pages":
            "the publication reports 173 pages for the same seeded 140-file sample; a different page "
            "count means MuPDF renders these documents differently from the version the audit used — "
            "recorded, and a reason to look, not a reason to stop",
    },
}

#: How far a percentage may sit from the rounded published value.
PERCENT_TOLERANCE = 0.051


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def run(argv, log_path):
    print("+ %s" % " ".join(argv), flush=True)
    started = time.time()
    completed = subprocess.run(argv, capture_output=True, text=True)
    elapsed = time.time() - started
    with open(log_path, "w") as handle:
        handle.write("$ %s\n\n" % " ".join(argv))
        handle.write(completed.stdout)
        if completed.stderr:
            handle.write("\n--- stderr ---\n")
            handle.write(completed.stderr)
        handle.write("\n--- exit %d in %.1f s ---\n" % (completed.returncode, elapsed))
    return completed, elapsed


def percentile(sorted_values, fraction):
    if not sorted_values:
        return None
    index = int(fraction * len(sorted_values))
    return sorted_values[min(index, len(sorted_values) - 1)]


def distribution(values):
    values = sorted(values)
    if not values:
        return dict(n=0)
    mean = sum(values) / len(values)
    return dict(
        n=len(values),
        median=percentile(values, 0.5),
        q1=percentile(values, 0.25),
        q3=percentile(values, 0.75),
        mean=mean,
        share_positive=100.0 * sum(1 for v in values if v > 0) / len(values),
        maximum=values[-1],
        minimum=values[0],
    )


def load_jsonl(path):
    rows = []
    with open(path) as handle:
        for line in handle:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def render_totals(text):
    """The last `rendered N files, M pages, K differences` line the instrument printed."""
    out = {}
    for line in text.splitlines():
        if line.startswith("rendered "):
            words = line.replace(",", "").split()
            try:
                out = dict(
                    render_files=int(words[1]),
                    render_pages=int(words[3]),
                    render_differences=int(words[5]),
                )
            except (IndexError, ValueError):
                continue
    return out


def fits_aggregates(rows):
    """The FITS table, recomputed from the JSONL rather than read from stdout."""
    by_status = {}
    for row in rows:
        by_status[row["status"]] = by_status.get(row["status"], 0) + 1
    tier2 = [r for r in rows if r["status"] == "tier2"]
    accepted = [r for r in tier2 if r.get("exact")]
    failed = [r for r in tier2 if not r.get("exact")]
    per_file = distribution([r["pct"] for r in accepted])
    per_corpus = {}
    for corpus in sorted({r["corpus"] for r in rows}):
        subset = [r["pct"] for r in accepted if r["corpus"] == corpus]
        if subset:
            per_corpus[corpus] = percentile(sorted(subset), 0.5)
    total_before = sum(r["bytes"] for r in accepted)
    total_after = sum(r["new_size"] for r in accepted)
    refusals = {}
    for row in rows:
        if row["status"] == "refused":
            refusals[row["reason"]] = refusals.get(row["reason"], 0) + 1
    return dict(
        files=len(rows),
        by_status=by_status,
        tier2=len(tier2),
        accepted=len(accepted),
        **per_file,
        bytes_before=total_before,
        bytes_after=total_after,
        bytes_pct=100.0 * (total_before - total_after) / total_before if total_before else None,
        by_corpus=per_corpus,
        refusals=refusals,
        exact_failures=[
            dict(path=os.path.basename(r["path"]), why=r.get("why"), corpus=r["corpus"])
            for r in failed
        ],
    )


def pdf_aggregates(rows):
    by_status = {}
    for row in rows:
        by_status[row["status"]] = by_status.get(row["status"], 0) + 1
    accepted = [r for r in rows if r["status"] == "accepted"]
    faithful = distribution([r["faithful_pct"] for r in accepted])
    qpdf = distribution([r["qpdf_pct"] for r in accepted])
    repack = distribution([r["repack_pct"] for r in accepted])
    refusals = {}
    for row in rows:
        if row["status"] == "refused":
            refusals[row["reason"]] = refusals.get(row["reason"], 0) + 1
    by_corpus = {}
    for corpus in sorted({r["corpus"] for r in rows}):
        subset = [r["faithful_pct"] for r in accepted if r["corpus"] == corpus]
        if subset:
            by_corpus[corpus] = percentile(sorted(subset), 0.5)
    return dict(
        files=len(rows),
        by_status=by_status,
        accepted=len(accepted),
        median=faithful.get("median"),
        q1=faithful.get("q1"),
        q3=faithful.get("q3"),
        mean=faithful.get("mean"),
        share_positive=faithful.get("share_positive"),
        maximum=faithful.get("maximum"),
        qpdf_median=qpdf.get("median"),
        qpdf_q1=qpdf.get("q1"),
        qpdf_q3=qpdf.get("q3"),
        qpdf_share_positive=qpdf.get("share_positive"),
        repack_median=repack.get("median"),
        repack_mean=repack.get("mean"),
        qpdf_check_passed=sum(1 for r in accepted if r.get("check_ok")),
        payloads_equal=sum(1 for r in accepted if r.get("payloads_equal")),
        refusals=refusals,
        by_corpus=by_corpus,
    )


def compare(measured, published, soft=None):
    """Metric-by-metric comparison against the published table."""
    soft = soft or {}
    problems = []
    rows = []
    for key, want in published.items():
        if key in ("where", "by_corpus", "exact_failures"):
            continue
        got = measured.get(key)
        if got is None:
            problems.append("%s is missing from the run" % key)
            rows.append(dict(metric=key, published=want, measured=None, verdict="missing"))
            continue
        if key in soft:
            verdict = "recorded"
        elif isinstance(want, float):
            verdict = "reproduced" if abs(got - want) <= PERCENT_TOLERANCE else "DIFFERENT"
        elif key.endswith("share_positive"):
            # The audit prints a share of a count as whole per cent; one point is
            # that rounding and not a different measurement.
            verdict = "reproduced" if abs(got - want) <= 1 else "DIFFERENT"
        else:
            verdict = "reproduced" if got == want else "DIFFERENT"
        show = round(got, 4) if isinstance(got, float) else got
        rows.append(dict(metric=key, published=want, measured=show, verdict=verdict,
                         note=soft.get(key)))
        if verdict == "DIFFERENT":
            problems.append("%s: published %s, measured %s" % (key, want, got))
    for corpus, want in (published.get("by_corpus") or {}).items():
        got = (measured.get("by_corpus") or {}).get(corpus)
        if got is None:
            problems.append("median for %s is missing from the run" % corpus)
            rows.append(dict(metric="median[%s]" % corpus, published=want, measured=None, verdict="missing"))
            continue
        ok = abs(got - want) <= PERCENT_TOLERANCE
        rows.append(
            dict(metric="median[%s]" % corpus, published=want, measured=round(got, 4),
                 verdict="reproduced" if ok else "DIFFERENT")
        )
        if not ok:
            problems.append("median for %s: published %s, measured %s" % (corpus, want, got))
    return rows, problems


def environment():
    """Versions, hashes and identities. Everything a re-run needs to be a re-run."""
    env = dict(
        python=sys.version,
        executable=sys.executable,
        platform=platform.platform(),
        machine=platform.machine(),
        processor=platform.processor(),
        started=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        packages={},
        scripts={},
    )
    for name in ("numpy", "astropy", "pikepdf", "pymupdf", "fitz"):
        try:
            module = __import__(name)
        except ImportError:
            env["packages"][name] = None
            continue
        env["packages"][name] = {
            "version": getattr(module, "__version__", None),
            "file": getattr(module, "__file__", None),
        }
    try:
        import pikepdf

        env["packages"].setdefault("pikepdf", {})["libqpdf"] = pikepdf.__libqpdf_version__
    except Exception:  # noqa: BLE001
        pass
    for name in ("pymupdf", "fitz"):
        try:
            module = __import__(name)
            version = getattr(module, "mupdf_version", None)
            if version:
                env["packages"].setdefault(name, {})["mupdf"] = version
        except Exception:  # noqa: BLE001
            pass
    for name in ("measure-fetch.py", "fits-measure.py", "pdf-measure.py", "measure-repro.py"):
        path = os.path.join(HERE, name)
        if os.path.exists(path):
            env["scripts"][name] = sha256_of(path)
    try:
        env["git_head"] = subprocess.run(
            ["git", "-C", HERE, "rev-parse", "HEAD"], capture_output=True, text=True
        ).stdout.strip()
        env["git_dirty"] = bool(
            subprocess.run(["git", "-C", HERE, "status", "--porcelain"], capture_output=True, text=True).stdout
        )
    except OSError:
        pass
    return env


def write_summary(path, payload):
    lines = ["# Measurement re-run", ""]
    lines.append("Run at %s, new measurement, not the published one." % payload["environment"]["started"])
    lines.append("")
    lines.append("Manifest: `%s`, %d files, digest `%s`." % (
        payload.get("manifest_path", "?"),
        payload.get("manifest_files", 0),
        payload.get("manifest_digest", "?"),
    ))
    lines.append("")
    for name in ("fits", "pdf"):
        block = payload.get(name)
        if not block:
            continue
        lines.append("## %s — published in %s" % (name.upper(), PUBLISHED[name]["where"]))
        lines.append("")
        lines.append("| metric | published | measured | verdict |")
        lines.append("| --- | --- | --- | --- |")
        for row in block["comparison"]:
            lines.append("| `%s` | %s | %s | %s |" % (
                row["metric"], row["published"], row["measured"], row["verdict"]))
        lines.append("")
        for row in block["comparison"]:
            if row.get("note"):
                lines.append("* `%s` is recorded rather than compared: %s." % (row["metric"], row["note"]))
        if any(row.get("note") for row in block["comparison"]):
            lines.append("")
        if block["problems"]:
            lines.append("Disagreements, kept rather than smoothed:")
            lines.append("")
            for problem in block["problems"]:
                lines.append("* %s" % problem)
            lines.append("")
        refusals = block["aggregates"].get("refusals") or {}
        if refusals:
            lines.append("Refusals, by reason: %s." % ", ".join(
                "`%s` %d" % (k, v) for k, v in sorted(refusals.items(), key=lambda kv: -kv[1])))
            lines.append("")
        failures = block["aggregates"].get("exact_failures")
        if failures:
            lines.append("Files the instrument rewrote and could not verify:")
            lines.append("")
            for failure in failures:
                lines.append("* `%s` (`%s`, `%s`)" % (failure["path"], failure["corpus"], failure["why"]))
            lines.append("")
    lines.append("## What this does not prove")
    lines.append("")
    lines.append(
        "The archive hashes above are the first recorded baseline for those codeload URLs; the run "
        "verifies that the bytes match *that* baseline and nothing about what the original audits "
        "downloaded. The corpus is still the four FITS test-data sets and three PDF corpora — no "
        "archive survey FITS product and no PDF from the toolchain that produced the published PDFs "
        "— so the open gaps in §8.1 and §9.1 are unchanged, and the eight-per-cent and fifteen-per-cent "
        "figures are the old thresholds reported, not a new pre-registration."
    )
    lines.append("")
    with open(path, "w") as handle:
        handle.write("\n".join(lines))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", required=True, help="the corpus tree measure-fetch.py produced")
    parser.add_argument("--manifest", default="", help="default: <root>/manifest.json")
    parser.add_argument("--out", required=True, help="where the evidence bundle is written")
    parser.add_argument("--fetch", action="store_true", help="fetch the corpus first")
    parser.add_argument("--no-fits", action="store_true")
    parser.add_argument("--no-pdf", action="store_true")
    parser.add_argument("--render-sample", type=int, default=140)
    args = parser.parse_args()

    manifest = args.manifest or os.path.join(args.root, "manifest.json")
    os.makedirs(args.out, exist_ok=True)
    started = time.time()

    if args.fetch:
        run([sys.executable, os.path.join(HERE, "measure-fetch.py"),
             "--root", args.root, "--manifest", manifest],
            os.path.join(args.out, "fetch.txt"))

    # 1. The manifest is verified before anything is measured.
    verify, _ = run([sys.executable, os.path.join(HERE, "measure-fetch.py"),
                     "--root", args.root, "--manifest", manifest, "--verify"],
                    os.path.join(args.out, "verify.txt"))
    print(verify.stdout.strip().splitlines()[-1] if verify.stdout else "")
    if verify.returncode != 0:
        print("::error::the corpus does not match its manifest; refusing to measure it")
        return 1

    with open(manifest) as handle:
        recorded = json.load(handle)

    payload = dict(
        note=(
            "A re-run is a new measurement. The comparison below says whether the published numbers are "
            "still what the instruments produce; it does not promote this run to the publication."
        ),
        manifest_path=manifest,
        manifest_digest=recorded["manifest_digest"],
        manifest_files=sum(c["files"] for c in recorded["corpora"]),
        corpora=[
            dict(name=c["name"], repo=c["repo"], commit=c["commit"], archive=c["archive"],
                 archive_sha256=c["archive_sha256"], files=c["files"], bytes=c["bytes"],
                 excluded=len(c.get("excluded") or []))
            for c in recorded["corpora"]
        ],
        environment=environment(),
        commands=[],
        problems=[],
    )

    for name, script, enabled in (
        ("fits", "fits-measure.py", not args.no_fits),
        ("pdf", "pdf-measure.py", not args.no_pdf),
    ):
        if not enabled:
            payload[name] = None
            continue
        jsonl = os.path.join(args.out, "%s.jsonl" % name)
        argv = [sys.executable, os.path.join(HERE, script),
                "--root", args.root, "--manifest", manifest, "--out", jsonl]
        if name == "pdf":
            argv += ["--render-sample", str(args.render_sample)]
        payload["commands"].append(" ".join(argv))
        completed, elapsed = run(argv, os.path.join(args.out, "%s-stdout.txt" % name))
        if completed.returncode != 0 or not os.path.exists(jsonl):
            print("::error::%s-measure.py exited %d; see %s-stdout.txt"
                  % (name, completed.returncode, name))
            payload["problems"].append("%s-measure.py exited %d" % (name, completed.returncode))
            payload[name] = dict(aggregates=dict(files=0), comparison=[], problems=[
                "%s-measure.py exited %d" % (name, completed.returncode)])
            continue
        rows = load_jsonl(jsonl)
        aggregates = fits_aggregates(rows) if name == "fits" else pdf_aggregates(rows)
        if name == "pdf":
            aggregates.update(render_totals(completed.stdout))
        comparison, problems = compare(aggregates, PUBLISHED[name], SOFT.get(name))
        payload[name] = dict(
            aggregates=aggregates,
            comparison=comparison,
            problems=problems,
            elapsed_seconds=round(elapsed, 1),
            rows=len(rows),
        )
        payload["problems"].extend("%s: %s" % (name, problem) for problem in problems)
        if name == "fits":
            expected = PUBLISHED["fits"]["exact_failures"]
            found = len(aggregates["exact_failures"])
            if found != expected:
                note = (
                    "the files that fail verification are %d, the publication implies %d"
                    " (151 accepted out of 153 rewritten)"
                    % (found, expected)
                )
                payload["problems"].append("fits: %s" % note)
                payload["fits"]["comparison"].append(
                    dict(metric="exact_failures", published=expected, measured=found, verdict="DIFFERENT")
                )

    payload["elapsed_seconds"] = round(time.time() - started, 1)
    for name, path in (("aggregates", "aggregates.json"), ("summary", "summary.md")):
        target = os.path.join(args.out, path)
        if name == "aggregates":
            with open(target, "w") as handle:
                json.dump(payload, handle, indent=2, sort_keys=True)
                handle.write("\n")

    write_summary(os.path.join(args.out, "summary.md"), payload)

    # 2. A checksum over the whole bundle, so a hand-off can be checked.
    names = sorted(
        name for name in os.listdir(args.out) if name != "SHA256SUMS" and os.path.isfile(os.path.join(args.out, name))
    )
    with open(os.path.join(args.out, "SHA256SUMS"), "w") as handle:
        for name in names:
            handle.write("%s  %s\n" % (sha256_of(os.path.join(args.out, name)), name))
    bundle = sha256_of(os.path.join(args.out, "SHA256SUMS"))

    print("")
    for name in ("fits", "pdf"):
        block = payload.get(name)
        if not block:
            continue
        prints = [row for row in block["comparison"] if row["verdict"] != "reproduced"]
        print("%s: %s" % (name, "reproduced" if not prints else "DIFFERS: " + ", ".join(
            "%s %s != %s" % (row["metric"], row["measured"], row["published"]) for row in prints)))
    print("evidence: %s (%d files)" % (os.path.abspath(args.out), len(names)))
    print("bundle digest: %s" % bundle)

    if payload["problems"]:
        for problem in payload["problems"]:
            print("!! %s" % problem)
        print("::error::the run does not reproduce the publication; see summary.md")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
