#!/usr/bin/env python3
"""Independent recomputation of the fixed #57 PDF aggregate and threshold from raw rows."""

import argparse
import json
import statistics
from collections import Counter
from pathlib import Path


def quantile(values, numerator, denominator):
    if not values:
        return None
    ordered = sorted(values)
    return ordered[len(ordered) * numerator // denominator]


def instrument(values):
    return {
        "n": len(values),
        "median_upper": quantile(values, 1, 2),
        "q1_index": quantile(values, 1, 4),
        "q3_index": quantile(values, 3, 4),
        "share_positive": sum(value > 0 for value in values) / len(values) if values else None,
    }


def threshold_disposition(rows, recomputed, feature_delta_bytes=None):
    accepted_n = recomputed["accepted_n"]
    row_count = recomputed["row_count"]
    faithful = recomputed["faithful_pct"]
    accepted_share = 100.0 * accepted_n / row_count if row_count else None
    blockers = []
    for key, label in (
        ("faithful_pct", "accepted-subset faithful_pct"),
        ("qpdf_allowed_pct", "permitted-qpdf distribution"),
        ("forbidden_repack_pct", "forbidden-repack distribution"),
    ):
        if recomputed[key]["n"] != accepted_n:
            blockers.append(f"{label} has {recomputed[key]['n']} values for {accepted_n} accepted rows")
    if recomputed["render_differences"]:
        blockers.append(f"{recomputed['render_differences']} registered render comparisons differ")
    if recomputed["qpdf_source_check_failures"]:
        blockers.append(f"{recomputed['qpdf_source_check_failures']} accepted source PDFs failed qpdf --check")
    if recomputed["qpdf_candidate_check_failures"]:
        blockers.append(f"{recomputed['qpdf_candidate_check_failures']} qpdf candidate checks failed")
    if len(rows) != 21:
        blockers.append(f"expected all 21 frozen outputs, found {len(rows)}")
    if blockers:
        return {
            "status": "not_applied",
            "decision": "threshold not applied",
            "reason": "registered measurements or required checks are incomplete",
            "blockers": blockers,
            "metric": "faithful_pct (flate2 level 9, as registered in the original accepted-subset rule)",
            "accepted_share_of_corpus_pct": accepted_share,
            "faithful_median_pct": faithful["median_upper"],
            "pdf_feature_delta_bytes": feature_delta_bytes,
        }
    median = faithful["median_upper"]
    if median is None:
        return {
            "status": "not_applied",
            "decision": "threshold not applied",
            "reason": "the accepted subset has no faithful_pct median",
            "blockers": ["accepted subset is empty"],
            "metric": "faithful_pct",
            "accepted_share_of_corpus_pct": accepted_share,
            "faithful_median_pct": None,
            "pdf_feature_delta_bytes": feature_delta_bytes,
        }
    if median >= 3.0 and accepted_share is not None and accepted_share >= 10.0:
        status = "applied"
        decision = "threshold clears: PDF contract is a candidate for an opt-in pdf feature"
        rule_result = "admitted"
        reason = "faithful_pct median is at least 3% and at least 10% of the frozen corpus is accepted"
    elif median < 1.0:
        status = "applied"
        decision = "refused after measurement"
        rule_result = "refused"
        reason = "accepted-subset faithful_pct median is below 1%"
    elif 1.0 <= median < 3.0:
        if feature_delta_bytes is None:
            status = "not_applied"
            decision = "threshold needs the registered binary-delta comparison"
            rule_result = "pending_binary_delta"
            reason = "faithful_pct median is between 1% and 3%; no final-head PDF feature delta was supplied"
        elif feature_delta_bytes > 70 * 1024:
            status = "applied"
            decision = "refused after measurement"
            rule_result = "refused"
            reason = "the registered mid-band rule refuses a PDF feature delta above 70 KiB"
        else:
            status = "applied"
            decision = "threshold clears: PDF contract is a candidate for an opt-in pdf feature"
            rule_result = "admitted"
            reason = "the registered mid-band rule accepts a PDF feature delta at or below 70 KiB"
    else:
        status = "applied"
        decision = "admission threshold not met because less than 10% of the corpus is accepted"
        rule_result = "refused"
        reason = "median is at least 3% but the accepted share is below the registered 10% bar"
    return {
        "status": status,
        "decision": decision,
        "rule_result": rule_result,
        "reason": reason,
        "blockers": [],
        "metric": "faithful_pct (flate2 level 9, as registered in the original accepted-subset rule)",
        "accepted_n": accepted_n,
        "row_count": row_count,
        "accepted_share_of_corpus_pct": accepted_share,
        "faithful_median_pct": median,
        "pdf_feature_delta_bytes": feature_delta_bytes,
        "thresholds": {
            "median_at_least_for_admission_pct": 3.0,
            "accepted_share_at_least_for_admission_pct": 10.0,
            "median_below_refusal_pct": 1.0,
            "mid_band_upper_exclusive_pct": 3.0,
            "mid_band_max_pdf_feature_delta_bytes": 70 * 1024,
        },
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rows", required=True)
    parser.add_argument("--aggregate", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    rows = [json.loads(line) for line in Path(args.rows).read_text(encoding="utf-8").splitlines() if line.strip()]
    published = json.loads(Path(args.aggregate).read_text(encoding="utf-8"))
    accepted = [row for row in rows if row.get("classification") == "accepted"]
    optimized = [row for row in rows if row.get("tzim", {}).get("status") == "optimized"]
    gain_rows = [
        row for row in optimized
        if isinstance(row.get("bytes"), int) and isinstance(row.get("tzim", {}).get("bytes"), int)
    ]
    gains = [
        row["tzim"]["gain_pct"] for row in optimized
        if isinstance(row.get("tzim", {}).get("gain_pct"), (int, float))
    ]
    optimized_source_bytes = sum(row["bytes"] for row in gain_rows)
    optimized_candidate_bytes = sum(row["tzim"]["bytes"] for row in gain_rows)

    def accepted_values(key):
        return [row[key] for row in accepted if isinstance(row.get(key), (int, float))]

    refusal_reasons = dict(sorted(Counter(
        row.get("reason", "unspecified") for row in rows if row.get("classification") == "refused"
    ).items()))
    recomputed = {
        "row_count": len(rows),
        "statuses": dict(sorted(Counter(row.get("classification", "missing") for row in rows).items())),
        "refusal_reasons": refusal_reasons,
        "accepted_n": len(accepted),
        "tzim_optimized_n": len(optimized),
        "faithful_pct": instrument(accepted_values("faithful_pct")),
        "qpdf_allowed_pct": instrument(accepted_values("qpdf_pct")),
        "forbidden_repack_pct": instrument(accepted_values("repack_pct")),
        "tzim_gain_pct": {
            "n": len(gains),
            "median": statistics.median(gains) if gains else None,
            "mean": statistics.fmean(gains) if gains else None,
        },
        "tzim_optimized_source_bytes": optimized_source_bytes,
        "tzim_optimized_candidate_bytes": optimized_candidate_bytes,
        "tzim_saved_bytes": optimized_source_bytes - optimized_candidate_bytes,
        "tzim_weighted_gain_pct": (
            100.0 * (optimized_source_bytes - optimized_candidate_bytes) / optimized_source_bytes
            if optimized_source_bytes else None
        ),
        "render_differences": sum(
            1 for row in rows for item in row.get("render_comparisons", [])
            if item.get("compared") and not item.get("identical")
        ),
        "qpdf_source_check_failures": sum(
            1 for row in rows
            if row.get("original_qpdf_check") is not None and row["original_qpdf_check"].get("returncode") != 0
        ),
        "qpdf_candidate_check_failures": sum(
            1 for row in rows for key in ("qpdf_allowed_check", "qpdf_repack_check", "tzim_qpdf_check")
            if row.get(key) is not None and row[key].get("returncode") not in (None, 0)
        ),
    }
    feature_delta = (published.get("pre_registered_threshold_disposition") or {}).get("pdf_feature_delta_bytes")
    threshold_result = threshold_disposition(rows, recomputed, feature_delta)
    recomputed["pre_registered_threshold_disposition"] = threshold_result
    fields = list(recomputed)
    mismatches = {
        key: {"script": published.get(key), "independent": recomputed[key]}
        for key in fields if published.get(key) != recomputed[key]
    }
    result = {
        "independent_recomputation": recomputed,
        "matches_measurement_aggregate": not mismatches,
        "mismatches": mismatches,
    }
    Path(args.out).write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if mismatches:
        raise SystemExit(f"aggregate mismatch: {json.dumps(mismatches, sort_keys=True)}")
    if len(rows) != 21 or len({row.get("id") for row in rows}) != 21:
        raise SystemExit(f"raw PDF rows must retain exactly 21 unique products; got {len(rows)}")
    print(f"independently recomputed all {len(rows)} PDF rows; aggregate matches")


if __name__ == "__main__":
    main()
