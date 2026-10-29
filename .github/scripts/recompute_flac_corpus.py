#!/usr/bin/env python3
"""Independently recompute and validate the fixed FLAC corpus aggregates."""

from __future__ import annotations

import argparse
import hashlib
import json
import statistics
from collections import Counter
from pathlib import Path


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def write_json(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n", encoding="utf-8")


def normalized_md5(value: object) -> str | None:
    text = str(value or "").strip().lower()
    if len(text) != 32 or any(character not in "0123456789abcdef" for character in text):
        return None
    return text


def streaminfo_md5_check(source_md5: str | None, candidate_md5: str | None, pcm_md5: str | None) -> dict:
    zero_md5 = "0" * 32
    result = {
        "source_streaminfo_md5": source_md5,
        "candidate_streaminfo_md5": candidate_md5,
        "external_pcm_md5": pcm_md5,
        "input_zero_md5_sentinel": source_md5 == zero_md5,
        "input_md5_matches_external_pcm": (
            source_md5 == zero_md5
            or (source_md5 is not None and pcm_md5 is not None and source_md5 == pcm_md5)
        ),
        "candidate_md5_matches_external_pcm": (
            candidate_md5 is not None and pcm_md5 is not None and candidate_md5 == pcm_md5
        ),
        "candidate_preserves_recorded_input_md5": (
            source_md5 == zero_md5
            or (source_md5 is not None and candidate_md5 is not None and candidate_md5 == source_md5)
        ),
    }
    result["ok"] = all(
        result[key]
        for key in (
            "input_md5_matches_external_pcm",
            "candidate_md5_matches_external_pcm",
            "candidate_preserves_recorded_input_md5",
        )
    )
    return result


def canonical_manifest_hash(expanded: dict) -> str:
    value = dict(expanded)
    value.pop("expanded_manifest_sha256", None)
    canonical = json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(canonical).hexdigest()


def recompute(rows: list[dict], expanded: dict) -> dict:
    counts = Counter(row.get("status", "unknown") for row in rows)
    before = sum(int(row.get("before_bytes") or 0) for row in rows)
    after = sum(int(row.get("after_bytes") or row.get("before_bytes") or 0) for row in rows)
    optimized = [row for row in rows if row.get("status") == "optimized"]
    gains = [100.0 * row["saved_bytes"] / row["before_bytes"] for row in optimized if row.get("before_bytes")]
    times = [float(row["tzim_duration_ms"]) for row in rows if row.get("tzim_duration_ms") is not None]
    blockers = [
        {"id": row.get("id"), **blocker}
        for row in rows
        for blocker in row.get("blockers", [])
    ]
    return {
        "schema": 1,
        "expected_rows": len(expanded.get("inputs", [])) + len(expanded.get("derived_inputs", [])),
        "rows": len(rows),
        "status_counts": dict(sorted(counts.items())),
        "source_fetch_statuses": dict(sorted(Counter(row.get("fetch_status", "derived") for row in expanded.get("inputs", [])).items())),
        "derived_generation_statuses": dict(sorted(Counter(row.get("generation_status", "unknown") for row in expanded.get("derived_inputs", [])).items())),
        "input_bytes": before,
        "after_bytes": after,
        "saved_bytes": before - after,
        "optimized_rows": len(optimized),
        "optimized_median_gain_pct": round(statistics.median(gains), 6) if gains else None,
        "optimized_mean_gain_pct": round(statistics.fmean(gains), 6) if gains else None,
        "tzim_total_seconds": round(sum(times) / 1000, 3),
        "tzim_median_ms": round(statistics.median(times), 3) if times else None,
        "external_flac_source_passes": sum(row.get("reference_flac_test", {}).get("return_code") == 0 for row in rows),
        "external_flac_candidate_passes": sum(row.get("candidate_checks", {}).get("flac_test", {}).get("return_code") == 0 for row in rows),
        "external_metaflac_source_passes": sum(row.get("reference_metaflac", {}).get("return_code") == 0 for row in rows),
        "external_metaflac_candidate_passes": sum(row.get("candidate_checks", {}).get("metaflac", {}).get("return_code") == 0 for row in rows),
        "external_pcm_pairs_equal": sum(
            bool(row.get("candidate_checks", {}).get("external_pcm", {}).get("source", {}).get("sha256"))
            and row.get("candidate_checks", {}).get("external_pcm", {}).get("source", {}).get("sha256")
            == row.get("candidate_checks", {}).get("external_pcm", {}).get("candidate", {}).get("sha256")
            for row in rows
        ),
        "external_streaminfo_md5_checks_passed": sum(
            row.get("candidate_checks", {}).get("streaminfo_md5", {}).get("ok") is True
            for row in rows
        ),
        "external_streaminfo_md5_checks_run": sum(
            "streaminfo_md5" in row.get("candidate_checks", {})
            for row in rows
        ),
        "blockers": blockers,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence-dir", type=Path, required=True)
    args = parser.parse_args()
    evidence = args.evidence_dir
    expanded_path = evidence / "expanded-manifest.json"
    raw_path = evidence / "raw-rows.jsonl"
    aggregate_path = evidence / "aggregate.json"
    output_path = evidence / "independent-recomputation.json"
    errors: list[str] = []
    if not expanded_path.is_file() or not raw_path.is_file() or not aggregate_path.is_file():
        output = {"matches": False, "errors": ["expanded manifest, raw rows, or aggregate is missing"]}
        write_json(output_path, output)
        print(output["errors"][0])
        return 1

    expanded = load_json(expanded_path)
    rows = load_jsonl(raw_path)
    published = load_json(aggregate_path)
    expected_ids = [row["id"] for row in expanded.get("inputs", []) + expanded.get("derived_inputs", [])]
    observed_ids = [row.get("id") for row in rows]
    if len(expected_ids) != len(set(expected_ids)):
        errors.append("expanded manifest contains duplicate IDs")
    if len(observed_ids) != len(set(observed_ids)):
        errors.append("raw rows contain duplicate IDs")
    if observed_ids != expected_ids:
        errors.append(f"raw row identities/order differ: expected {expected_ids}, observed {observed_ids}")
    if len(rows) != len(expected_ids) or len(rows) < 20 or len(rows) > 50:
        errors.append(f"raw row count {len(rows)} is not the registered 20–50 row set ({len(expected_ids)})")
    if not expanded.get("frozen_before_tzim_measurement"):
        errors.append("expanded manifest does not attest it was frozen before measurement")
    source_manifest_path = evidence / "source-manifest.json"
    if not source_manifest_path.is_file():
        errors.append("static source manifest copy is missing")
    else:
        source_bytes = source_manifest_path.read_bytes()
        if len(source_bytes) != expanded.get("source_manifest_bytes"):
            errors.append("static source manifest byte count differs from expanded identity")
        if hashlib.sha256(source_bytes).hexdigest() != expanded.get("source_manifest_sha256"):
            errors.append("static source manifest SHA-256 differs from expanded identity")
    if expanded.get("expanded_manifest_sha256") != canonical_manifest_hash(expanded):
        errors.append("expanded manifest self-excluding SHA-256 does not match")

    expanded_by_id = {row["id"]: row for row in expanded.get("inputs", []) + expanded.get("derived_inputs", [])}
    for row in rows:
        registered = expanded_by_id.get(row.get("id"))
        if registered is None:
            errors.append(f"raw row has unregistered ID {row.get('id')}")
            continue
        expected_sha = registered.get("observed_sha256", registered.get("sha256"))
        expected_bytes = registered.get("observed_bytes", registered.get("bytes"))
        if expected_sha is not None and row.get("source_sha256") != expected_sha:
            errors.append(f"{row['id']} source SHA differs from its frozen expanded manifest")
        if expected_bytes is not None and row.get("before_bytes") is not None and row.get("before_bytes") != expected_bytes:
            errors.append(f"{row['id']} byte count differs from its frozen expanded manifest")
        if row.get("status") == "optimized":
            if row.get("after_bytes", 0) >= row.get("before_bytes", 0):
                errors.append(f"{row['id']} is marked optimized without a strictly smaller candidate")
            checks = row.get("candidate_checks") or {}
            source_meta = row.get("reference_metaflac", {}).get("values", {})
            candidate_meta = checks.get("metaflac", {}).get("values", {})
            pcm = checks.get("external_pcm", {})
            source_pcm = pcm.get("source", {})
            candidate_pcm = pcm.get("candidate", {})
            source_md5 = normalized_md5(source_meta.get("md5"))
            candidate_md5 = normalized_md5(candidate_meta.get("md5"))
            pcm_md5 = source_pcm.get("md5") if source_pcm.get("return_code") == 0 else None
            md5_check = streaminfo_md5_check(source_md5, candidate_md5, pcm_md5)
            if checks.get("streaminfo_md5") != md5_check:
                errors.append(f"{row['id']} STREAMINFO/independent PCM MD5 evidence does not recompute")
            if not md5_check["ok"]:
                errors.append(f"{row['id']} STREAMINFO MD5 does not agree with independent decoded PCM")
            if (
                source_pcm.get("return_code") == 0
                and candidate_pcm.get("return_code") == 0
                and source_pcm.get("md5") != candidate_pcm.get("md5")
            ):
                errors.append(f"{row['id']} external PCM MD5 differs between source and candidate")
        elif row.get("before_sha256") is not None and row.get("after_sha256") != row.get("before_sha256"):
            errors.append(f"{row['id']} changed bytes without an optimized status")

    expected_aggregate = recompute(rows, expanded)
    for key, value in expected_aggregate.items():
        if published.get(key) != value:
            errors.append(f"aggregate mismatch for {key}: published={published.get(key)!r}, recomputed={value!r}")
    if published.get("expected_rows") != len(expected_ids):
        errors.append("aggregate expected_rows differs from the frozen manifest")
    if published.get("blockers"):
        errors.append(f"measurement blockers remain: {len(published['blockers'])}")
    if published.get("fetch_blockers"):
        errors.append(f"pinned-source fetch blockers remain: {len(published['fetch_blockers'])}")
    if published.get("generation_blockers"):
        errors.append(f"derived-input generation blockers remain: {len(published['generation_blockers'])}")

    output = {
        **expected_aggregate,
        "matches": not errors,
        "errors": errors,
        "expanded_manifest_sha256": expanded.get("expanded_manifest_sha256"),
    }
    write_json(output_path, output)
    print(f"rows={output['rows']} optimized={output['optimized_rows']} saved_bytes={output['saved_bytes']} matches={output['matches']}")
    if errors:
        for error in errors:
            print(f"ERROR: {error}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
