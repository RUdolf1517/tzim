#!/usr/bin/env python3
"""Run the frozen #59 FITS products through tzim, fitsverify, funpack and array/header checks."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import shutil
import statistics
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

try:
    import numpy as np
    from astropy.io import fits
    FITS_IO_IMPORT_ERROR = None
except Exception as error:
    np = None
    fits = None
    FITS_IO_IMPORT_ERROR = f"{type(error).__name__}: {error}"

fits_measure = None

STRUCTURAL_CARDS = {
    "SIMPLE", "XTENSION", "BITPIX", "NAXIS", "PCOUNT", "GCOUNT", "EXTEND", "GROUPS",
    "TFIELDS", "THEAP", "ZIMAGE", "ZCMPTYPE", "ZBITPIX", "ZNAXIS", "ZTENSION",
}


def structural(keyword: str) -> bool:
    key = keyword.upper()
    return (
        key in STRUCTURAL_CARDS
        or key.startswith("NAXIS")
        or key.startswith("ZNAXIS")
        or key.startswith("ZTILE")
        or key.startswith("ZNAME")
        or key.startswith("ZVAL")
        or key.startswith("TTYPE")
        or key.startswith("TFORM")
        or key.startswith("TUNIT")
        or key.startswith("TSCAL")
        or key.startswith("TZERO")
    )


def load_fits_measure(path: Path):
    spec = importlib.util.spec_from_file_location("tzim_fits_measure", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot import existing FITS measurement instrument {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_fixed_products(path: Path) -> list[dict]:
    spec = importlib.util.spec_from_file_location("tzim_fixed_fits_sources", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot import the owner-fixed FITS source registry {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.PRODUCTS


def digest(path: Path) -> tuple[int, str]:
    h = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        while chunk := stream.read(8 * 1024 * 1024):
            size += len(chunk)
            h.update(chunk)
    return size, h.hexdigest()


def run(argv: list[str], log: list[dict], timeout: int = 1800) -> dict:
    try:
        result = subprocess.run(argv, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
        row = {"argv": argv, "returncode": result.returncode, "stdout": result.stdout, "stderr": result.stderr}
    except Exception as error:
        row = {"argv": argv, "error": f"{type(error).__name__}: {error}"}
    log.append(row)
    return row


def image_bytes(hdu) -> bytes | None:
    if hdu.data is None:
        return None
    array = np.asarray(hdu.data)
    if array.dtype.fields is not None or array.dtype.kind == "O":
        return None
    # FITS images are stored big-endian. Normalize both reader paths to that
    # byte order so signed zero and NaN payload bits remain part of the check.
    dtype = array.dtype.newbyteorder(">")
    normalized = array.astype(dtype, copy=False)
    return normalized.tobytes(order="C")


def user_cards(hdu, ignore_checksums: bool = False) -> list[str]:
    ignored = {"CHECKSUM", "DATASUM"} if ignore_checksums else set()
    return [
        card.image for card in hdu.header.cards
        if not structural(card.keyword) and card.keyword.upper() not in ignored
    ]


def raw_hdu(path: Path, hdu) -> bytes:
    start = int(hdu._header_offset)
    data_start = int(hdu._data_offset if hdu._data_offset is not None else start + len(hdu.header.tostring().encode("ascii")))
    data_size = int(hdu._data_size or 0)
    padded = ((data_size + 2879) // 2880) * 2880
    end = data_start + padded
    with path.open("rb") as stream:
        stream.seek(start)
        return stream.read(end - start)


def raw_data(path: Path, hdu) -> bytes:
    data_start = int(hdu._data_offset if hdu._data_offset is not None else hdu._header_offset + len(hdu.header.tostring().encode("ascii")))
    data_size = int(hdu._data_size or 0)
    padded = ((data_size + 2879) // 2880) * 2880
    with path.open("rb") as stream:
        stream.seek(data_start)
        return stream.read(padded)


def is_compressed_image_hdu(hdu) -> bool:
    """Recognize Astropy's logical CompImageHDU view as well as raw ZIMAGE tables.

    Astropy projects a compressed binary table into the original image-header
    view by default, so `hdu.header.get("ZIMAGE")` is absent even though the raw
    table has `ZIMAGE = T`. The class identity is the reliable signal here.
    """
    return isinstance(hdu, fits.CompImageHDU) or bool(hdu.header.get("ZIMAGE", False))


def compare_fits(
    original: Path,
    candidate: Path,
    compare_untransformed_raw: bool = True,
    allow_primary_filler: bool = False,
) -> dict:
    with fits.open(original, memmap=False, do_not_scale_image_data=True, uint=False) as before, \
         fits.open(candidate, memmap=False, do_not_scale_image_data=True, uint=False) as after:
        offset = 0
        offset_probe = None
        if len(before) and len(after) == len(before) + 1 and len(after) >= 2:
            first = after[0]
            second = after[1]
            primary_eligible = bool(fits_measure.eligible(before[0]) and not is_compressed_image_hdu(before[0]))
            has_filler = first.header.get("NAXIS", 0) == 0 and first.data is None
            second_is_compressed = is_compressed_image_hdu(second)
            offset_probe = {
                "primary_eligible": primary_eligible,
                "empty_primary_filler": has_filler,
                "second_hdu_is_compressed_image": second_is_compressed,
                "second_hdu_type": type(second).__name__,
            }
            if primary_eligible and has_filler and (second_is_compressed or allow_primary_filler):
                offset = 1
        if len(after) != len(before) + offset:
            result = {"ok": False, "reason": "hdu_count", "before_hdus": len(before), "after_hdus": len(after), "offset": offset}
            if offset_probe is not None:
                result["primary_offset_probe"] = offset_probe
            return result
        if offset:
            filler = after[0]
            if filler.header.get("SIMPLE") is not True or filler.header.get("NAXIS", 0) != 0 or filler.data is not None:
                return {"ok": False, "reason": "invalid-empty-primary-before-compressed-primary"}

        checks = []
        for index, original_hdu in enumerate(before):
            candidate_index = index + offset
            candidate_hdu = after[candidate_index]
            original_zimage = is_compressed_image_hdu(original_hdu)
            candidate_zimage = is_compressed_image_hdu(candidate_hdu)
            original_eligible = bool(
                fits_measure.eligible(original_hdu)
                and not original_zimage
            )
            transformed = original_eligible and candidate_zimage
            item = {"original_hdu": index, "candidate_hdu": candidate_index, "transformed": transformed}
            if transformed:
                left = image_bytes(original_hdu)
                right = image_bytes(candidate_hdu)
                if left is None or right is None or left != right:
                    item.update(ok=False, reason="image-array-byte-mismatch")
                elif user_cards(original_hdu) != user_cards(candidate_hdu):
                    item.update(ok=False, reason="image-user-header-card-mismatch")
                else:
                    item.update(ok=True, reason="array-bytes-and-user-header-cards-identical",
                                image_sha256=hashlib.sha256(left).hexdigest())
            else:
                # Every HDU not transformed must remain a complete byte-for-byte
                # copy, including non-image HDUs and original compressed tables.
                raw_equal = raw_hdu(original, original_hdu) == raw_hdu(candidate, candidate_hdu)
                left = image_bytes(original_hdu)
                right = image_bytes(candidate_hdu)
                if left is None and right is None:
                    arrays_equal = raw_data(original, original_hdu) == raw_data(candidate, candidate_hdu)
                else:
                    arrays_equal = left is not None and right is not None and left == right
                cards_equal = user_cards(original_hdu, ignore_checksums=not compare_untransformed_raw) == user_cards(
                    candidate_hdu, ignore_checksums=not compare_untransformed_raw
                )
                exact = raw_equal if compare_untransformed_raw else (arrays_equal and cards_equal)
                item.update(ok=exact, reason="untransformed-HDU-byte-identical" if exact else "untransformed-HDU-changed",
                            raw_hdu_equal=raw_equal, arrays_equal=arrays_equal, user_header_cards_equal=cards_equal)
            checks.append(item)
        if offset:
            checks.append({"original_hdu": None, "candidate_hdu": 0, "transformed": False,
                           "ok": True, "reason": "required-empty-primary"})
        return {"ok": all(item.get("ok", False) for item in checks), "hdu_checks": checks,
                "original_hdus": len(before), "candidate_hdus": len(after), "primary_offset": offset}


def sample_tier1(original: Path, fits_measure) -> dict:
    try:
        payloads = fits_measure.tiles(str(original))
        if not payloads:
            return {"status": "not_applicable", "reason": "no existing compressed image tiles"}
        result = fits_measure.recompress(payloads)
        if result is None:
            return {"status": "refused", "reason": "existing tile is not a complete gzip member"}
        old, new = result
        return {
            "status": "measured_comparator",
            "tier": "Tier 1 existing compressed tiles, recompress-only estimate",
            "old_tile_bytes": old,
            "recompressed_tile_bytes": new,
            "gain_pct": 100.0 * (old - new) / old if old else 0.0,
            "parent_product_input_sha256": digest(original)[1],
            "derivation": "read tiles from this published source FITS; gzip-decompress and recompress each at level 9; no new published product and no source modification",
            "tool": "Python gzip, same function as .github/scripts/fits-measure.py",
        }
    except Exception as error:
        return {"status": "failed", "error": f"{type(error).__name__}: {error}"}


def main() -> int:
    global fits_measure
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-manifest", required=True)
    parser.add_argument("--root", required=True)
    parser.add_argument("--audit-rows", required=True)
    parser.add_argument("--tzim", required=True)
    parser.add_argument("--fitsverify", required=True)
    parser.add_argument("--funpack", required=True)
    parser.add_argument("--work", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--fits-measure", default=".github/scripts/fits-measure.py")
    parser.add_argument("--expected-source-sha", required=True)
    args = parser.parse_args()

    source_manifest_path = Path(args.source_manifest)
    source_manifest_bytes = source_manifest_path.read_bytes()
    manifest = json.loads(source_manifest_bytes)
    manifest_sha = hashlib.sha256(source_manifest_bytes).hexdigest()
    if manifest.get("schema") != "tzim-fixed-fits-source-manifest-v1" or manifest.get("source_decision_comment") != "6498702204":
        raise SystemExit("input source manifest does not match the owner pre-registration")
    if manifest.get("repository_source_sha") != args.expected_source_sha:
        raise SystemExit("FITS source manifest was not generated from the expected repository SHA")
    products = manifest.get("products", [])
    fixed_products = load_fixed_products(Path(__file__).with_name("fetch-fixed-fits.py"))
    fixed_by_id = {row["id"]: row for row in fixed_products}
    products_by_id = {row.get("id"): row for row in products if isinstance(row, dict)}
    if (
        len(products) != 14
        or len(products_by_id) != 14
        or set(products_by_id) != set(fixed_by_id)
    ):
        raise SystemExit("fixed #59 FITS manifest must retain the exact 14 owner-registered product IDs")
    identity_fields = ("id", "mission", "source_uri", "retrieval_url", "product", "rights_basis", "transport")
    for product_id, fixed in fixed_by_id.items():
        observed = products_by_id[product_id]
        if any(observed.get(field) != fixed.get(field) for field in identity_fields):
            raise SystemExit(f"fixed #59 source identity changed for {product_id}; do not substitute products")

    fits_measure_import_error = None
    try:
        fits_measure = load_fits_measure(Path(args.fits_measure))
    except Exception as error:
        fits_measure = None
        fits_measure_import_error = f"{type(error).__name__}: {error}"
    dependency_errors = [
        error for error in (FITS_IO_IMPORT_ERROR, fits_measure_import_error) if error is not None
    ]
    measurement_dependency_error = "; ".join(dependency_errors) or None
    audit_path = Path(args.audit_rows)
    audit_rows = [json.loads(line) for line in audit_path.read_text(encoding="utf-8").splitlines() if line.strip()] if audit_path.exists() else []
    audit_by_path = {Path(row.get("path", "")).name: row for row in audit_rows}
    input_root = Path(args.root).resolve()
    work = Path(args.work).resolve()
    out = Path(args.out).resolve()
    work.mkdir(parents=True, exist_ok=True)
    out.mkdir(parents=True, exist_ok=True)
    logs: list[dict] = []
    products_by_id = {row["id"]: row for row in products}
    rows: list[dict] = []

    for order, product in enumerate(products, start=1):
        row = {
            "order": order,
            "product_id": product["id"],
            "mission": product["mission"],
            "product": product["product"],
            "source_uri": product["source_uri"],
            "requested_url": product["requested_url"],
            "rights_basis": product["rights_basis"],
            "fetch_status": product["fetch_status"],
            "source_manifest_sha256": manifest_sha,
            "source_product_record": product,
            "temporary_cleanup_errors": product.get("temporary_cleanup_errors", []),
            "measurement_dependency_error": measurement_dependency_error,
            "tier2_tzim": {"status": "not_run"},
            "external_checks": {},
        }
        if product["fetch_status"] != "downloaded":
            row["measurement_status"] = "fetch_failed"
            row["fetch_error"] = product.get("fetch_error")
            row["needed_owner_action"] = product.get("needed_owner_action")
            rows.append(row)
            continue
        source = input_root / product["measurement_path"]
        if not source.is_file():
            row["measurement_status"] = "input_missing"
            row["fetch_error"] = f"fetched input missing after manifest freeze: {product['measurement_path']}"
            rows.append(row)
            continue
        size, sha = digest(source)
        row.update(input_path=product["measurement_path"], input_bytes=size, input_sha256=sha)
        if size != product["measurement_bytes"] or sha != product["measurement_sha256"]:
            row["measurement_status"] = "manifest_integrity_failure"
            row["fetch_error"] = "FITS measurement input differs from frozen byte count or SHA-256"
            rows.append(row)
            continue

        audit = audit_by_path.get(source.name)
        if audit is not None:
            row["existing_two_tier_instrument"] = audit
        else:
            row["existing_two_tier_instrument"] = {"status": "not_measured", "reason": "input absent from fits-measure audit rows"}
        row["tier1"] = sample_tier1(source, fits_measure)
        if row["tier1"].get("status") == "failed":
            row["tier1_error"] = row["tier1"].get("error")

        candidate = work / f"{order:02d}-{product['id']}.candidate.fits"
        shutil.copyfile(source, candidate)
        before_size, before_sha = digest(candidate)
        tzim_argv = [
            str(Path(args.tzim).resolve()), "--formats", "fits", "--fits-tile",
            "--jobs", "1", "--json", "--no-progress", str(candidate),
        ]
        tzim_result = run(tzim_argv, logs)
        file_rows = []
        for line in tzim_result.get("stdout", "").splitlines():
            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                continue
            if record.get("type") == "file":
                file_rows.append(record)
        file_row = file_rows[0] if file_rows else None
        after_size, after_sha = digest(candidate)
        tzim_record = {
            "argv": tzim_argv,
            "returncode": tzim_result.get("returncode"),
            "stdout": tzim_result.get("stdout", ""),
            "stderr": tzim_result.get("stderr", ""),
            "file_row": file_row,
            "candidate_bytes": after_size,
            "candidate_sha256": after_sha,
            "candidate_changed": after_sha != before_sha,
            "status": file_row.get("status") if file_row else "failed",
        }
        if tzim_result.get("error"):
            tzim_record["error"] = tzim_result["error"]
        elif tzim_result.get("returncode") != 0 or file_row is None:
            tzim_record["error"] = "tzim returned non-zero or emitted no file row"
        elif after_sha != before_sha and (after_size >= before_size or file_row.get("status") != "optimized"):
            tzim_record["error"] = "tzim changed bytes without a strictly smaller optimized result"
        elif after_sha == before_sha and file_row.get("status") == "optimized":
            tzim_record["error"] = "tzim reported optimized without changing the candidate bytes"
        elif after_sha != before_sha:
            tzim_record["gain_pct"] = 100.0 * (before_size - after_size) / before_size if before_size else 0.0
        tzim_record["unchanged_nonoptimized"] = (
            after_sha == before_sha
            and file_row is not None
            and file_row.get("status") != "optimized"
            and not tzim_record.get("error")
        )
        row["tier2_tzim"] = tzim_record
        if measurement_dependency_error:
            row["measurement_status"] = "measurement_dependency_failed"
        else:
            row["measurement_status"] = "measured" if not tzim_record.get("error") else "tzim_failed"

        qv_source = run([str(Path(args.fitsverify).resolve()), str(source)], logs)
        qv_candidate = run([str(Path(args.fitsverify).resolve()), str(candidate)], logs)
        row["external_checks"]["fitsverify_source"] = qv_source
        row["external_checks"]["fitsverify_candidate"] = qv_candidate
        row["external_checks"]["fitsverify_source_tool"] = "HEASARC fitsverify from cfitsio 4.6.2"

        unpack_source = work / f"{order:02d}-{product['id']}.source-unpacked.fits"
        unpack_candidate = work / f"{order:02d}-{product['id']}.candidate-unpacked.fits"
        funpack_source_argv = [str(Path(args.funpack).resolve()), "-O", str(unpack_source), str(source)]
        funpack_candidate_argv = [str(Path(args.funpack).resolve()), "-O", str(unpack_candidate), str(candidate)]
        if tzim_record.get("unchanged_nonoptimized"):
            funpack_source = {
                "status": "not_applicable",
                "argv": funpack_source_argv,
                "reason": "unchanged non-optimized row; verified by Tzim status and byte identity",
            }
            funpack_candidate = {
                "status": "not_applicable",
                "argv": funpack_candidate_argv,
                "reason": "unchanged non-optimized row; verified by Tzim status and byte identity",
            }
        else:
            funpack_source = run(funpack_source_argv, logs)
            funpack_candidate = run(funpack_candidate_argv, logs)
            if funpack_source.get("returncode") == 0 and unpack_source.is_file():
                row["external_checks"]["funpack_source_output"] = {"bytes": digest(unpack_source)[0], "sha256": digest(unpack_source)[1]}
            if funpack_candidate.get("returncode") == 0 and unpack_candidate.is_file():
                row["external_checks"]["funpack_candidate_output"] = {"bytes": digest(unpack_candidate)[0], "sha256": digest(unpack_candidate)[1]}
        row["external_checks"]["funpack_source"] = funpack_source
        row["external_checks"]["funpack_candidate"] = funpack_candidate
        row["external_checks"]["funpack_checksum_mode"] = "default checksum behavior; -C not used; fpack checksum behavior is comparator documentation only"

        try:
            direct = compare_fits(source, candidate)
            row["array_header_roundtrip"] = direct
        except Exception as error:
            row["array_header_roundtrip"] = {"ok": False, "reason": f"{type(error).__name__}: {error}"}
        if tzim_record.get("unchanged_nonoptimized"):
            row["funpack_array_header_roundtrip"] = {
                "ok": None,
                "status": "not_applicable",
                "reason": "unchanged non-optimized rows are verified by Tzim status and source/candidate byte identity",
            }
        elif unpack_source.is_file() and unpack_candidate.is_file():
            try:
                unpacked = compare_fits(
                    unpack_source, unpack_candidate, compare_untransformed_raw=False, allow_primary_filler=True
                )
                row["funpack_array_header_roundtrip"] = unpacked
            except Exception as error:
                row["funpack_array_header_roundtrip"] = {"ok": False, "reason": f"{type(error).__name__}: {error}"}
        else:
            row["funpack_array_header_roundtrip"] = {"ok": False, "reason": "funpack output missing"}

        candidate_size, candidate_sha = digest(candidate)
        row["tier2_tzim"]["candidate_bytes"] = candidate_size
        row["tier2_tzim"]["candidate_sha256"] = candidate_sha
        # The evidence artifact retains only hashes and oracle output, never the
        # survey bytes, transformed candidates, or funpack-expanded arrays. Drop
        # each large temporary before moving to the next registered product.
        cleanup_errors = []
        for temporary in (candidate, unpack_source, unpack_candidate):
            try:
                temporary.unlink(missing_ok=True)
            except OSError as error:
                cleanup_errors.append(f"{temporary.name}: {type(error).__name__}: {error}")
        if cleanup_errors:
            row["temporary_cleanup_errors"] = cleanup_errors
        print(f"[{order}/14] {product['id']}: {row['measurement_status']} / {row['tier2_tzim']['status']}", flush=True)
        rows.append(row)

    rows_path = out / "raw-rows.jsonl"
    rows_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8")
    (out / "external-tool-output.jsonl").write_text("".join(json.dumps(item, sort_keys=True) + "\n" for item in logs), encoding="utf-8")

    audit_stats = {"tier2_rows": len(audit_rows), "status_counts": dict(sorted(Counter(row.get("status", "missing") for row in audit_rows).items()))}
    audit_gains = [row["pct"] for row in audit_rows if row.get("status") == "tier2" and row.get("exact") and isinstance(row.get("pct"), (int, float))]
    tier1_gains = [row["tier1"]["gain_pct"] for row in rows if row.get("tier1", {}).get("status") == "measured_comparator"]
    tzim_gains = [row["tier2_tzim"]["gain_pct"] for row in rows if isinstance(row.get("tier2_tzim", {}).get("gain_pct"), (int, float))]
    aggregate = {
        "source_manifest_sha256": manifest_sha,
        "products_n": len(rows),
        "fetch_status_counts": dict(sorted(Counter(row.get("fetch_status", "missing") for row in rows).items())),
        "measurement_status_counts": dict(sorted(Counter(row.get("measurement_status", "missing") for row in rows).items())),
        "tzim_status_counts": dict(sorted(Counter(row.get("tier2_tzim", {}).get("status", "missing") for row in rows).items())),
        "existing_fits_measure_instrument": audit_stats,
        "existing_fits_measure_tier2_gain_pct": {
            "n": len(audit_gains), "median": statistics.median(audit_gains) if audit_gains else None,
            "min": min(audit_gains) if audit_gains else None, "max": max(audit_gains) if audit_gains else None,
        },
        "existing_tile_tier1_comparator_gain_pct": {
            "n": len(tier1_gains), "median": statistics.median(tier1_gains) if tier1_gains else None,
            "min": min(tier1_gains) if tier1_gains else None, "max": max(tier1_gains) if tier1_gains else None,
        },
        "tzim_tier2_gain_pct": {
            "n": len(tzim_gains), "median": statistics.median(tzim_gains) if tzim_gains else None,
            "min": min(tzim_gains) if tzim_gains else None, "max": max(tzim_gains) if tzim_gains else None,
        },
        "fitsverify_candidate_nonzero": sum(1 for row in rows if row.get("tier2_tzim", {}).get("status") == "optimized" and row.get("external_checks", {}).get("fitsverify_candidate", {}).get("returncode") != 0),
        "array_header_roundtrip_passes": sum(1 for row in rows if row.get("array_header_roundtrip", {}).get("ok") is True),
        "array_header_roundtrip_failures": sum(1 for row in rows if row.get("array_header_roundtrip", {}).get("ok") is False),
        "funpack_roundtrip_passes": sum(1 for row in rows if row.get("funpack_array_header_roundtrip", {}).get("ok") is True),
        "funpack_roundtrip_not_applicable": sum(1 for row in rows if row.get("funpack_array_header_roundtrip", {}).get("status") == "not_applicable"),
        "funpack_failures": sum(1 for row in rows if row.get("funpack_array_header_roundtrip", {}).get("ok") is False),
        "measurement_script": ".github/scripts/fixed-fits-measure.py",
        "existing_two_tier_script": ".github/scripts/fits-measure.py",
        "derived_tier1_is_comparator_only": True,
        "fpack_checksum_behavior": "documentation/comparator only; never used to admit or repair a candidate",
    }
    (out / "aggregate.json").write_text(json.dumps(aggregate, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    summary = [
        "# Fixed #59 FITS measurement",
        "",
        f"- Frozen 14-product source manifest SHA-256: `{manifest_sha}`",
        f"- Fetch statuses: `{json.dumps(aggregate['fetch_status_counts'], sort_keys=True)}`",
        f"- Measurement statuses: `{json.dumps(aggregate['measurement_status_counts'], sort_keys=True)}`",
        f"- Tzim Tier 2 statuses: `{json.dumps(aggregate['tzim_status_counts'], sort_keys=True)}`",
        f"- Existing Tier 2 audit outputs: {len(audit_rows)}",
        f"- Astropy Tier 2 median: {aggregate['existing_fits_measure_tier2_gain_pct']['median']}",
        f"- Existing tile Tier 1 comparator median: {aggregate['existing_tile_tier1_comparator_gain_pct']['median']}",
        f"- Tzim Tier 2 median among strictly smaller candidates: {aggregate['tzim_tier2_gain_pct']['median']}",
        f"- Direct array/header comparisons: {aggregate['array_header_roundtrip_passes']} passed, {aggregate['array_header_roundtrip_failures']} failed, {len(rows) - aggregate['array_header_roundtrip_passes'] - aggregate['array_header_roundtrip_failures']} not measured.",
        f"- funpack array/header comparisons: {aggregate['funpack_roundtrip_passes']} passed, {aggregate['funpack_failures']} failed, {aggregate['funpack_roundtrip_not_applicable']} not applicable for unchanged non-optimized rows.",
        "",
        "All 14 source rows remain in `raw-rows.jsonl`. The Tier 1 outputs are comparators derived from the corresponding source product and are not additional inputs. No source bytes, candidates, or ROSAT data are included in this evidence directory.",
        "",
        "## All fixed product rows — identity and round-trip disposition",
        "",
        "No row is dropped based on outcome. Hashes and check statuses are reported; FITS and ROSAT data bytes are not included.",
        "",
        "| Fixed product | Input bytes / SHA-256 | Tzim status; candidate bytes / SHA-256 | Direct HDU/content comparison | funpack round trip | fitsverify candidate |",
        "| --- | ---: | --- | --- | --- | --- |",
    ]
    for row in rows:
        tzim = row.get("tier2_tzim") or {}
        direct = row.get("array_header_roundtrip") or {}
        funpack_roundtrip = row.get("funpack_array_header_roundtrip") or {}
        checks = row.get("external_checks") or {}
        candidate_fitsverify = checks.get("fitsverify_candidate") or {}
        if direct.get("ok") is True:
            direct_status = "PASS"
        elif direct.get("ok") is False:
            direct_status = "FAIL"
        else:
            direct_status = "not measured"
        if funpack_roundtrip.get("status") == "not_applicable":
            funpack_status = "not applicable (unchanged non-optimized row)"
        elif funpack_roundtrip.get("ok") is True:
            funpack_status = "pass"
        elif funpack_roundtrip.get("ok") is False:
            funpack_status = "FAIL"
        else:
            funpack_status = "not run"
        if candidate_fitsverify.get("returncode") is None and candidate_fitsverify.get("error"):
            fitsverify_status = f"error: {candidate_fitsverify['error']}"
        elif tzim.get("unchanged_nonoptimized"):
            fitsverify_status = f"exit {candidate_fitsverify.get('returncode')} (unchanged source diagnostic)"
        else:
            fitsverify_status = f"exit {candidate_fitsverify.get('returncode')}"
        summary.append(
            f"| `{row.get('product_id', 'unknown')}` | {row.get('input_bytes', '—')} / "
            f"`{row.get('input_sha256', '—')}` | {tzim.get('status', 'not-run')}; "
            f"{tzim.get('candidate_bytes', '—')} / `{tzim.get('candidate_sha256', '—')}` | "
            f"{direct_status} | {funpack_status} | {fitsverify_status} |"
        )

    def diagnostic_excerpt(value: object, limit: int = 600) -> str:
        text = str(value or "").strip()
        if not text:
            return "(empty)"
        if len(text) > limit:
            return text[:limit] + "\n...[truncated]"
        return text

    diagnostics = []
    for row in rows:
        product_id = row.get("product_id", "unknown-product")
        if row.get("fetch_status") != "downloaded":
            diagnostics.extend([
                f"### Fetch failure: {product_id}",
                f"- Source identity: `{row.get('source_uri')}`",
                f"- Exact requested URL: `{row.get('requested_url')}`",
                f"- Error: {diagnostic_excerpt(row.get('fetch_error'))}",
                f"- Needed owner action: {row.get('needed_owner_action') or 'not recorded'}",
                "",
            ])
        tzim = row.get("tier2_tzim") or {}
        if tzim.get("error") or tzim.get("returncode") not in (None, 0) or tzim.get("status") == "failed":
            diagnostics.extend([
                f"### Tzim Tier 2 failure: {product_id}",
                f"- Source identity: `{row.get('source_uri')}`",
                f"- Exact requested URL: `{row.get('requested_url')}`",
                f"- Error: {diagnostic_excerpt(tzim.get('error'))}",
                f"- Return code: `{tzim.get('returncode')}`",
                f"- Command: `{json.dumps(tzim.get('argv', []), ensure_ascii=False)}`",
            ])
            for label in ("stderr", "stdout"):
                text = diagnostic_excerpt(tzim.get(label))
                if text != "(empty)":
                    diagnostics.extend([f"- {label}:", "", "```text", text, "```"])
            diagnostics.append("")
        for name, check in (row.get("external_checks") or {}).items():
            if not isinstance(check, dict) or (not check.get("error") and check.get("returncode") in (None, 0)):
                continue
            if name.endswith("_source"):
                heading = f"### External source diagnostic: {product_id} / {name}"
            elif (row.get("tier2_tzim") or {}).get("unchanged_nonoptimized"):
                heading = f"### Inherited external diagnostic (unchanged source): {product_id} / {name}"
            else:
                heading = f"### External check failure: {product_id} / {name}"
            diagnostics.extend([
                heading,
                f"- Error: {diagnostic_excerpt(check.get('error'))}",
                f"- Return code: `{check.get('returncode')}`",
            ])
            for label in ("stderr", "stdout"):
                text = diagnostic_excerpt(check.get(label))
                if text != "(empty)":
                    diagnostics.extend([f"- {label}:", "", "```text", text, "```"])
            diagnostics.append("")
        for label in ("array_header_roundtrip", "funpack_array_header_roundtrip"):
            check = row.get(label) or {}
            if check.get("ok") is False:
                details = json.dumps(check, sort_keys=True, ensure_ascii=False)
                diagnostics.extend([
                    f"### Oracle failure: {product_id} / {label}",
                    f"- Reason: {diagnostic_excerpt(check.get('reason'))}",
                    f"- Oracle details: `{diagnostic_excerpt(details)}`",
                    "",
                ])

    if diagnostics:
        summary.extend(["", "## Per-product diagnostics", "", *diagnostics])
    (out / "summary.md").write_text("\n".join(summary) + "\n", encoding="utf-8")

    failures = []
    for row in rows:
        if row.get("fetch_status") != "downloaded":
            failures.append(f"{row['product_id']}: fetch failed at {row.get('requested_url')}: {row.get('fetch_error')}")
        if row.get("measurement_status") in (
            "input_missing", "manifest_integrity_failure", "tzim_failed", "measurement_dependency_failed"
        ):
            failures.append(f"{row['product_id']}: {row['measurement_status']} {row.get('measurement_dependency_error') or row.get('fetch_error') or row.get('tier2_tzim', {}).get('error')}")
        if row.get("measurement_dependency_error"):
            failures.append(f"{row['product_id']}: measurement dependency unavailable: {row['measurement_dependency_error']}")
        if row.get("tier2_tzim", {}).get("error"):
            failures.append(f"{row['product_id']}: tzim failed: {row['tier2_tzim']['error']}")
        if row.get("array_header_roundtrip", {}).get("ok") is False:
            failures.append(f"{row['product_id']}: reconstructed array/header mismatch: {row['array_header_roundtrip'].get('reason')}")
        if row.get("funpack_array_header_roundtrip", {}).get("ok") is False:
            failures.append(f"{row['product_id']}: funpack array/header mismatch: {row['funpack_array_header_roundtrip'].get('reason')}")
        unchanged_nonoptimized = bool(row.get("tier2_tzim", {}).get("unchanged_nonoptimized"))
        for name in ("fitsverify_source", "fitsverify_candidate", "funpack_source", "funpack_candidate"):
            check = row.get("external_checks", {}).get(name)
            if check is None or (not check.get("error") and check.get("returncode") in (None, 0)):
                continue
            # Source diagnostics establish provenance, not candidate validity. A
            # refused/no-gain row is judged by its Tzim status and byte identity;
            # per the FITS contract it is not required to pass external readers.
            if name == "fitsverify_source" or unchanged_nonoptimized:
                continue
            detail = check.get("error") or check.get("stderr") or check.get("stdout") or check.get("returncode")
            failures.append(f"{row['product_id']}: {name} failed: {diagnostic_excerpt(detail)}")
        if row.get("tier1_error"):
            failures.append(f"{row['product_id']}: existing tile Tier 1 comparator failed ({row['tier1_error']})")
        if row.get("temporary_cleanup_errors"):
            failures.append(f"{row['product_id']}: temporary cleanup failed: {row['temporary_cleanup_errors']}")
    if failures:
        (out / "blockers.txt").write_text("\n".join(failures) + "\n", encoding="utf-8")
        print("\n".join(f"BLOCKER: {item}" for item in failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
