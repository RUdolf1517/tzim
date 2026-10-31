#!/usr/bin/env python3
"""#72: reproduce the disclosed `measure-repro-2026-10-31-v2` baseline.

The two audits (`docs/fits-audit.md` §8.2 and `docs/pdf-audit.md` §9.2) began
with manually fetched inputs. `measure-fetch.py` now downloads seven archives at
full commit SHAs, checks the pinned archive SHA-256 values, records every selected
file's size and SHA-256, and verifies the extracted tree before measurement. The
expected source-manifest digest is also pinned by the versioned baseline JSON.

This check compares the resulting measurements with one explicit, versioned
baseline. The old published values and the red source run 41239337100 remain
historical errata; v2 does not silently rewrite them or claim that they reproduced.
The new `pdf-zopfli-small-v1` sample is likewise a new sample, not a reproduction of
the old unavailable Zopfli sample. Its ordered manifest and every per-file result
are versioned and compared exactly.

A green status means this disclosed v2 baseline reproduced—including its known
FITS refusals and the retained `zerowidth.fits/array_0` exact failure. It does not
mean zero measurement failures, full FITS/PDF admission, or a release approval.
Every refusal, exclusion, identity mismatch, roundtrip error, and verification
failure remains in the evidence; none is dropped to make a number match.

Everything—manifest, environment, script hashes, commands, measurements,
per-file diagnostics, comparisons and logs—is written under `--out` with a
`SHA256SUMS` file.

    python3 .github/scripts/measure-repro.py --root /tmp/corpus --out /tmp/evidence
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import random
import subprocess
import sys
import time
import zlib
from decimal import Decimal, ROUND_HALF_UP, InvalidOperation

HERE = os.path.dirname(os.path.abspath(__file__))

#: Versioned expected values and provenance are data, not silently maintained
#: transcriptions in this script. A green run means this exact baseline reproduced;
#: known refusals and the retained exact failure remain part of the baseline.
BASELINE_DIR = os.path.join(HERE, "measure-repro-baselines")
BASELINE_PATH = os.path.join(BASELINE_DIR, "measure-repro-2026-10-31-v2.json")
with open(BASELINE_PATH, encoding="utf-8") as _baseline_file:
    BASELINE = json.load(_baseline_file)

BASELINE_ID = BASELINE["baseline_id"]
PUBLISHED = BASELINE["published_tables"]
PUBLISHED_PRECISION = BASELINE["published_precision"]
PUBLISHED_INPUT_CORPORA = BASELINE["published_input_corpora"]
PUBLISHED_ZOPFLI = BASELINE["pdf_zopfli"]
SAMPLE_BASELINE_PATH = os.path.join(BASELINE_DIR, PUBLISHED_ZOPFLI["sample_manifest_file"])
RESULTS_BASELINE_PATH = os.path.join(BASELINE_DIR, PUBLISHED_ZOPFLI["results_file"])

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
    """Nearest-rank quartile; at p=0.5 use the conventional arithmetic median."""
    if not sorted_values:
        return None
    if fraction == 0.5 and len(sorted_values) % 2 == 0:
        middle = len(sorted_values) // 2
        return (sorted_values[middle - 1] + sorted_values[middle]) / 2
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
    if not out:
        return dict(render_files=0, render_pages=0, render_differences=0)
    return out


def render_aggregates(rows):
    """Derive render totals from the selected-file records, not only stdout."""
    rendered = [row for row in rows if row.get("rendered")]
    return dict(
        render_files=len(rendered),
        render_pages=sum(row.get("compared_pages", 0) for row in rendered),
        render_differences=sum(1 for row in rendered if not row.get("identical")),
        render_sampled_files=len(rows),
        render_errors=sum(1 for row in rows if row.get("render_error")),
    )


def fits_aggregates(rows):
    """Full FITS result table, recomputed from the per-file JSONL."""
    by_status = {}
    for row in rows:
        by_status[row["status"]] = by_status.get(row["status"], 0) + 1
    tier2 = [r for r in rows if r["status"] == "tier2"]
    accepted = [r for r in tier2 if r.get("exact")]
    failed = [r for r in tier2 if not r.get("exact")]
    tier2_distribution = distribution([r["pct"] for r in accepted])
    tier1_rows = [r for r in accepted if r.get("tier1_pct") is not None]
    tier1_distribution = distribution([r["tier1_pct"] for r in tier1_rows])
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
        tier2_candidates=len(tier2),
        tier2_files=len(accepted),
        tier2_median=tier2_distribution.get("median"),
        tier2_q1=tier2_distribution.get("q1"),
        tier2_q3=tier2_distribution.get("q3"),
        tier2_mean=tier2_distribution.get("mean"),
        tier2_share_positive=tier2_distribution.get("share_positive"),
        tier2_maximum=tier2_distribution.get("maximum"),
        tier1_files=len(tier1_rows),
        tier1_median=tier1_distribution.get("median"),
        tier1_q1=tier1_distribution.get("q1"),
        tier1_q3=tier1_distribution.get("q3"),
        tier1_mean=tier1_distribution.get("mean"),
        tier1_share_positive=tier1_distribution.get("share_positive"),
        tier1_maximum=tier1_distribution.get("maximum"),
        bytes_before=total_before,
        bytes_after=total_after,
        bytes_pct=100.0 * (total_before - total_after) / total_before if total_before else None,
        by_corpus=per_corpus,
        refusals=refusals,
        exact_failure_count=len(failed),
        exact_failures=[
            dict(
                path=os.path.basename(r.get("path", r.get("manifest_path", ""))),
                manifest_path=r.get("manifest_path"),
                manifest_sha256=r.get("manifest_sha256"),
                why=r.get("why"),
                corpus=r["corpus"],
            )
            for r in failed
        ],
    )


def _fits_manifest_path(row, root=None):
    """Return the stable manifest path used in the public diagnostics."""
    path = row.get("manifest_path")
    if path:
        return path
    if row.get("path") and root:
        return os.path.relpath(row["path"], root)
    return row.get("path")


def fits_refusal_diagnostics(rows, root=None):
    """All `cannot_compress` rows with corpus/path/input identity and full error."""
    diagnostics = []
    for row in rows:
        if row.get("status") != "refused" or row.get("reason") != "cannot_compress":
            continue
        diagnostics.append(dict(
            corpus=row.get("corpus"),
            manifest_path=_fits_manifest_path(row, root),
            input_sha256=row.get("manifest_sha256"),
            error=row.get("err"),
        ))
    return sorted(diagnostics, key=lambda item: (item.get("corpus") or "", item.get("manifest_path") or ""))


def compare_fits_refusal_diagnostics(rows, root=None):
    """Require the complete five-row refusal identity/error set, without changing counts."""
    expected = sorted(
        BASELINE["fits_refusal_diagnostics"],
        key=lambda item: (item.get("corpus") or "", item.get("manifest_path") or ""),
    )
    observed = fits_refusal_diagnostics(rows, root)
    verdict = "reproduced" if observed == expected else "DIFFERENT"
    problems = []
    if verdict != "reproduced":
        if len(observed) != len(expected):
            problems.append("cannot_compress refusal diagnostic count differs: expected %d, measured %d"
                            % (len(expected), len(observed)))
        expected_by_path = {row.get("manifest_path"): row for row in expected}
        observed_by_path = {row.get("manifest_path"): row for row in observed}
        for path in sorted(set(expected_by_path) | set(observed_by_path)):
            want = expected_by_path.get(path)
            got = observed_by_path.get(path)
            if want is None:
                problems.append("unexpected cannot_compress refusal diagnostic: %s" % got)
            elif got is None:
                problems.append("missing cannot_compress refusal diagnostic: %s" % path)
            elif got != want:
                problems.append("cannot_compress diagnostic differs for %s: expected %s, measured %s"
                                % (path, want, got))
    comparison = dict(expected=expected, measured=observed, verdict=verdict)
    return comparison, problems


def fits_exact_failure_diagnostics(rows, root=None):
    """Stable identity/hash for every tier2 candidate that failed exact verification."""
    failures = []
    for row in rows:
        if row.get("status") != "tier2" or row.get("exact") is not False:
            continue
        failures.append(dict(
            corpus=row.get("corpus"),
            manifest_path=_fits_manifest_path(row, root),
            input_sha256=row.get("manifest_sha256"),
            why=row.get("why"),
        ))
    return sorted(failures, key=lambda item: (item.get("corpus") or "", item.get("manifest_path") or ""))


def compare_fits_exact_failure_diagnostics(rows, root=None):
    """Keep the explicit zerowidth/array_0 failure visible and identity-pinned."""
    expected = sorted(
        BASELINE["fits_exact_failures"],
        key=lambda item: (item.get("corpus") or "", item.get("manifest_path") or ""),
    )
    observed = fits_exact_failure_diagnostics(rows, root)
    verdict = "reproduced" if observed == expected else "DIFFERENT"
    if verdict == "reproduced":
        problems = []
    else:
        problems = ["FIT exact-failure identities differ: expected %s, measured %s" % (expected, observed)]
    return dict(expected=expected, measured=observed, verdict=verdict), problems


def pdf_aggregates(rows):
    """Full PDF result/status/refusal table, recomputed from all JSONL rows."""
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
    files = len(rows)
    already_optimal = by_status.get("already_optimal", 0)
    refused = by_status.get("refused", 0)
    accepted_count = len(accepted)
    return dict(
        files=files,
        by_status=by_status,
        accepted=accepted_count,
        already_optimal=already_optimal,
        refused=refused,
        accepted_pct=100.0 * accepted_count / files if files else None,
        already_optimal_pct=100.0 * already_optimal / files if files else None,
        refused_pct=100.0 * refused / files if files else None,
        median=faithful.get("median"),
        q1=faithful.get("q1"),
        q3=faithful.get("q3"),
        mean=faithful.get("mean"),
        share_positive=faithful.get("share_positive"),
        share_positive_count=sum(1 for r in accepted if r["faithful_pct"] > 0),
        share_positive_denominator=accepted_count,
        share_zero_count=sum(1 for r in accepted if r["faithful_pct"] == 0),
        share_negative_count=sum(1 for r in accepted if r["faithful_pct"] < 0),
        maximum=faithful.get("maximum"),
        qpdf_median=qpdf.get("median"),
        qpdf_q1=qpdf.get("q1"),
        qpdf_q3=qpdf.get("q3"),
        qpdf_mean=qpdf.get("mean"),
        qpdf_share_positive=qpdf.get("share_positive"),
        qpdf_maximum=qpdf.get("maximum"),
        repack_median=repack.get("median"),
        repack_mean=repack.get("mean"),
        qpdf_check_passed=sum(1 for r in accepted if r.get("check_ok")),
        payloads_equal=sum(1 for r in accepted if r.get("payloads_equal")),
        refusals=refusals,
        by_corpus=by_corpus,
    )


def _rounded_decimal(value, places):
    quantum = Decimal(1).scaleb(-places)
    return Decimal(str(value)).quantize(quantum, rounding=ROUND_HALF_UP)


def _display_rounded(value, places):
    rounded = _rounded_decimal(value, places)
    return int(rounded) if places == 0 else float(rounded)


def _compare_count_mapping(actual, expected, prefix):
    rows = []
    problems = []
    for key in sorted(set(actual) | set(expected)):
        got = actual.get(key, 0)
        want = expected.get(key)
        if key not in expected:
            verdict = "DIFFERENT"
            problems.append("unexpected %s %s: measured %s" % (prefix, key, got))
        elif got != want:
            verdict = "DIFFERENT"
            problems.append("%s %s: published %s, measured %s" % (prefix, key, want, got))
        else:
            verdict = "reproduced"
        rows.append(dict(metric="%s[%s]" % (prefix, key), published=want,
                         measured=got, verdict=verdict))
    return rows, problems


def compare(measured, published, precision=None):
    """Compare every numeric result-table cell at its printed precision."""
    precision = precision or {}
    group_keys = {"where", "by_corpus", "by_status", "refusals", "exact_failure_set"}
    problems = []
    rows = []
    for key, want in published.items():
        if key in group_keys:
            continue
        got = measured.get(key)
        if got is None:
            problems.append("%s is missing from the run" % key)
            rows.append(dict(metric=key, published=want, measured=None, verdict="missing"))
            continue
        if key in precision:
            places = precision[key]
            try:
                measured_display = _display_rounded(got, places)
                published_display = _display_rounded(want, places)
            except (InvalidOperation, ValueError, TypeError):
                measured_display = got
                published_display = want
                verdict = "invalid"
                problems.append("%s could not be compared at %d decimal places" % (key, places))
            else:
                verdict = "reproduced" if measured_display == published_display else "DIFFERENT"
                if verdict == "DIFFERENT":
                    problems.append(
                        "%s: published %s, measured %s (rounded to %d decimal places)"
                        % (key, want, got, places)
                    )
        elif isinstance(want, float):
            measured_display = got
            published_display = want
            verdict = "invalid"
            problems.append("%s is a rounded float without a declared printed precision" % key)
        else:
            measured_display = got
            published_display = want
            verdict = "reproduced" if got == want else "DIFFERENT"
            if verdict == "DIFFERENT":
                problems.append("%s: published %s, measured %s" % (key, want, got))
        rows.append(dict(metric=key, published=published_display,
                         measured=measured_display, measured_exact=got,
                         precision=precision.get(key), verdict=verdict))

    published_corpora = published.get("by_corpus") or {}
    measured_corpora = measured.get("by_corpus") or {}
    corpus_keys = (
        sorted(set(published_corpora) | set(measured_corpora))
        if "by_corpus" in published else []
    )
    for key in corpus_keys:
        want = published_corpora.get(key)
        got = measured_corpora.get(key)
        if key not in published_corpora:
            problems.append("unexpected corpus median %s: measured %s" % (key, got))
            rows.append(dict(metric="median[%s]" % key, published=None,
                             measured=got, verdict="DIFFERENT"))
            continue
        if got is None:
            problems.append("median for %s is missing from the run" % key)
            rows.append(dict(metric="median[%s]" % key, published=want,
                             measured=None, verdict="missing"))
            continue
        places = precision.get("by_corpus")
        if places is None:
            verdict = "invalid"
            measured_display = got
            problems.append("by_corpus medians have no declared printed precision")
        else:
            measured_display = _display_rounded(got, places)
            published_display = _display_rounded(want, places)
            verdict = "reproduced" if measured_display == published_display else "DIFFERENT"
            if verdict == "DIFFERENT":
                problems.append(
                    "median for %s: published %s, measured %s (rounded to %d decimal places)"
                    % (key, want, got, places)
                )
        rows.append(dict(metric="median[%s]" % key, published=want,
                         measured=measured_display, measured_exact=got,
                         precision=places, verdict=verdict))

    for key in ("by_status", "refusals"):
        mapping_rows, mapping_problems = _compare_count_mapping(
            measured.get(key) or {}, published.get(key) or {}, key
        )
        rows.extend(mapping_rows)
        problems.extend(mapping_problems)

    if "exact_failure_set" in published:
        expected = sorted(tuple(item) for item in published["exact_failure_set"])
        observed = sorted(
            (os.path.basename(item.get("path", "")), item.get("why"))
            for item in measured.get("exact_failures", [])
        )
        verdict = "reproduced" if observed == expected else "DIFFERENT"
        rows.append(dict(metric="exact_failure_set", published=[list(x) for x in expected],
                         measured=[list(x) for x in observed], verdict=verdict))
        if verdict != "reproduced":
            problems.append("exact failure identities differ: published %s, measured %s"
                            % (expected, observed))
    return rows, problems


def _unit_divisor(unit):
    return {"B": 1, "KiB": 1024, "MiB": 1024 ** 2,
            "kB": 1000, "MB": 1000 ** 2, "GB": 1000 ** 3}[unit]


def compare_input_corpora(corpora, family):
    """Compare full manifest file counts and selected bytes with the audit table."""
    published = PUBLISHED_INPUT_CORPORA[family]
    actual = {
        row["name"]: row for row in corpora
        if row.get("format") == ("fits" if family == "fits" else "pdf")
    }
    rows = []
    problems = []

    def add_row(metric, want, got, verdict, extra=None):
        row = dict(metric=metric, published=want, measured=got, verdict=verdict)
        if extra:
            row.update(extra)
        rows.append(row)
        if verdict != "reproduced":
            problems.append("%s: published %s, measured %s" % (metric, want, got))

    expected_names = set(published["rows"])
    for name in sorted(expected_names | set(actual)):
        expected = published["rows"].get(name)
        found = actual.get(name)
        if expected is None:
            for field in ("repo", "commit", "files", "bytes"):
                add_row("%s.%s.%s" % (family, name, field), None,
                        found.get(field) if found else None, "DIFFERENT")
            continue
        if found is None:
            add_row("%s.%s.repo" % (family, name), expected["repo"], None, "missing")
            add_row("%s.%s.commit" % (family, name), expected["commit_prefix"], None, "missing")
            add_row("%s.%s.files" % (family, name), expected["files"], None, "missing")
            add_row("%s.%s.bytes" % (family, name), expected["bytes"], None, "missing")
            continue
        repo_verdict = "reproduced" if found.get("repo") == expected["repo"] else "DIFFERENT"
        add_row("%s.%s.repo" % (family, name), expected["repo"], found.get("repo"), repo_verdict)
        commit = found.get("commit", "")
        commit_prefix = expected["commit_prefix"]
        commit_verdict = (
            "reproduced" if len(commit) == 40 and commit.startswith(commit_prefix) else "DIFFERENT"
        )
        add_row("%s.%s.commit" % (family, name), commit_prefix, commit, commit_verdict,
                extra=dict(published_is_abbreviated_prefix=True))
        file_verdict = "reproduced" if found["files"] == expected["files"] else "DIFFERENT"
        add_row("%s.%s.files" % (family, name), expected["files"], found["files"], file_verdict)
        unit = expected["unit"]
        scaled = found["bytes"] / _unit_divisor(unit)
        places = expected["places"]
        got_display = _display_rounded(scaled, places)
        want_display = _display_rounded(expected["bytes"], places)
        byte_verdict = "reproduced" if got_display == want_display else "DIFFERENT"
        add_row(
            "%s.%s.bytes[%s]" % (family, name, unit), want_display, got_display,
            byte_verdict,
            extra=dict(measured_exact_bytes=found["bytes"], selected_bytes=True,
                       precision=places),
        )

    expected_total = published["total"]
    total_files = sum(row["files"] for row in actual.values())
    total_bytes = sum(row["bytes"] for row in actual.values())
    files_verdict = "reproduced" if total_files == expected_total["files"] else "DIFFERENT"
    add_row("%s.total.files" % family, expected_total["files"], total_files, files_verdict)
    unit = expected_total["unit"]
    scaled = total_bytes / _unit_divisor(unit)
    places = expected_total["places"]
    got_display = _display_rounded(scaled, places)
    want_display = _display_rounded(expected_total["bytes"], places)
    bytes_verdict = "reproduced" if got_display == want_display else "DIFFERENT"
    add_row(
        "%s.total.bytes[%s]" % (family, unit), want_display, got_display,
        bytes_verdict,
        extra=dict(measured_exact_bytes=total_bytes, selected_bytes=True,
                   precision=places),
    )
    return rows, problems


def environment():
    """Versions, hashes and identities. Everything a re-run needs to be a re-run."""
    env = dict(
        python=sys.version,
        executable=sys.executable,
        platform=platform.platform(),
        machine=platform.machine(),
        processor=platform.processor(),
        zlib_compile_version=zlib.ZLIB_VERSION,
        zlib_runtime_version=zlib.ZLIB_RUNTIME_VERSION,
        started=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        packages={},
        scripts={},
        baseline=dict(
            id=BASELINE_ID,
            file=os.path.relpath(BASELINE_PATH, HERE),
            sha256=sha256_of(BASELINE_PATH),
            zopfli_sample_sha256=PUBLISHED_ZOPFLI["sample_manifest_sha256"],
            zopfli_results_sha256=PUBLISHED_ZOPFLI["results_jsonl_sha256"],
            zopfli_calibration_aggregate_sha256=PUBLISHED_ZOPFLI["calibration_aggregate_sha256"],
        ),
    )
    for name in ("numpy", "astropy", "pikepdf", "pymupdf", "fitz", "zopfli"):
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
    for name in ("measure-fetch.py", "fits-measure.py", "pdf-measure.py", "pdf-zopfli-measure.py", "measure-repro.py"):
        path = os.path.join(HERE, name)
        if os.path.exists(path):
            env["scripts"][name] = sha256_of(path)
    try:
        env["git_head"] = subprocess.run(
            ["git", "-C", HERE, "rev-parse", "HEAD"], capture_output=True, text=True
        ).stdout.strip()
        # Evidence is written under the checkout and appears as an untracked
        # directory; dirty means tracked source changes, not this run's outputs.
        env["git_dirty"] = bool(
            subprocess.run(
                ["git", "-C", HERE, "status", "--porcelain", "--untracked-files=no"],
                capture_output=True,
                text=True,
            ).stdout
        )
    except OSError:
        pass
    env["requested_sha"] = os.environ.get("REQUESTED_SHA", os.environ.get("GITHUB_SHA"))
    env["checked_out_sha"] = env.get("git_head")
    env["run_id"] = os.environ.get("GITHUB_RUN_ID")
    env["run_attempt"] = os.environ.get("GITHUB_RUN_ATTEMPT")
    env["workflow"] = os.environ.get("GITHUB_WORKFLOW")
    return env


def write_summary(path, payload):
    lines = ["# Measurement re-run", ""]
    baseline = payload.get("baseline", {})
    provenance = BASELINE["provenance"]
    source_run = provenance["hosted_source_run"]
    evidence = provenance["independent_evidence_bundle"]
    lines.append("Run at %s; this is a new measurement against a versioned baseline." %
                 payload["environment"]["started"])
    lines.append("")
    lines.append("## Active comparison baseline")
    lines.append("")
    lines.append("- Baseline ID: `%s`." % baseline.get("baseline_id", BASELINE_ID))
    baseline_file = payload["environment"].get("baseline", {})
    lines.append("- Baseline JSON: `%s` (SHA-256 `%s`)." % (
        baseline_file.get("file"), baseline_file.get("sha256")))
    lines.append("- Provenance: hosted source run [%s](%s) at `%s`, plus owner-specified independent evidence Git blob `%s` (ZIP SHA-256 `%s`, %s bytes)." % (
        source_run["run_id"], source_run["url"], provenance["source_head"],
        evidence["git_blob"], evidence["sha256"], evidence["bytes"]))
    lines.append("- Source manifest digest: `%s` (2,386 fixed, full-SHA-pinned inputs)." %
                 BASELINE["manifest_digest"])
    lines.append("- A green check means reproduction of this disclosed v2 baseline, including its known refusals and retained FITS exact failure. It does **not** mean zero measurement failures or full FITS/PDF admission.")
    lines.append("- Historical pre-v2 values and red hosted run `%s` remain errata/history; this run does not relabel that run as green." %
                 source_run["run_id"])
    lines.append("")
    lines.append("Manifest: `%s`, %d files, digest `%s`." % (
        payload.get("manifest_path", "?"),
        payload.get("manifest_files", 0),
        payload.get("manifest_digest", "?"),
    ))
    lines.append("")
    for family in ("fits", "pdf"):
        block = payload.get("input_corpus_comparison", {}).get(family)
        if not block:
            continue
        lines.append("## Input corpus table — current baseline in %s" % PUBLISHED_INPUT_CORPORA[family]["where"])
        lines.append("")
        lines.append("Selected-file sizes from the verified manifest are compared in the audit's printed units and precision; archive bytes are not substituted.")
        lines.append("")
        lines.append("| corpus metric | baseline | measured | verdict |")
        lines.append("| --- | --- | --- | --- |")
        for row in block["rows"]:
            lines.append("| `%s` | %s | %s | %s |" % (
                row["metric"], row["published"], row["measured"], row["verdict"]))
        lines.append("")
        if block["problems"]:
            for problem in block["problems"]:
                lines.append("* %s" % problem)
            lines.append("")
    for name in ("fits", "pdf"):
        block = payload.get(name)
        if not block:
            continue
        lines.append("## %s — current baseline in %s" % (name.upper(), PUBLISHED[name]["where"]))
        lines.append("")
        lines.append("| metric | baseline | measured | verdict |")
        lines.append("| --- | --- | --- | --- |")
        for row in block["comparison"]:
            lines.append("| `%s` | %s | %s | %s |" % (
                row["metric"], row["published"], row["measured"], row["verdict"]))
        lines.append("")
        for row in block["comparison"]:
            if row.get("note"):
                lines.append("* `%s`: %s." % (row["metric"], row["note"]))
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
        if name == "fits":
            diagnostics = block.get("refusal_diagnostics", {}).get("measured", [])
            lines.append("### FITS `cannot_compress` refusal diagnostics")
            lines.append("")
            lines.append("These are refusals before a candidate is accepted; the diagnostic table does not add candidates or alter the 148/147 counts.")
            lines.append("")
            lines.append("| corpus | manifest path | input SHA-256 | complete error string |")
            lines.append("| --- | --- | --- | --- |")
            for item in diagnostics:
                lines.append("| `%s` | `%s` | `%s` | `%s` |" % (
                    item.get("corpus"), item.get("manifest_path"),
                    item.get("input_sha256"), item.get("error")))
            if not diagnostics:
                lines.append("| — | — | — | no refusal diagnostics were emitted |")
            lines.append("")
            failure_details = block.get("exact_failure_diagnostics", {}).get("measured", [])
            lines.append("Retained exact-verification failures:")
            lines.append("")
            if failure_details:
                for item in failure_details:
                    lines.append("- `%s` (`%s`, input SHA-256 `%s`): `%s`." % (
                        item.get("manifest_path"), item.get("corpus"),
                        item.get("input_sha256"), item.get("why")))
            else:
                lines.append("- No exact-failure row was emitted; this is compared against the versioned expected set.")
            lines.append("")
        else:
            zopfli = block.get("zopfli") or {}
            aggregate = zopfli.get("aggregate") or {}
            if aggregate:
                dist = aggregate.get("distribution") or {}
                lines.append("### New Zopfli sample — `pdf-zopfli-small-v1` (not the historical sample)")
                lines.append("")
                lines.append("- Selection: %s accepted rows, %s eligible at bytes ≤ 65,536, %s sampled; selected input bytes %s." % (
                    aggregate.get("accepted_rows"), aggregate.get("eligible_rows"),
                    aggregate.get("sample_files"), aggregate.get("selected_bytes")))
                lines.append("- Membership digests: path `%s`; path+input-SHA `%s`; sample-manifest SHA-256 `%s`; result-JSONL SHA-256 `%s`." % (
                    aggregate.get("sample_path_sha256"), aggregate.get("sample_identity_sha256"),
                    aggregate.get("sample_manifest_sha256"), aggregate.get("results_jsonl_sha256")))
                lines.append("- Streams: %s roundtrips; errors %s; positive/zero/negative rows %s/%s/%s." % (
                    aggregate.get("roundtrip_streams"), aggregate.get("errors"),
                    aggregate.get("positive_rows"), aggregate.get("zero_rows"),
                    aggregate.get("negative_rows")))
                lines.append("- Per-file gain distribution, retaining instrument precision: median `%s`, Q1 `%s`, Q3 `%s`, mean `%s`, positive share `%s%%`, maximum `%s%%`." % (
                    dist.get("median"), dist.get("q1"), dist.get("q3"), dist.get("mean"),
                    dist.get("share_positive"), dist.get("maximum")))
                lines.append("- Stream-byte totals: Flate `%s`; Zopfli encoded `%s`; per-stream-minimum `%s`." % (
                    aggregate.get("total_flate_bytes"), aggregate.get("total_zopfli_encoded_bytes"),
                    aggregate.get("total_zopfli_after")))
                lines.append("- All 150 ordered paths/input hashes and exact per-file results are versioned in `%s` and `%s`; the run's measured copies are `pdf-zopfli-small-v1-sample.json` and `pdf-zopfli-small-v1-results.jsonl` in the evidence bundle." % (
                    os.path.relpath(SAMPLE_BASELINE_PATH, os.path.dirname(path)),
                    os.path.relpath(RESULTS_BASELINE_PATH, os.path.dirname(path))))
                lines.append("- This is a new sample, not a reproduction of the old unavailable 150-file sample; the historical 0.15/0.14/2.81/91% cells and red run remain preserved as errata.")
                lines.append("")
    details = os.path.join(os.path.dirname(path), "metric-definitions.md")
    if os.path.exists(details):
        lines.append("## Source-row definitions for the disputed metrics")
        lines.append("")
        with open(details, encoding="utf-8") as handle:
            lines.extend(handle.read().rstrip().splitlines())
        lines.append("")
    lines.append("## What this does not prove")
    lines.append("")
    lines.append(
        "The archive hashes above are the first recorded baseline for those codeload URLs; the run "
        "verifies that the bytes match *that* baseline and nothing about what the original audits "
        "downloaded. The corpus is still the four FITS test-data sets and three PDF corpora — no "
        "archive survey FITS product and no PDF from the toolchain that produced the published PDFs "
        "— so the open gaps in §8.1 and §9.1 are unchanged, and the eight-per-cent and fifteen-per-cent "
        "figures are the old thresholds reported, not a new pre-registration. This comparison does not "
        "close #57, #59, full-format admission, or any release gate."
    )
    lines.append("")
    with open(path, "w", encoding="utf-8") as handle:
        handle.write("\n".join(lines))

def _relative(path, root):
    try:
        return os.path.relpath(path, root)
    except (TypeError, ValueError):
        return path


def _path_digest(paths):
    digest = hashlib.sha256()
    for path in paths:
        digest.update(path.encode("utf-8") + b"\0")
    return digest.hexdigest()


def pdf_path_order_audit(pdf_rows, render_rows, root, render_sample=140):
    """Prove input/accepted order matches the original sorted-glob instrument."""
    by_corpus = {}
    corpus_order = []
    for row in pdf_rows:
        corpus = row.get("corpus", "")
        if corpus not in by_corpus:
            by_corpus[corpus] = []
            corpus_order.append(corpus)
        path = row.get("manifest_path") or _relative(row.get("path", ""), root)
        by_corpus[corpus].append((path, row))

    corpus_records = {}
    all_sorted = True
    accepted_sorted = True
    for corpus, entries in by_corpus.items():
        paths = [path for path, _row in entries]
        accepted_paths = [path for path, row in entries if row.get("status") == "accepted"]
        input_sorted = paths == sorted(paths)
        accepted_in_order = accepted_paths == sorted(accepted_paths)
        all_sorted = all_sorted and input_sorted
        accepted_sorted = accepted_sorted and accepted_in_order
        corpus_records[corpus] = dict(
            files=len(paths),
            input_paths_sorted=input_sorted,
            first_path=paths[0] if paths else None,
            last_path=paths[-1] if paths else None,
            input_path_sha256=_path_digest(paths),
            accepted=len(accepted_paths),
            accepted_paths_sorted=accepted_in_order,
            accepted_first_path=accepted_paths[0] if accepted_paths else None,
            accepted_last_path=accepted_paths[-1] if accepted_paths else None,
            accepted_path_sha256=_path_digest(accepted_paths),
        )

    expected_corpus_order = ["pdfjs", "qpdf", "verapdf"] if pdf_rows else []
    corpus_order_matches = corpus_order == expected_corpus_order
    accepted_rows = [row for row in pdf_rows if row.get("status") == "accepted"]
    random.seed(57)
    expected_sample = random.sample(accepted_rows, min(render_sample, len(accepted_rows)))
    expected_sample_paths = [
        row.get("manifest_path") or _relative(row.get("path", ""), root)
        for row in expected_sample
    ]
    actual_sample_paths = [row.get("relative_path", "") for row in render_rows]
    sample_matches = actual_sample_paths == expected_sample_paths

    return dict(
        original_instrument="per-corpus sorted(glob.glob(...)) then preserve accepted-row order",
        manifest_rows_sorted_by_path=all_sorted,
        accepted_rows_sorted_by_path=accepted_sorted,
        corpus_order=corpus_order,
        expected_corpus_order=expected_corpus_order,
        corpus_order_matches=corpus_order_matches,
        corpora=corpus_records,
        render_sample_seed=57,
        render_sample_selected=len(actual_sample_paths),
        render_sample_paths_match_seeded_accepted_rows=sample_matches,
        render_sample_paths_sha256=_path_digest(actual_sample_paths),
        expected_render_sample_paths_sha256=_path_digest(expected_sample_paths),
        render_sample_first_path=actual_sample_paths[0] if actual_sample_paths else None,
        render_sample_last_path=actual_sample_paths[-1] if actual_sample_paths else None,
        ok=all_sorted and accepted_sorted and corpus_order_matches and sample_matches,
    )


def write_metric_sources(out, root, fits_rows, pdf_rows, render_rows, render_sample=140):
    """Emit row-level inputs to every disputed aggregate in the evidence bundle."""
    fits_source = []
    for row in fits_rows:
        if row.get("corpus") not in ("astropy", "reproject") or row.get("status") != "tier2":
            continue
        fits_source.append(dict(
            metric="fits_corpus_median",
            corpus=row["corpus"],
            path=_relative(row["path"], root),
            manifest_sha256=row.get("manifest_sha256"),
            status=row.get("status"),
            included_in_median=bool(row.get("exact")),
            exact=row.get("exact"),
            reason=row.get("why"),
            bytes=row.get("bytes"),
            new_size=row.get("new_size"),
            pct=row.get("pct"),
        ))
    with open(os.path.join(out, "fits-corpus-median-source.jsonl"), "w", encoding="utf-8") as handle:
        for row in fits_source:
            handle.write(json.dumps(row, sort_keys=True) + "\n")

    pdf_source = []
    corpus_positions = {}
    accepted_positions = {}
    for jsonl_order, row in enumerate(pdf_rows, start=1):
        corpus = row.get("corpus")
        corpus_position = corpus_positions.get(corpus, 0) + 1
        corpus_positions[corpus] = corpus_position
        if row.get("status") != "accepted":
            continue
        accepted_position = accepted_positions.get(corpus, 0) + 1
        accepted_positions[corpus] = accepted_position
        pdf_source.append(dict(
            metric="pdf_share_positive",
            corpus=corpus,
            path=row.get("manifest_path") or _relative(row["path"], root),
            jsonl_order=jsonl_order,
            corpus_path_order=corpus_position,
            accepted_path_order=accepted_position,
            manifest_sha256=row.get("manifest_sha256"),
            bytes=row.get("bytes"),
            status=row.get("status"),
            streams=row.get("streams"),
            flate_bytes=row.get("flate_bytes"),
            flate_after=row.get("flate_after"),
            faithful_pct=row.get("faithful_pct"),
            positive=bool(row.get("faithful_pct", 0) > 0),
        ))
    with open(os.path.join(out, "pdf-share-positive-source.jsonl"), "w", encoding="utf-8") as handle:
        for row in pdf_source:
            handle.write(json.dumps(row, sort_keys=True) + "\n")

    order_audit = pdf_path_order_audit(pdf_rows, render_rows, root, render_sample)
    with open(os.path.join(out, "pdf-order-verification.json"), "w", encoding="utf-8") as handle:
        json.dump(order_audit, handle, indent=2, sort_keys=True)
        handle.write("\n")

    # Preserve the 140 selected records and per-page hashes exactly as emitted by
    # pdf-measure.py; the file itself is `render-sample.jsonl` in this bundle.
    formulas = [
        "# Metric definitions and source rows", "",
        "The active comparison target is versioned as `measure-repro-2026-10-31-v2`; the pre-v2 published table and its red hosted run remain explicit historical errata. The original audit bundles did not preserve per-file JSONL or the old render-sample list, so those historical source rows cannot be reconstructed from rounded table entries alone.", "",
        "## Published-table comparison", "",
        "Counts and refusal/status maps compare exactly. Displayed percentages and byte totals are rounded half-up to the number of decimal places printed in the specific published cell; there is no global 0.051 tolerance. Whole-percent shares compare after rounding to an integer percentage. Corpus rows also compare the repository and displayed commit prefix. Byte rows compare selected uncompressed file bytes from the new manifest in the audit's displayed unit, not codeload archive sizes.", "",
        "The FITS Tier 1 row is recomputed for exact-verified Tier 2 rows using each row's `tier1_pct`; its denominator is the number of those exact rows with a Tier 1 value. FITS `tier2_candidates` is the full writer-candidate count; `tier2_files` is the exact-verified subset used by the table. The two exact-failure identities and all refused counts remain explicit.", "",
        "The historical Zopfli 150-file table remains unavailable and is not reproduced. The separate `pdf-zopfli-small-v1` is a new small-file sample with its own versioned 150-path/input-SHA manifest, exact per-file results, and strict row-by-row comparison; it does not replace or relabel the historical sample.", "",
        "## FITS median by corpus: astropy and reproject", "",
        "Per-file value: `pct = 100 * (bytes - new_size) / bytes`. Include only `status=tier2` rows with `exact=true`; the median denominator is the number of such rows in the corpus. For an even count, median is the arithmetic mean of the two middle sorted values. Quartiles use the instrument's nearest-rank indices `int(p*n)`.", "",
        "The earlier re-run used the upper middle element for even counts. That is a different definition, not a change in input data. The source rows in `fits-corpus-median-source.jsonl` include all Tier 2 rows from these two corpora, including failed-verification rows (marked `included_in_median=false`).", "",
        "## PDF share with a positive faithful recompression", "",
        "For each accepted PDF, `faithful_pct = 100 * (F - F9) / S`, where `S` is whole-file bytes, `F` is the sum of original encoded bytes of in-scope Flate streams, and `F9` is the sum over those streams of `min(original_encoded_length, zlib_level_9_length(inflated_payload))`. A file is positive exactly when `faithful_pct > 0`. `pdf-share-positive-source.jsonl` contains all accepted rows, with the positive flag; `pdf.jsonl` retains all accepted, already-optimal and refused rows.", "",
        "## PDF render-page count", "",
        "Selection: manifest entries are explicitly sorted by path within each corpus, matching the original `sorted(glob.glob(...))` input order; accepted rows preserve that order. Then `random.seed(57)` and `random.sample(accepted_rows, min(140, len(accepted_rows)))` select the render sample. `pdf-order-verification.json` records the per-corpus input/accepted order checks, path digests, and whether the exact render path sequence matches the seeded selection. `pdf-share-positive-source.jsonl` carries global JSONL, per-corpus path, and accepted-row order numbers. Original and allowed-recompression candidate are rendered by PyMuPDF at 100 dpi; at most the first 6 pages per document are compared. `render_pages` is the sum of `compared_pages` for successfully rendered sample files, not the sum of each document's full page count. `render-sample.jsonl` records all selected paths, manifest hashes, original/candidate page totals, compared-page counts, page-image SHA-256 lists and equality result. Render failures are retained and make the run red.", "",
        "## PDF path-order verification", "",
        "The original tool used sorted paths within each corpus. This run checks that every corpus block and its accepted subset are still in that order; `pdf-order-verification.json` contains counts, first/last paths, and SHA-256 digests of each ordered path sequence. The accepted source rows also carry 1-based `jsonl_order`, `corpus_path_order`, and `accepted_path_order` fields.", "",
        "- Corpus order: `%s`; expected original order: `%s`; match: `%s`." % (
            ", ".join(order_audit["corpus_order"]),
            ", ".join(order_audit["expected_corpus_order"]),
            order_audit["corpus_order_matches"]),
        "- All per-corpus rows sorted: `%s`; accepted subsets sorted: `%s`." % (
            order_audit["manifest_rows_sorted_by_path"],
            order_audit["accepted_rows_sorted_by_path"]),
        "- Exact seeded render paths match accepted-row order: `%s`; sample path-sequence SHA-256: `%s`." % (
            order_audit["render_sample_paths_match_seeded_accepted_rows"],
            order_audit["render_sample_paths_sha256"]),
        "",
        "",
    ]
    for corpus, order in order_audit["corpora"].items():
        formulas.append(
            "- `%s`: %d rows, sorted=%s, %d accepted sorted=%s; accepted path SHA-256 `%s`." % (
                corpus, order["files"], order["input_paths_sorted"],
                order["accepted"], order["accepted_paths_sorted"],
                order["accepted_path_sha256"])
        )
    formulas += ["", "## Current source counts and medians", ""]
    from collections import defaultdict
    by_corpus = defaultdict(list)
    for row in fits_source:
        if row["included_in_median"]:
            by_corpus[row["corpus"]].append(row)
    for corpus in ("astropy", "reproject"):
        ordered = sorted(by_corpus[corpus], key=lambda row: row["pct"])
        n = len(ordered)
        if n % 2:
            center = [ordered[n // 2]]
            med = center[0]["pct"]
            center_text = "row %d = %.9f%%" % (n // 2 + 1, med)
        else:
            center = ordered[n // 2 - 1:n // 2 + 1]
            med = sum(row["pct"] for row in center) / 2
            center_text = "rows %d–%d = %.9f%% and %.9f%%" % (
                n // 2, n // 2 + 1, center[0]["pct"], center[1]["pct"])
        formulas.append("- `%s`: n=%d; median %.9f%% (%s)." % (corpus, n, med, center_text))
        for row in center:
            formulas.append("  - center source: `%s`, %d → %d bytes, SHA-256 `%s`." % (
                row["path"], row["bytes"], row["new_size"], row["manifest_sha256"]))
    positive = sum(1 for row in pdf_source if row["positive"])
    denominator = len(pdf_source)
    zero = sum(1 for row in pdf_source if row["faithful_pct"] == 0)
    formulas += ["", "## Current PDF source counts", "",
                 "- Positive accepted PDFs: %d / %d = %.9f%%; exactly zero: %d; negative: %d." % (
                     positive, denominator, 100.0 * positive / denominator if denominator else 0,
                     zero, sum(1 for row in pdf_source if row["faithful_pct"] < 0)),
                 "- Positive and zero/negative source rows are listed in `pdf-share-positive-source.jsonl` (one record per accepted input)."]
    selected = len(render_rows)
    rendered = [row for row in render_rows if row.get("rendered")]
    formulas += ["", "## Current render sample counts", "",
                 "- Selected: %d; rendered: %d; compared pages: %d; differences: %d; errors: %d." % (
                     selected, len(rendered),
                     sum(row.get("compared_pages", 0) for row in rendered),
                     sum(1 for row in rendered if not row.get("identical")),
                     sum(1 for row in render_rows if row.get("render_error"))),
                 "- Prior aggregate 173 has no preserved old sample path list, so it cannot be asserted to refer to these same 140 documents/pages."]
    with open(os.path.join(out, "metric-definitions.md"), "w", encoding="utf-8") as handle:
        handle.write("\n".join(formulas) + "\n")
    return order_audit


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
            "A run is compared with one disclosed, versioned baseline. A green result means that baseline "
            "reproduced; it does not mean zero known failures or full FITS/PDF admission."
        ),
        baseline=dict(
            baseline_id=BASELINE_ID,
            provenance=BASELINE["provenance"],
            historical_errata=BASELINE["historical_errata"],
        ),
        manifest_path=manifest,
        manifest_digest=recorded["manifest_digest"],
        manifest_files=sum(c["files"] for c in recorded["corpora"]),
        corpora=[
            dict(name=c["name"], format=c["format"], repo=c["repo"], commit=c["commit"],
                 archive=c["archive"], archive_sha256=c["archive_sha256"],
                 files=c["files"], bytes=c["bytes"], excluded=len(c.get("excluded") or []))
            for c in recorded["corpora"]
        ],
        environment=environment(),
        commands=[],
        problems=[],
    )
    if recorded.get("manifest_digest") != BASELINE["manifest_digest"]:
        payload["problems"].append(
            "source manifest digest differs from baseline %s: expected %s, measured %s"
            % (BASELINE_ID, BASELINE["manifest_digest"], recorded.get("manifest_digest"))
        )

    input_comparison = {}
    for family in ("fits", "pdf"):
        corpus_rows, corpus_problems = compare_input_corpora(payload["corpora"], family)
        input_comparison[family] = dict(rows=corpus_rows, problems=corpus_problems)
        payload["problems"].extend("%s input corpus: %s" % (family, problem)
                                  for problem in corpus_problems)
    payload["input_corpus_comparison"] = input_comparison

    source_rows = {"fits": [], "pdf": []}
    render_rows = []
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
        render_jsonl = os.path.join(args.out, "render-sample.jsonl")
        if name == "pdf":
            argv += ["--render-sample", str(args.render_sample), "--render-out", render_jsonl]
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
        source_rows[name] = rows
        aggregates = fits_aggregates(rows) if name == "fits" else pdf_aggregates(rows)
        if name == "pdf":
            if not os.path.exists(render_jsonl):
                payload["problems"].append("pdf-measure.py did not produce render-sample.jsonl")
                render_rows = []
            else:
                render_rows = load_jsonl(render_jsonl)
            from_log = render_totals(completed.stdout)
            from_rows = render_aggregates(render_rows)
            if any(from_log.get(key) != from_rows.get(key)
                   for key in ("render_files", "render_pages", "render_differences")):
                payload["problems"].append(
                    "render stdout totals disagree with per-file source rows: %s vs %s"
                    % (from_log, from_rows)
                )
            aggregates.update(from_rows)
        comparison, problems = compare(
            aggregates, PUBLISHED[name], precision=PUBLISHED_PRECISION[name]
        )
        fits_refusal_check = None
        fits_failure_check = None
        if name == "fits":
            fits_refusal_check, refusal_problems = compare_fits_refusal_diagnostics(rows, args.root)
            fits_failure_check, failure_problems = compare_fits_exact_failure_diagnostics(rows, args.root)
            problems.extend(refusal_problems)
            problems.extend(failure_problems)
            comparison.extend([
                dict(metric="fits.cannot_compress_diagnostics", published=5,
                     measured=len(fits_refusal_check["measured"]),
                     verdict=fits_refusal_check["verdict"]),
                dict(metric="fits.exact_failure_diagnostics", published=1,
                     measured=len(fits_failure_check["measured"]),
                     verdict=fits_failure_check["verdict"]),
            ])
        zopfli_result = None
        if name == "pdf":
            zopfli_argv = [
                sys.executable,
                os.path.join(HERE, "pdf-zopfli-measure.py"),
                "--root", args.root,
                "--manifest", manifest,
                "--pdf-jsonl", jsonl,
                "--out", args.out,
            ]
            payload["commands"].append(" ".join(zopfli_argv))
            zopfli_completed, zopfli_elapsed = run(
                zopfli_argv, os.path.join(args.out, "pdf-zopfli-small-v1-stdout.txt")
            )
            zopfli_comparison_path = os.path.join(
                args.out, "pdf-zopfli-small-v1-comparison.json"
            )
            if os.path.exists(zopfli_comparison_path):
                with open(zopfli_comparison_path, encoding="utf-8") as handle:
                    zopfli_result = json.load(handle)
                comparison.extend(zopfli_result.get("comparison", []))
                problems.extend(zopfli_result.get("problems", []))
            else:
                zopfli_problem = "pdf-zopfli-measure.py produced no comparison JSON"
                problems.append(zopfli_problem)
            if zopfli_completed.returncode != 0 and not (zopfli_result or {}).get("problems"):
                problems.append("pdf-zopfli-measure.py exited %d" % zopfli_completed.returncode)
        payload[name] = dict(
            aggregates=aggregates,
            comparison=comparison,
            problems=problems,
            elapsed_seconds=round(elapsed, 1),
            rows=len(rows),
        )
        if name == "fits":
            payload[name]["refusal_diagnostics"] = fits_refusal_check
            payload[name]["exact_failure_diagnostics"] = fits_failure_check
        if name == "pdf" and zopfli_result is not None:
            payload[name]["zopfli"] = dict(
                aggregate=zopfli_result.get("aggregate"),
                comparison=zopfli_result.get("comparison", []),
                problems=zopfli_result.get("problems", []),
                sample_manifest=zopfli_result.get("sample_manifest"),
            )
            payload[name]["zopfli_elapsed_seconds"] = round(zopfli_elapsed, 1)
        payload["problems"].extend("%s: %s" % (name, problem) for problem in problems)
        if name == "pdf":
            if aggregates.get("render_errors"):
                render_problem = "%d of %d selected render files could not be compared" % (
                    aggregates["render_errors"], aggregates.get("render_sampled_files", 0))
                payload["pdf"]["problems"].append(render_problem)
                payload["problems"].append("pdf: " + render_problem)
            if aggregates.get("render_sampled_files") != min(args.render_sample, aggregates.get("accepted", 0)):
                render_problem = "render sample has %d records; expected %d" % (
                    aggregates.get("render_sampled_files", 0),
                    min(args.render_sample, aggregates.get("accepted", 0)))
                payload["pdf"]["problems"].append(render_problem)
                payload["problems"].append("pdf: " + render_problem)

    payload["elapsed_seconds"] = round(time.time() - started, 1)
    order_audit = write_metric_sources(
        args.out, args.root, source_rows["fits"], source_rows["pdf"], render_rows, args.render_sample
    )
    if source_rows["pdf"]:
        payload["pdf"]["path_order"] = order_audit
        if not order_audit["ok"]:
            problem = "PDF input/accepted path order or seeded render selection differs from the original instrument"
            payload["pdf"]["problems"].append(problem)
            payload["problems"].append("pdf: " + problem)
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
        print("::error::the run does not reproduce baseline %s; see summary.md" % BASELINE_ID)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
