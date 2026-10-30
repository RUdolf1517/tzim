#!/usr/bin/env python3
"""Independently recompute all fixed #59 tier, reference-gate and threshold aggregates."""

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


def distribution(values):
    ordered = sorted(values)
    if not ordered:
        return {
            "n": 0,
            "status": "empty",
            "median": None,
            "q1_index": None,
            "q3_index": None,
            "share_positive": None,
        }
    return {
        "n": len(ordered),
        "status": "measured",
        "median": statistics.median(ordered),
        "q1_index": ordered[len(ordered) // 4],
        "q3_index": ordered[(3 * len(ordered)) // 4],
        "share_positive": sum(value > 0 for value in ordered) / len(ordered),
    }


def threshold_disposition(rows, tier1_stats, tier2_stats):
    blockers = []
    if len(rows) != 14:
        blockers.append(f"expected all 14 frozen products, found {len(rows)}")
    tier2_rows = []
    for row in rows:
        product_id = row.get("product_id", "unknown")
        if row.get("fetch_status") != "downloaded":
            blockers.append(f"{product_id}: source fetch did not complete")
        if row.get("measurement_status") != "measured":
            blockers.append(f"{product_id}: fixed-product measurement status is {row.get('measurement_status')}")
        audit = row.get("existing_two_tier_instrument") or {}
        if audit.get("status") == "not_measured":
            blockers.append(f"{product_id}: no row in the existing two-tier instrument")
        if audit.get("status") == "tier2":
            tier2_rows.append(row)
            if audit.get("exact") is not True:
                blockers.append(f"{product_id}: Astropy reference candidate did not pass its original verifier")
            validation = row.get("reference_candidate_validation") or {}
            if validation.get("status") != "passed":
                blockers.append(f"{product_id}: reference-candidate HEASARC/funpack gate is {validation.get('status')}")

    if blockers:
        return {
            "status": "not_applied",
            "decision": "threshold not applied",
            "reason": "one or more fixed rows or reference-candidate validation checks remain unresolved",
            "blockers": blockers,
            "tier2_threshold": {
                "median_gain_pct_at_least": 8.0,
                "share_of_frozen_corpus_pct_at_least": 15.0,
                "n": tier2_stats["n"],
                "median_gain_pct": tier2_stats["median"],
                "q1_index_gain_pct": tier2_stats["q1_index"],
                "q3_index_gain_pct": tier2_stats["q3_index"],
                "share_positive": tier2_stats["share_positive"],
                "share_of_frozen_corpus_pct": 100.0 * len(tier2_rows) / len(rows) if rows else None,
                "result": "not evaluated because the validation gate is incomplete",
            },
            "tier1_threshold": {
                "median_gain_pct_at_least": 2.0,
                "n": tier1_stats["n"],
                "median_gain_pct": tier1_stats["median"],
                "result": "empty set; not evaluated" if tier1_stats["n"] == 0 else "not evaluated because the validation gate is incomplete",
            },
        }

    tier2_share = 100.0 * len(tier2_rows) / len(rows) if rows else None
    tier2_clears = (
        tier2_stats["n"] > 0
        and tier2_stats["median"] is not None
        and tier2_stats["median"] >= 8.0
        and tier2_share is not None
        and tier2_share >= 15.0
    )
    tier1_clears = (
        tier1_stats["n"] > 0
        and tier1_stats["median"] is not None
        and tier1_stats["median"] >= 2.0
    )
    if tier1_stats["n"] == 0:
        tier1_result = "empty set; not evaluated (not 0%)"
    elif tier1_clears:
        tier1_result = "clears the ≥2% median bar; ships on by default inside the fits feature"
    else:
        tier1_result = "does not clear the ≥2% median bar"

    if tier2_clears and tier1_clears:
        decision = "Tier 1 clears for default-in-feature use; Tier 2 clears for opt-in --fits-tile use"
    elif tier2_clears:
        decision = "Tier 2 clears; opt-in --fits-tile use; Tier 1 is not evaluated because its set is empty"
    elif tier1_clears:
        decision = "Tier 1 clears for default-in-feature use; Tier 2 does not clear"
    else:
        decision = "neither tier clears; FITS refused after measurement"
    return {
        "status": "applied",
        "decision": decision,
        "blockers": [],
        "tier2_threshold": {
            "median_gain_pct_at_least": 8.0,
            "share_of_frozen_corpus_pct_at_least": 15.0,
            "n": tier2_stats["n"],
            "median_gain_pct": tier2_stats["median"],
            "q1_index_gain_pct": tier2_stats["q1_index"],
            "q3_index_gain_pct": tier2_stats["q3_index"],
            "share_positive": tier2_stats["share_positive"],
            "share_of_frozen_corpus_pct": tier2_share,
            "result": "clears" if tier2_clears else "does not clear",
        },
        "tier1_threshold": {
            "median_gain_pct_at_least": 2.0,
            "n": tier1_stats["n"],
            "median_gain_pct": tier1_stats["median"],
            "q1_index_gain_pct": tier1_stats["q1_index"],
            "q3_index_gain_pct": tier1_stats["q3_index"],
            "share_positive": tier1_stats["share_positive"],
            "result": tier1_result,
        },
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
    audit_status_counts = dict(sorted(Counter(row.get("status", "missing") for row in audit_rows).items()))
    refusal_reasons = dict(sorted(Counter(
        row.get("reason", "unspecified") for row in audit_rows if row.get("status") == "refused"
    ).items()))
    audit_tier2 = [row for row in audit_rows if row.get("status") == "tier2"]
    tier2_gains = [
        row["pct"] for row in audit_tier2
        if row.get("exact") is True and isinstance(row.get("pct"), (int, float))
    ]
    tier1_gains = [
        row["tier1"]["gain_pct"] for row in rows
        if row.get("tier1", {}).get("status") == "measured_comparator"
    ]
    tzim_gains = [
        row["tier2_tzim"]["gain_pct"] for row in rows
        if isinstance(row.get("tier2_tzim", {}).get("gain_pct"), (int, float))
    ]
    tier2_stats = distribution(tier2_gains)
    tier1_stats = distribution(tier1_gains)
    tzim_stats = distribution(tzim_gains)
    reference_rows = [
        row for row in rows
        if (row.get("existing_two_tier_instrument") or {}).get("status") == "tier2"
    ]
    reference_validation_counts = dict(sorted(Counter(
        (row.get("reference_candidate_validation") or {}).get("status", "missing")
        for row in reference_rows
    ).items()))
    reference_fitsverify_counts = dict(sorted(Counter(
        ((row.get("reference_candidate_validation") or {}).get("fitsverify") or {}).get("status", "missing")
        for row in reference_rows
    ).items()))
    tier_distribution = {
        "tier1_comparator_n": len(tier1_gains),
        "tier1_comparator_product_ids": [
            row["product_id"] for row in rows if row.get("tier1", {}).get("status") == "measured_comparator"
        ],
        "tier2_reference_n": len(audit_tier2),
        "tier2_reference_product_ids": [row.get("product_id") for row in reference_rows],
        "refused_n": sum(1 for row in audit_rows if row.get("status") == "refused"),
        "refusal_reasons": refusal_reasons,
        "not_measured_n": sum(
            1 for row in rows
            if (row.get("existing_two_tier_instrument") or {}).get("status") == "not_measured"
        ),
        "note": "Tier 1 recompression rows are comparators derived from the frozen products and may overlap a Tier 2 product; they are not new survey inputs.",
    }
    reference_gate = {
        "expected_candidates_n": len(reference_rows),
        "validation_status_counts": reference_validation_counts,
        "fitsverify_status_counts": reference_fitsverify_counts,
        "validated_candidates_n": sum(
            1 for row in reference_rows
            if (row.get("reference_candidate_validation") or {}).get("status") == "passed"
        ),
        "candidate_identity_mismatches": sum(
            1 for row in reference_rows
            if (row.get("reference_candidate_validation") or {}).get("identity_matches_measurement_candidate") is False
        ),
        "funpack_roundtrip_failures": sum(
            1 for row in reference_rows
            if (row.get("reference_candidate_validation") or {})
            .get("reconstructed_array_literal_header_comparison", {}).get("ok") is False
        ),
    }
    recomputed = {
        "products_n": len(rows),
        "fetch_status_counts": dict(sorted(Counter(row.get("fetch_status", "missing") for row in rows).items())),
        "measurement_status_counts": dict(sorted(Counter(row.get("measurement_status", "missing") for row in rows).items())),
        "tzim_status_counts": dict(sorted(Counter(row.get("tier2_tzim", {}).get("status", "missing") for row in rows).items())),
        "existing_fits_measure_instrument": {
            "audit_rows": len(audit_rows),
            "tier2_candidate_rows": len(audit_tier2),
            "status_counts": audit_status_counts,
            "refusal_reasons": refusal_reasons,
        },
        "tier_and_refusal_distribution": tier_distribution,
        "existing_fits_measure_tier2_gain_pct": {
            **tier2_stats,
            "min": min(tier2_gains) if tier2_gains else None,
            "max": max(tier2_gains) if tier2_gains else None,
        },
        "existing_tile_tier1_comparator_gain_pct": {
            **tier1_stats,
            "min": min(tier1_gains) if tier1_gains else None,
            "max": max(tier1_gains) if tier1_gains else None,
        },
        "tzim_tier2_gain_pct": {
            **tzim_stats,
            "min": min(tzim_gains) if tzim_gains else None,
            "max": max(tzim_gains) if tzim_gains else None,
        },
        "reference_candidate_gate": reference_gate,
        "pre_registered_threshold_disposition": threshold_disposition(rows, tier1_stats, tier2_stats),
        "fitsverify_candidate_nonzero": sum(
            1 for row in rows
            if row.get("tier2_tzim", {}).get("status") == "optimized"
            and row.get("external_checks", {}).get("fitsverify_candidate", {}).get("returncode") != 0
        ),
        "array_header_roundtrip_passes": sum(1 for row in rows if row.get("array_header_roundtrip", {}).get("ok") is True),
        "array_header_roundtrip_failures": sum(1 for row in rows if row.get("array_header_roundtrip", {}).get("ok") is False),
        "funpack_roundtrip_passes": sum(1 for row in rows if row.get("funpack_array_header_roundtrip", {}).get("ok") is True),
        "funpack_roundtrip_not_applicable": sum(1 for row in rows if row.get("funpack_array_header_roundtrip", {}).get("status") == "not_applicable"),
        "funpack_failures": sum(1 for row in rows if row.get("funpack_array_header_roundtrip", {}).get("ok") is False),
        "reference_funpack_roundtrip_passes": sum(
            1 for row in reference_rows
            if (row.get("reference_candidate_validation") or {})
            .get("reconstructed_array_literal_header_comparison", {}).get("ok") is True
        ),
        "reference_funpack_roundtrip_failures": sum(
            1 for row in reference_rows
            if (row.get("reference_candidate_validation") or {})
            .get("reconstructed_array_literal_header_comparison", {}).get("ok") is False
        ),
    }
    fields = list(recomputed)
    mismatches = {
        key: {"measurement": aggregate.get(key), "independent": recomputed[key]}
        for key in fields if aggregate.get(key) != recomputed[key]
    }
    result = {
        "independent_recomputation": recomputed,
        "matches_measurement_aggregate": not mismatches,
        "mismatches": mismatches,
    }
    Path(args.out).write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if mismatches:
        raise SystemExit(f"FITS aggregate mismatch: {json.dumps(mismatches, sort_keys=True)}")

    failed_fetches = [row for row in rows if row.get("fetch_status") != "downloaded"]
    for row in failed_fetches:
        if not row.get("source_uri") or not row.get("fetch_error") or not row.get("needed_owner_action"):
            raise SystemExit(f"failed fetch row is incomplete: {row.get('product_id')}")
    print(
        f"independently recomputed all {len(rows)} FITS rows; aggregate matches; "
        f"fetch failures={len(failed_fetches)}; reference-candidate gate="
        f"{reference_gate['validated_candidates_n']}/{reference_gate['expected_candidates_n']}"
    )


if __name__ == "__main__":
    main()
