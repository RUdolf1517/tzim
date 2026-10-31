#!/usr/bin/env python3
"""Measure the versioned `pdf-zopfli-small-v1` sample and compare every row.

This is a new, explicitly small-file sample. It is not a reproduction of the old
150-file Zopfli sample, whose membership and per-file results were not retained.
The expected sample manifest and exact per-file results are versioned beside the
measure-repro baseline. Any changed identity, row order, integer result, or codec
error makes this comparator fail.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import importlib.util
import json
import os
import random
import sys
import time
import zlib

import pikepdf
import zopfli
import zopfli.zlib as zopfli_zlib

HERE = os.path.dirname(os.path.abspath(__file__))
BASELINE_DIR = os.path.join(HERE, "measure-repro-baselines")
BASELINE_PATH = os.path.join(BASELINE_DIR, "measure-repro-2026-10-31-v2.json")
with open(BASELINE_PATH, encoding="utf-8") as handle:
    BASELINE = json.load(handle)
ZOPFLI_BASELINE = BASELINE["pdf_zopfli"]
PROTOCOL_ID = ZOPFLI_BASELINE["protocol_id"]

SAMPLE_FILE = os.path.join(BASELINE_DIR, ZOPFLI_BASELINE["sample_manifest_file"])
RESULTS_FILE = os.path.join(BASELINE_DIR, ZOPFLI_BASELINE["results_file"])
CALIBRATION_AGGREGATE_FILE = os.path.join(
    BASELINE_DIR, ZOPFLI_BASELINE["calibration_aggregate_file"]
)

ZOPFLI_OPTIONS = dict(
    verbose=0,
    numiterations=15,
    blocksplitting=1,
    blocksplittinglast=0,
    blocksplittingmax=15,
    gzip_mode=0,
)

SAMPLE_MANIFEST_TEMPLATE = {
    "disposition": (
        "new reproducible small-file comparator; not a reproduction of the unavailable "
        "historical 150-file sample"
    ),
    "metric": {
        "errors": "any open/inflate/compress/roundtrip error or missing row fails; no row substitution",
        "flate_bytes": "sum original encoded byte lengths for the same rewritable streams as faithful_pct",
        "formula": "100 * (flate_bytes - zopfli_after) / whole_file_bytes",
        "roundtrip": "zlib.decompress(candidate) must equal inflated payload for every stream",
        "zopfli_after": (
            "sum min(original_encoded_length, len(pyzopfli_zlib_compress(inflated_payload))) "
            "over those streams"
        ),
    },
    "selection": {
        "input_bytes_max_inclusive": 65536,
        "ordering": "global manifest_path ascending before sampling",
        "rng": "Python random.Random(57).sample(eligible, 150)",
        "selection_order": "random.sample return order",
    },
    "zopfli": {
        "blocksplitting": 1,
        "blocksplittinglast": 0,
        "blocksplittingmax": 15,
        "gzip_mode": 0,
        "numiterations": 15,
        "package": "zopfli",
        "verbose": 0,
        "wrapper": "zopfli.zlib.compress",
    },
}


def _load_pdf_measure():
    path = os.path.join(HERE, "pdf-measure.py")
    spec = importlib.util.spec_from_file_location("pdf_measure_for_zopfli", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load pdf-measure.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


PDF_MEASURE = _load_pdf_measure()


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def load_jsonl(path):
    rows = []
    with open(path, encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError as error:
                raise ValueError("invalid JSONL at %s:%d: %s" % (path, line_number, error))
    return rows


def path_digest(paths):
    """SHA-256 of each UTF-8 manifest path followed by LF, in sample order."""
    return sha256_bytes("".join(path + "\n" for path in paths).encode("utf-8"))


def identity_digest(rows):
    """SHA-256 of path-NUL-input-SHA-LF for each sample row, in sample order."""
    payload = "".join(
        "%s\0%s\n" % (row["manifest_path"], row["manifest_sha256"])
        for row in rows
    )
    return sha256_bytes(payload.encode("utf-8"))


def select_sample(pdf_rows, sample_size=150, max_bytes=65536, seed=57):
    """Return accepted, globally path-sorted eligible, and seeded sample rows."""
    accepted = [row for row in pdf_rows if row.get("status") == "accepted"]
    eligible = [row for row in accepted if row.get("bytes", max_bytes + 1) <= max_bytes]
    eligible.sort(key=lambda row: row.get("manifest_path", ""))
    sample = random.Random(seed).sample(eligible, min(sample_size, len(eligible)))
    return accepted, eligible, sample


def sample_manifest_row(row, order):
    return dict(
        bytes=row["bytes"],
        corpus=row["corpus"],
        flate_bytes=row["flate_bytes"],
        manifest_path=row["manifest_path"],
        manifest_sha256=row["manifest_sha256"],
        sample_order=order,
        streams=row["streams"],
    )


def build_sample_manifest(accepted, eligible, sample, manifest_digest):
    rows = [sample_manifest_row(row, order) for order, row in enumerate(sample, start=1)]
    paths = [row["manifest_path"] for row in rows]
    corpus_counts = {}
    for row in rows:
        corpus_counts[row["corpus"]] = corpus_counts.get(row["corpus"], 0) + 1
    result = dict(
        accepted_rows=len(accepted),
        disposition=SAMPLE_MANIFEST_TEMPLATE["disposition"],
        metric=SAMPLE_MANIFEST_TEMPLATE["metric"],
        protocol_id=PROTOCOL_ID,
        sample_identity_sha256=identity_digest(rows),
        sample_path_sha256=path_digest(paths),
        selected_bytes=sum(row["bytes"] for row in rows),
        selected_rows=rows,
        selection=dict(
            eligible_rows=len(eligible),
            input_bytes_max_inclusive=SAMPLE_MANIFEST_TEMPLATE["selection"]["input_bytes_max_inclusive"],
            ordering=SAMPLE_MANIFEST_TEMPLATE["selection"]["ordering"],
            rng=SAMPLE_MANIFEST_TEMPLATE["selection"]["rng"],
            sample_files=len(rows),
            selection_order=SAMPLE_MANIFEST_TEMPLATE["selection"]["selection_order"],
        ),
        source_full_manifest_sha256=manifest_digest,
        zopfli=dict(
            SAMPLE_MANIFEST_TEMPLATE["zopfli"],
            version=importlib.metadata.version("zopfli"),
        ),
    )
    return result


def _safe_input_path(root, manifest_path):
    root_real = os.path.realpath(root)
    path_real = os.path.realpath(os.path.join(root_real, manifest_path))
    if os.path.commonpath((root_real, path_real)) != root_real:
        raise ValueError("manifest path escapes the corpus root: %s" % manifest_path)
    return path_real


def measure_file(row, root, order):
    """Measure one selected PDF; return an error-bearing row rather than dropping it."""
    result = dict(
        bytes=row.get("bytes"),
        corpus=row.get("corpus"),
        error=None,
        flate_bytes=0,
        manifest_path=row.get("manifest_path"),
        manifest_sha256=row.get("manifest_sha256"),
        roundtrip_streams=0,
        sample_order=order,
        streams=0,
        zopfli_after=0,
        zopfli_encoded_bytes=0,
        zopfli_pct=None,
    )
    try:
        manifest_path = row["manifest_path"]
        path = _safe_input_path(root, manifest_path)
        if row.get("path"):
            row_path = os.path.realpath(row["path"])
            if row_path != path:
                raise ValueError("measurement row path does not match manifest path %s" % manifest_path)
        if not os.path.isfile(path):
            raise FileNotFoundError(path)
        observed_size = os.path.getsize(path)
        if observed_size != row["bytes"]:
            raise ValueError("input size %d differs from measurement row %d" % (observed_size, row["bytes"]))
        observed_sha = sha256_file(path)
        if observed_sha != row.get("manifest_sha256"):
            raise ValueError("input SHA-256 %s differs from measurement row %s" % (
                observed_sha, row.get("manifest_sha256")))

        with pikepdf.open(path, attempt_recovery=False) as pdf:
            for stream, refusal in PDF_MEASURE.rewritable_streams(pdf):
                if refusal:
                    raise ValueError("accepted PDF contains refused stream: %s" % refusal)
                if stream is None:
                    raise ValueError("rewritable-stream iterator returned no stream")
                raw = stream.read_raw_bytes()
                plain = zlib.decompress(raw)
                candidate = zopfli_zlib.compress(plain, **ZOPFLI_OPTIONS)
                if not isinstance(candidate, bytes):
                    raise TypeError("zopfli.zlib.compress returned %s, not bytes" % type(candidate).__name__)
                roundtrip = zlib.decompress(candidate)
                if roundtrip != plain:
                    raise ValueError("Zopfli stream roundtrip differs from the inflated input")
                result["streams"] += 1
                result["roundtrip_streams"] += 1
                result["flate_bytes"] += len(raw)
                result["zopfli_encoded_bytes"] += len(candidate)
                result["zopfli_after"] += min(len(raw), len(candidate))
        if result["streams"] == 0:
            raise ValueError("accepted sample PDF contained no rewritable Flate streams")
        if row.get("streams") != result["streams"]:
            raise ValueError("measured stream count %d differs from PDF instrument row %s" % (
                result["streams"], row.get("streams")))
        if row.get("flate_bytes") != result["flate_bytes"]:
            raise ValueError("measured Flate bytes %d differs from PDF instrument row %s" % (
                result["flate_bytes"], row.get("flate_bytes")))
        result["zopfli_pct"] = 100.0 * (
            result["flate_bytes"] - result["zopfli_after"]
        ) / result["bytes"]
    except Exception as error:  # every open/inflate/compress/roundtrip error is retained and fatal
        result["error"] = "%s: %s" % (type(error).__name__, error)
    return result


def percentile(sorted_values, fraction):
    if not sorted_values:
        return None
    if fraction == 0.5 and len(sorted_values) % 2 == 0:
        middle = len(sorted_values) // 2
        return (sorted_values[middle - 1] + sorted_values[middle]) / 2
    index = int(fraction * len(sorted_values))
    return sorted_values[min(index, len(sorted_values) - 1)]


def distribution(values):
    ordered = sorted(values)
    if not ordered:
        return dict(n=0)
    return dict(
        maximum=ordered[-1],
        mean=sum(ordered) / len(ordered),
        median=percentile(ordered, 0.5),
        minimum=ordered[0],
        n=len(ordered),
        q1=percentile(ordered, 0.25),
        q3=percentile(ordered, 0.75),
        share_positive=100.0 * sum(value > 0 for value in ordered) / len(ordered),
    )


def _read_expected_rows():
    expected_manifest = json.loads(open(SAMPLE_FILE, encoding="utf-8").read())
    expected_results = load_jsonl(RESULTS_FILE)
    expected_aggregate = json.loads(open(CALIBRATION_AGGREGATE_FILE, encoding="utf-8").read())
    return expected_manifest, expected_results, expected_aggregate


def _check_baseline_assets(problems):
    checks = (
        (SAMPLE_FILE, ZOPFLI_BASELINE["sample_manifest_sha256"], "sample manifest"),
        (RESULTS_FILE, ZOPFLI_BASELINE["results_jsonl_sha256"], "per-file results"),
        (CALIBRATION_AGGREGATE_FILE, ZOPFLI_BASELINE["calibration_aggregate_sha256"], "calibration aggregate"),
    )
    for path, expected, label in checks:
        try:
            measured = sha256_file(path)
        except OSError as error:
            problems.append("missing versioned Zopfli %s file: %s" % (label, error))
            continue
        if measured != expected:
            problems.append("versioned Zopfli %s SHA-256 differs: expected %s, measured %s" % (
                label, expected, measured))


def _row_differences(actual, expected):
    if actual == expected:
        return []
    keys = sorted(set(actual) | set(expected))
    return [key for key in keys if actual.get(key) != expected.get(key)]


def compare_result_rows(actual, expected):
    """Strict positional equality: no missing, extra, reordered, or substituted rows."""
    problems = []
    if len(actual) != len(expected):
        problems.append("Zopfli result row count differs: expected %d, measured %d" % (
            len(expected), len(actual)))
    for index in range(max(len(actual), len(expected))):
        actual_row = actual[index] if index < len(actual) else None
        expected_row = expected[index] if index < len(expected) else None
        if actual_row is None:
            problems.append("missing Zopfli result row %d: expected %s" % (index + 1, expected_row))
            continue
        if expected_row is None:
            problems.append("extra Zopfli result row %d: measured %s" % (index + 1, actual_row))
            continue
        if actual_row != expected_row:
            keys = _row_differences(actual_row, expected_row)
            problems.append("Zopfli result row %d differs in %s: expected %s, measured %s" % (
                index + 1, ", ".join(keys), expected_row, actual_row))
    return problems


def compare_aggregate(actual, expected):
    problems = []
    for key in sorted(expected):
        if key not in actual:
            problems.append("Zopfli aggregate is missing %s" % key)
        elif actual[key] != expected[key]:
            problems.append("Zopfli aggregate %s differs: expected %s, measured %s" % (
                key, expected[key], actual[key]))
    return problems


def _sample_manifest_differences(actual, expected):
    return _row_differences(actual, expected)


def run_protocol(root, manifest_path, pdf_jsonl_path, out_dir):
    started = time.perf_counter()
    os.makedirs(out_dir, exist_ok=True)
    problems = []
    _check_baseline_assets(problems)

    try:
        with open(manifest_path, encoding="utf-8") as handle:
            manifest = json.load(handle)
        pdf_rows = load_jsonl(pdf_jsonl_path)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        return {
            "aggregates": {},
            "comparison": [],
            "problems": ["could not load the manifest or PDF JSONL: %s: %s" % (
                type(error).__name__, error)],
            "sample_manifest": None,
            "results": [],
        }

    manifest_digest = manifest.get("manifest_digest")
    if manifest_digest != BASELINE["manifest_digest"]:
        problems.append("source manifest digest differs: expected %s, measured %s" % (
            BASELINE["manifest_digest"], manifest_digest))

    expected_manifest, expected_results, calibration_aggregate = _read_expected_rows()
    accepted, eligible, sample = select_sample(pdf_rows)
    expected_aggregate = ZOPFLI_BASELINE["expected_aggregate"]
    expected_manifest_count = expected_manifest["accepted_rows"]
    expected_accepted = expected_manifest_count
    expected_eligible = expected_manifest["selection"]["eligible_rows"]
    expected_sample_size = expected_manifest["selection"]["sample_files"]
    if len(accepted) != expected_accepted:
        problems.append("accepted PDF row count differs: expected %d, measured %d" % (
            expected_accepted, len(accepted)))
    accepted_sorted = sorted(accepted, key=lambda row: row.get("manifest_path", ""))
    try:
        accepted_identity = identity_digest(accepted_sorted)
    except (KeyError, TypeError) as error:
        accepted_identity = None
        problems.append("accepted PDF identities are incomplete: %s" % error)
    expected_accepted_identity = ZOPFLI_BASELINE["accepted_identity_sha256"]
    if accepted_identity != expected_accepted_identity:
        problems.append("accepted PDF path/input-SHA identity digest differs: expected %s, measured %s" % (
            expected_accepted_identity, accepted_identity))
    if len(eligible) != expected_eligible:
        problems.append("eligible small PDF row count differs: expected %d, measured %d" % (
            expected_eligible, len(eligible)))
    if len(sample) != expected_sample_size:
        problems.append("sample row count differs: expected %d, measured %d" % (
            expected_sample_size, len(sample)))

    accepted_paths = [row.get("manifest_path") for row in accepted]
    eligible_paths = [row.get("manifest_path") for row in eligible]
    if any(not path for path in accepted_paths):
        problems.append("an accepted PDF row is missing its manifest_path")
    if len(accepted_paths) != len(set(accepted_paths)):
        problems.append("accepted PDF rows contain duplicate manifest paths")
    if eligible_paths != sorted(eligible_paths):
        problems.append("eligible PDF paths are not globally sorted before sampling")

    try:
        sample_manifest = build_sample_manifest(accepted, eligible, sample, manifest_digest)
    except (KeyError, TypeError, ValueError) as error:
        problems.append("could not build the Zopfli sample manifest: %s: %s" % (
            type(error).__name__, error))
        sample_manifest = {
            "protocol_id": PROTOCOL_ID,
            "accepted_rows": len(accepted),
            "eligible_rows": len(eligible),
            "selected_rows": [],
            "error": "%s: %s" % (type(error).__name__, error),
        }
    manifest_diffs = _sample_manifest_differences(sample_manifest, expected_manifest)
    sample_manifest_verdict = "reproduced" if not manifest_diffs else "DIFFERENT"
    if manifest_diffs:
        problems.append("sample manifest differs from versioned %s: %s" % (
            PROTOCOL_ID, ", ".join(manifest_diffs)))

    result_rows = []
    for order, row in enumerate(sample, start=1):
        result_rows.append(measure_file(row, root, order))
    for row in result_rows:
        if row.get("error"):
            problems.append("Zopfli measurement error at sample row %s (%s): %s" % (
                row.get("sample_order"), row.get("manifest_path"), row["error"]))

    result_problems = compare_result_rows(result_rows, expected_results)
    if result_problems:
        problems.extend(result_problems)

    result_path = os.path.join(out_dir, "pdf-zopfli-small-v1-results.jsonl")
    with open(result_path, "w", encoding="utf-8") as handle:
        for row in result_rows:
            handle.write(json.dumps(row) + "\n")
    sample_manifest_path = os.path.join(out_dir, "pdf-zopfli-small-v1-sample.json")
    with open(sample_manifest_path, "w", encoding="utf-8") as handle:
        json.dump(sample_manifest, handle, indent=2, sort_keys=True)
        handle.write("\n")

    percentages = [row["zopfli_pct"] for row in result_rows if row.get("error") is None and row.get("zopfli_pct") is not None]
    result_errors = sum(1 for row in result_rows if row.get("error"))
    distribution_values = distribution(percentages)
    positive = sum(value > 0 for value in percentages)
    zero = sum(value == 0 for value in percentages)
    negative = sum(value < 0 for value in percentages)
    aggregate = dict(
        accepted_identity_sha256=accepted_identity,
        accepted_rows=len(accepted),
        blocksplitting=ZOPFLI_OPTIONS["blocksplitting"],
        blocksplittinglast=ZOPFLI_OPTIONS["blocksplittinglast"],
        blocksplittingmax=ZOPFLI_OPTIONS["blocksplittingmax"],
        distribution=distribution_values,
        eligible_rows=len(eligible),
        errors=result_errors,
        gzip_mode=ZOPFLI_OPTIONS["gzip_mode"],
        numiterations=ZOPFLI_OPTIONS["numiterations"],
        negative_rows=negative,
        package="zopfli",
        positive_rows=positive,
        protocol_id=PROTOCOL_ID,
        results_jsonl_sha256=sha256_file(result_path),
        roundtrip_streams=sum(row.get("roundtrip_streams", 0) for row in result_rows),
        sample_files=len(result_rows),
        sample_identity_sha256=sample_manifest.get("sample_identity_sha256"),
        sample_manifest_sha256=sha256_file(sample_manifest_path),
        sample_path_sha256=sample_manifest.get("sample_path_sha256"),
        selected_bytes=sample_manifest.get("selected_bytes"),
        source_pdf_jsonl_sha256=sha256_file(pdf_jsonl_path),
        total_flate_bytes=sum(row.get("flate_bytes", 0) for row in result_rows),
        total_zopfli_after=sum(row.get("zopfli_after", 0) for row in result_rows),
        total_zopfli_encoded_bytes=sum(row.get("zopfli_encoded_bytes", 0) for row in result_rows),
        verbose=ZOPFLI_OPTIONS["verbose"],
        zero_rows=zero,
        version=importlib.metadata.version("zopfli"),
        wrapper="zopfli.zlib.compress",
    )
    aggregate["elapsed_seconds"] = round(time.perf_counter() - started, 6)
    aggregate_path = os.path.join(out_dir, "pdf-zopfli-small-v1-aggregate.json")
    with open(aggregate_path, "w", encoding="utf-8") as handle:
        json.dump(aggregate, handle, indent=2, sort_keys=True)
        handle.write("\n")

    aggregate_problems = compare_aggregate(aggregate, expected_aggregate)
    if aggregate_problems:
        problems.extend(aggregate_problems)
    expected_calibration = {
        key: value for key, value in calibration_aggregate.items()
        if key not in ("elapsed_seconds", "source_pdf_jsonl_sha256")
    }
    if expected_calibration != expected_aggregate:
        problems.append("versioned calibration aggregate and baseline metadata disagree")
    if aggregate["accepted_rows"] != expected_manifest_count:
        problems.append("sample manifest accepted_rows differs from the required 1009-row set")
    if aggregate["accepted_identity_sha256"] != ZOPFLI_BASELINE["accepted_identity_sha256"]:
        problems.append("the exact accepted PDF row identities do not reproduce")
    if aggregate["sample_manifest_sha256"] != ZOPFLI_BASELINE["sample_manifest_sha256"]:
        problems.append("generated pre-gain sample manifest SHA-256 differs")
    if aggregate["results_jsonl_sha256"] != ZOPFLI_BASELINE["results_jsonl_sha256"]:
        problems.append("generated per-file results JSONL SHA-256 differs")

    actual_count = len(result_rows)
    comparison = []
    compared_aggregate = dict(aggregate)
    compared_aggregate["sample_manifest_matches"] = sample_manifest_verdict == "reproduced"
    compared_aggregate["result_rows_exact"] = not result_problems
    compared_aggregate["positive_rows"] = positive
    compared_aggregate["zero_rows"] = zero
    compared_aggregate["negative_rows"] = negative
    expected_comparison = dict(expected_aggregate)
    expected_comparison["accepted_rows"] = expected_manifest_count
    expected_comparison["accepted_identity_sha256"] = ZOPFLI_BASELINE["accepted_identity_sha256"]
    expected_comparison["sample_manifest_matches"] = True
    expected_comparison["result_rows_exact"] = True
    expected_comparison.update(ZOPFLI_BASELINE["expected_counts"])
    for metric in sorted(expected_comparison):
        got = compared_aggregate.get(metric)
        want = expected_comparison[metric]
        verdict = "reproduced" if got == want else "DIFFERENT"
        comparison.append(dict(metric="zopfli.%s" % metric, published=want, measured=got, verdict=verdict))
    comparison.append(dict(
        metric="zopfli.sample_manifest_rows",
        published=len(expected_manifest.get("selected_rows", [])),
        measured=len(sample_manifest.get("selected_rows", [])),
        verdict="reproduced" if sample_manifest_verdict == "reproduced" else "DIFFERENT",
    ))
    comparison.append(dict(
        metric="zopfli.per_file_rows_exact",
        published=len(expected_results),
        measured=actual_count,
        verdict="reproduced" if not result_problems else "DIFFERENT",
    ))

    comparison_payload = dict(
        baseline_id=BASELINE["baseline_id"],
        protocol_id=PROTOCOL_ID,
        aggregate=aggregate,
        comparison=comparison,
        problems=problems,
        sample_manifest=sample_manifest,
        result_rows=result_rows,
    )
    comparison_path = os.path.join(out_dir, "pdf-zopfli-small-v1-comparison.json")
    with open(comparison_path, "w", encoding="utf-8") as handle:
        json.dump(comparison_payload, handle, indent=2, sort_keys=True)
        handle.write("\n")
    return comparison_payload


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", required=True, help="verified measure-fetch corpus root")
    parser.add_argument("--manifest", required=True, help="verified 2,386-row corpus manifest")
    parser.add_argument("--pdf-jsonl", required=True, help="all rows emitted by pdf-measure.py")
    parser.add_argument("--out", required=True, help="evidence output directory")
    args = parser.parse_args()
    payload = run_protocol(args.root, args.manifest, args.pdf_jsonl, args.out)
    aggregate = payload.get("aggregate", {})
    distribution_values = aggregate.get("distribution", {})
    print(
        "%s: accepted=%s eligible=%s sample=%s, roundtrips=%s, errors=%s, mean=%s, positive=%s%%"
        % (
            PROTOCOL_ID,
            aggregate.get("accepted_rows"),
            aggregate.get("eligible_rows"),
            aggregate.get("sample_files"),
            aggregate.get("roundtrip_streams"),
            aggregate.get("errors"),
            distribution_values.get("mean"),
            distribution_values.get("share_positive"),
        )
    )
    for problem in payload.get("problems", []):
        print("!! %s" % problem)
    return 1 if payload.get("problems") else 0


if __name__ == "__main__":
    sys.exit(main())
