#!/usr/bin/env python3
"""Independent recomputation of the fixed #57 PDF aggregate from raw rows."""

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

    def instrument(key):
        values = [row[key] for row in accepted if isinstance(row.get(key), (int, float))]
        return {
            "n": len(values),
            "median_upper": quantile(values, 1, 2),
            "q1_index": quantile(values, 1, 4),
            "q3_index": quantile(values, 3, 4),
            "share_positive": sum(value > 0 for value in values) / len(values) if values else None,
        }

    gains = [row["tzim"]["gain_pct"] for row in optimized if isinstance(row.get("tzim", {}).get("gain_pct"), (int, float))]
    recomputed = {
        "row_count": len(rows),
        "statuses": dict(sorted(Counter(row.get("classification", "missing") for row in rows).items())),
        "accepted_n": len(accepted),
        "tzim_optimized_n": len(optimized),
        "faithful_pct": instrument("faithful_pct"),
        "qpdf_allowed_pct": instrument("qpdf_pct"),
        "forbidden_repack_pct": instrument("repack_pct"),
        "tzim_gain_pct": {"n": len(gains), "median": statistics.median(gains) if gains else None},
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
    fields = list(recomputed)
    mismatches = {
        key: {"script": published.get(key), "independent": recomputed[key]}
        for key in fields if published.get(key) != recomputed[key]
    }
    result = {"independent_recomputation": recomputed, "matches_measurement_aggregate": not mismatches, "mismatches": mismatches}
    Path(args.out).write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if mismatches:
        raise SystemExit(f"aggregate mismatch: {json.dumps(mismatches, sort_keys=True)}")
    if len(rows) != 21 or len({row.get("id") for row in rows}) != 21:
        raise SystemExit(f"raw PDF rows must retain exactly 21 unique products; got {len(rows)}")
    print(f"independently recomputed all {len(rows)} PDF rows; aggregate matches")


if __name__ == "__main__":
    main()
