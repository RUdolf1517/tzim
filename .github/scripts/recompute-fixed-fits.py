#!/usr/bin/env python3
"""Independently recompute #59 status and gain aggregates from all 14 raw rows."""

import argparse
import json
import statistics
from collections import Counter
from pathlib import Path

EXPECTED_IDS = {
    "tess-s1-c1-ccd1-calibrated", "tess-s1-c1-ccd1-uncalibrated",
    "tess-s1-c4-ccd2-calibrated", "tess-s1-c4-ccd2-uncalibrated",
    "galex-tile-01-MISDR1_18916_0459-fd-int", "galex-tile-02-MISDR1_18916_0459-nd-int",
    "galex-tile-03-MISWZS03_18917_0284-fd-int", "galex-tile-04-MISWZS03_18917_0284-nd-int",
    "rosat-rass-c", "rosat-rass-m", "rosat-rass-ij",
    "rosat-rass-exp-c", "rosat-rass-exp-m", "rosat-rass-exp-ij",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rows", required=True)
    parser.add_argument("--aggregate", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    rows = [json.loads(line) for line in Path(args.rows).read_text(encoding="utf-8").splitlines() if line.strip()]
    aggregate = json.loads(Path(args.aggregate).read_text(encoding="utf-8"))
    ids = {row.get("product_id") for row in rows}
    if len(rows) != 14 or ids != EXPECTED_IDS:
        raise SystemExit(f"raw FITS rows are not the exact 14-product source set: {len(rows)} rows, IDs={sorted(ids)}")

    audit_rows = [
        row["existing_two_tier_instrument"] for row in rows
        if row.get("existing_two_tier_instrument", {}).get("status") != "not_measured"
    ]
    audit_t2 = [row["pct"] for row in audit_rows if row.get("status") == "tier2" and row.get("exact") and isinstance(row.get("pct"), (int, float))]
    tier1 = [row["tier1"]["gain_pct"] for row in rows if row.get("tier1", {}).get("status") == "measured_comparator"]
    tzim = [row["tier2_tzim"]["gain_pct"] for row in rows if isinstance(row.get("tier2_tzim", {}).get("gain_pct"), (int, float))]
    recomputed = {
        "products_n": len(rows),
        "fetch_status_counts": dict(sorted(Counter(row.get("fetch_status", "missing") for row in rows).items())),
        "measurement_status_counts": dict(sorted(Counter(row.get("measurement_status", "missing") for row in rows).items())),
        "tzim_status_counts": dict(sorted(Counter(row.get("tier2_tzim", {}).get("status", "missing") for row in rows).items())),
        "existing_fits_measure_instrument": {
            "tier2_rows": len(audit_rows),
            "status_counts": dict(sorted(Counter(row.get("status", "missing") for row in audit_rows).items())),
        },
        "existing_fits_measure_tier2_gain_pct": {
            "n": len(audit_t2), "median": statistics.median(audit_t2) if audit_t2 else None,
            "min": min(audit_t2) if audit_t2 else None, "max": max(audit_t2) if audit_t2 else None,
        },
        "existing_tile_tier1_comparator_gain_pct": {
            "n": len(tier1), "median": statistics.median(tier1) if tier1 else None,
            "min": min(tier1) if tier1 else None, "max": max(tier1) if tier1 else None,
        },
        "tzim_tier2_gain_pct": {
            "n": len(tzim), "median": statistics.median(tzim) if tzim else None,
            "min": min(tzim) if tzim else None, "max": max(tzim) if tzim else None,
        },
        "fitsverify_candidate_nonzero": sum(
            1 for row in rows
            if row.get("tier2_tzim", {}).get("status") == "optimized"
            and row.get("external_checks", {}).get("fitsverify_candidate", {}).get("returncode") != 0
        ),
        "array_header_roundtrip_failures": sum(1 for row in rows if row.get("array_header_roundtrip", {}).get("ok") is False),
        "funpack_failures": sum(1 for row in rows if row.get("funpack_array_header_roundtrip", {}).get("ok") is False),
    }
    fields = list(recomputed)
    mismatches = {key: {"measurement": aggregate.get(key), "independent": recomputed[key]}
                  for key in fields if aggregate.get(key) != recomputed[key]}
    result = {"independent_recomputation": recomputed, "matches_measurement_aggregate": not mismatches, "mismatches": mismatches}
    Path(args.out).write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if mismatches:
        raise SystemExit(f"FITs aggregate mismatch: {json.dumps(mismatches, sort_keys=True)}")
    failed_fetches = [row for row in rows if row.get("fetch_status") != "downloaded"]
    for row in failed_fetches:
        if not row.get("source_uri") or not row.get("fetch_error") or not row.get("needed_owner_action"):
            raise SystemExit(f"failed fetch row is incomplete: {row.get('product_id')}")
    print(f"independently recomputed all {len(rows)} FITS rows; aggregate matches; fetch failures={len(failed_fetches)}")


if __name__ == "__main__":
    main()
