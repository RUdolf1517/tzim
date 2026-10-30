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


def positive_control_passed(control):
    """Check the retained command and comparator evidence independently."""
    comparison = control.get("literal_header_storage_array_comparison") or {}
    funpack_argv = control.get("funpack_invocation") or []
    fitsverify_argv = control.get("fitsverify_invocation") or []
    commands_ok = all(
        (control.get(name) or {}).get("returncode") == 0
        for name in ("fpack", "fitsverify", "funpack")
    )
    return (
        control.get("status") == "passed"
        and control.get("ok") is True
        and control.get("fixture_dtype") == "int16"
        and control.get("fpack_float_array_oracle_used") is False
        and commands_ok
        and "-l" in fitsverify_argv
        and "-C" in funpack_argv
        and comparison.get("diagnostic_schema") == "tzim-funpack-roundtrip-diagnostics-v2"
        and comparison.get("diagnostics_complete") is True
        and comparison.get("ok") is True
    )


def threshold_disposition(rows, tier1_stats, tier2_stats, positive_control):
    blockers = []
    if not positive_control_passed(positive_control):
        blockers.append("external funpack -C positive control did not pass; survey header/payload differences are not attributable")
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
            "reason": "positive control or one or more fixed-row/reference-candidate checks remain unresolved",
            "positive_control_status": positive_control.get("status", "missing"),
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
                "result": "empty set; not evaluated (not 0%)" if tier1_stats["n"] == 0 else "not evaluated because the validation gate is incomplete",
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
        "positive_control_status": positive_control.get("status", "missing"),
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
    parser.add_argument("--positive-control", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    rows = [json.loads(line) for line in Path(args.rows).read_text(encoding="utf-8").splitlines() if line.strip()]
    aggregate = json.loads(Path(args.aggregate).read_text(encoding="utf-8"))
    positive_control = json.loads(Path(args.positive_control).read_text(encoding="utf-8"))
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
        "header_payload_attribution_enabled": positive_control_passed(positive_control),
    }
    positive_control_summary = {
        "control_id": positive_control.get("control_id"),
        "status": positive_control.get("status", "missing"),
        "ok": positive_control_passed(positive_control),
        "fpack_exit": (positive_control.get("fpack") or {}).get("returncode"),
        "fitsverify_exit": (positive_control.get("fitsverify") or {}).get("returncode"),
        "funpack_exit": (positive_control.get("funpack") or {}).get("returncode"),
        "roundtrip_ok": (positive_control.get("literal_header_storage_array_comparison") or {}).get("ok") is True,
        "fpack_float_array_oracle_used": positive_control.get("fpack_float_array_oracle_used"),
        "funpack_checksum_mode": positive_control.get("funpack_checksum_mode"),
    }

    def comparison_diagnostics(rows_to_count, comparison_field, nested_field=None):
        failure_classes = Counter()
        card_difference_counts = Counter()
        header_card_mismatches = payload_mismatch_hdus = hdu_count_mismatches = mismatch_records = 0
        header_cards_equal_hdus = header_cards_mismatch_hdus = padding_only_hdus = 0
        complete_comparisons = comparisons_n = 0
        for row in rows_to_count:
            comparison = row.get(comparison_field) or {}
            if nested_field is not None:
                comparison = comparison.get(nested_field) or {}
            if comparison.get("diagnostic_schema") is None:
                continue
            comparisons_n += 1
            complete_comparisons += int(comparison.get("diagnostics_complete") is True)
            failure_classes.update(comparison.get("failure_class_counts") or {})
            mismatch_records += len(comparison.get("mismatch_records") or [])
            for check in comparison.get("hdu_checks") or []:
                header_card_mismatches += len(check.get("header_card_differences") or [])
                card_difference_counts.update(check.get("header_card_difference_counts") or {})
                payload_mismatch_hdus += int(check.get("storage_payload_equality") is False)
                header_cards_equal_hdus += int(check.get("header_cards_through_end_equality") is True)
                header_cards_mismatch_hdus += int(check.get("header_cards_through_end_equality") is False)
                padding_only_hdus += int(check.get("header_padding_only_difference") is True)
            hdu_count_mismatches += int(comparison.get("hdu_count_equal") is False)
        return {
            "comparisons_n": comparisons_n,
            "complete_comparisons_n": complete_comparisons,
            "failure_class_counts": dict(sorted(failure_classes.items())),
            "header_card_mismatch_count": header_card_mismatches,
            "header_card_difference_counts": {
                kind: card_difference_counts.get(kind, 0) for kind in ("inserted", "dropped", "changed")
            },
            "header_cards_through_end_equal_hdu_count": header_cards_equal_hdus,
            "header_cards_through_end_mismatch_hdu_count": header_cards_mismatch_hdus,
            "padding_only_hdu_count": padding_only_hdus,
            "payload_mismatch_hdu_count": payload_mismatch_hdus,
            "hdu_count_mismatch_count": hdu_count_mismatches,
            "structured_mismatch_record_count": mismatch_records,
            "diagnostics_file": "reference-hdu-diagnostics.jsonl",
        }

    reference_hdu_diagnostics = {
        "comparison_kind": "Astropy reference candidate funpack -C versus exact source",
        **comparison_diagnostics(rows, "reference_candidate_validation", "reconstructed_array_literal_header_comparison"),
    }
    tzim_hdu_diagnostics = {
        "comparison_kind": "Tzim candidate funpack -C versus source funpack -C",
        **comparison_diagnostics(rows, "funpack_array_header_roundtrip"),
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
        "external_funpack_C_positive_control": positive_control_summary,
        "reference_candidate_gate": reference_gate,
        "reference_hdu_diagnostics": reference_hdu_diagnostics,
        "tzim_hdu_diagnostics": tzim_hdu_diagnostics,
        "funpack_checksum_mode": "-C used for reference candidates and any optimized Tzim source/candidate reconstructions; checksum cards are not filtered",
        "pre_registered_threshold_disposition": threshold_disposition(rows, tier1_stats, tier2_stats, positive_control),
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
