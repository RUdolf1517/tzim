#!/usr/bin/env python3
"""Measure the frozen 21-document #57 PDF manifest without post-gain selection."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
from collections import Counter
from pathlib import Path

EXPECTED_IDS = {
    f"{producer}/{slug}"
    for producer in ("latex", "libreoffice", "quartz")
    for slug in ("readme", "changelog", "guarantees", "formats", "roadmap", "measurement-status")
} | {"scans/c03-29", "scans/linn", "scans/typewriter"}


def load_pdf_measure(path: Path):
    spec = importlib.util.spec_from_file_location("tzim_pdf_measure", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not import existing PDF audit instrument {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def digest(path: Path) -> tuple[int, str]:
    h = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            size += len(chunk)
            h.update(chunk)
    return size, h.hexdigest()


def run(argv: list[str], log: list[dict], timeout: int = 300) -> subprocess.CompletedProcess:
    try:
        result = subprocess.run(argv, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
        log.append({"argv": argv, "returncode": result.returncode, "stdout": result.stdout, "stderr": result.stderr})
        return result
    except Exception as error:  # record the failed tool and keep every registered source row
        message = f"{type(error).__name__}: {error}"
        log.append({"argv": argv, "error": message})
        return subprocess.CompletedProcess(argv, 127, "", message)


def render_pages(path: Path, page_indices: list[int], dpi: int = 100) -> dict:
    import pymupdf

    document = pymupdf.open(path)
    try:
        count = len(document)
        if count == 0:
            raise RuntimeError("MuPDF opened a PDF with zero pages")
        if any(index < 0 or index >= count for index in page_indices):
            raise RuntimeError(f"pre-registered page indices {page_indices} do not fit {count} pages")
        rendered = []
        for index in page_indices:
            pixmap = document[index].get_pixmap(dpi=dpi, alpha=False, annots=True)
            rendered.append({
                "page_index_zero_based": index,
                "width": pixmap.width,
                "height": pixmap.height,
                "samples_sha256": hashlib.sha256(pixmap.samples).hexdigest(),
            })
        return {"pages": count, "dpi": dpi, "samples": rendered}
    finally:
        document.close()


def sample_indices(page_count: int) -> list[int]:
    if page_count <= 0:
        return []
    # Fixed before measurement: first, upper-middle, last; duplicates are removed
    # for one- and two-page documents.
    return sorted({0, page_count // 2, page_count - 1})


def order_statistic(values: list[float], numerator: int, denominator: int) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    return ordered[(len(ordered) * numerator) // denominator]


def optional_version(module_name: str, attribute: str) -> str:
    try:
        return str(getattr(__import__(module_name), attribute))
    except Exception as error:
        return f"unavailable ({type(error).__name__}: {error})"


def aggregate(rows: list[dict]) -> dict:
    accepted = [row for row in rows if row.get("classification") == "accepted"]
    optimized = [row for row in rows if row.get("tzim", {}).get("status") == "optimized"]
    gain_rows = [
        row for row in optimized
        if isinstance(row.get("bytes"), int) and isinstance(row.get("tzim", {}).get("bytes"), int)
    ]
    gains = [row["tzim"]["gain_pct"] for row in optimized if isinstance(row.get("tzim", {}).get("gain_pct"), (int, float))]
    optimized_source_bytes = sum(row["bytes"] for row in gain_rows)
    optimized_candidate_bytes = sum(row["tzim"]["bytes"] for row in gain_rows)
    tzim_saved_bytes = optimized_source_bytes - optimized_candidate_bytes
    def stats(key: str) -> dict:
        values = [row[key] for row in accepted if isinstance(row.get(key), (int, float))]
        return {
            "n": len(values),
            "median_upper": order_statistic(values, 1, 2),
            "q1_index": order_statistic(values, 1, 4),
            "q3_index": order_statistic(values, 3, 4),
            "share_positive": (sum(value > 0 for value in values) / len(values)) if values else None,
        }
    refusal_reasons = dict(sorted(Counter(
        row.get("reason", "unspecified") for row in rows if row.get("classification") == "refused"
    ).items()))
    return {
        "row_count": len(rows),
        "statuses": dict(sorted(Counter(row.get("classification", "missing") for row in rows).items())),
        "refusal_reasons": refusal_reasons,
        "accepted_n": len(accepted),
        "tzim_optimized_n": len(optimized),
        "faithful_pct": stats("faithful_pct"),
        "qpdf_allowed_pct": stats("qpdf_pct"),
        "forbidden_repack_pct": stats("repack_pct"),
        "tzim_gain_pct": {
            "n": len(gains),
            "median": statistics.median(gains) if gains else None,
            "mean": statistics.fmean(gains) if gains else None,
        },
        "tzim_optimized_source_bytes": optimized_source_bytes,
        "tzim_optimized_candidate_bytes": optimized_candidate_bytes,
        "tzim_saved_bytes": tzim_saved_bytes,
        "tzim_weighted_gain_pct": (
            100.0 * tzim_saved_bytes / optimized_source_bytes if optimized_source_bytes else None
        ),
        "render_differences": sum(
            1 for row in rows for comparison in row.get("render_comparisons", [])
            if comparison.get("compared") and not comparison.get("identical")
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


def accepted_faithful_rows(rows: list[dict]) -> list[dict]:
    """Compact, owner-reviewable raw inputs for the registered accepted-subset metric."""
    return [
        {
            "id": row.get("id"),
            "bytes": row.get("bytes"),
            "sha256": row.get("sha256"),
            "streams": row.get("streams"),
            "flate_bytes": row.get("flate_bytes"),
            "flate_after": row.get("flate_after"),
            "faithful_pct": row.get("faithful_pct"),
            "faithful_error": row.get("faithful_error"),
        }
        for row in rows if row.get("classification") == "accepted"
    ]


def threshold_disposition(rows: list[dict], summary: dict, pdf_feature_delta_bytes: int | None = None) -> dict:
    """Apply #57's pre-registered rule to accepted-subset `faithful_pct`, not Tzim gains."""
    accepted_n = summary["accepted_n"]
    row_count = summary["row_count"]
    faithful = summary["faithful_pct"]
    accepted_share = 100.0 * accepted_n / row_count if row_count else None
    blockers = []
    for key, label in (
        ("faithful_pct", "accepted-subset faithful_pct"),
        ("qpdf_allowed_pct", "permitted-qpdf distribution"),
        ("forbidden_repack_pct", "forbidden-repack distribution"),
    ):
        if summary[key]["n"] != accepted_n:
            blockers.append(f"{label} has {summary[key]['n']} values for {accepted_n} accepted rows")
    if summary["render_differences"]:
        blockers.append(f"{summary['render_differences']} registered render comparisons differ")
    if summary["qpdf_source_check_failures"]:
        blockers.append(f"{summary['qpdf_source_check_failures']} accepted source PDFs failed qpdf --check")
    if summary["qpdf_candidate_check_failures"]:
        blockers.append(f"{summary['qpdf_candidate_check_failures']} qpdf candidate checks failed")
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
            "pdf_feature_delta_bytes": pdf_feature_delta_bytes,
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
            "pdf_feature_delta_bytes": pdf_feature_delta_bytes,
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
        if pdf_feature_delta_bytes is None:
            status = "not_applied"
            decision = "threshold needs the registered binary-delta comparison"
            rule_result = "pending_binary_delta"
            reason = "faithful_pct median is between 1% and 3%; no final-head PDF feature delta was supplied"
        elif pdf_feature_delta_bytes > 70 * 1024:
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
        "pdf_feature_delta_bytes": pdf_feature_delta_bytes,
        "thresholds": {
            "median_at_least_for_admission_pct": 3.0,
            "accepted_share_at_least_for_admission_pct": 10.0,
            "median_below_refusal_pct": 1.0,
            "mid_band_upper_exclusive_pct": 3.0,
            "mid_band_max_pdf_feature_delta_bytes": 70 * 1024,
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--root", required=True, help="directory containing downloaded ubuntu/ and quartz/ artifacts")
    parser.add_argument("--out", required=True)
    parser.add_argument("--tzim", required=True)
    parser.add_argument("--qpdf", required=True)
    parser.add_argument("--pdf-measure", default=".github/scripts/pdf-measure.py")
    parser.add_argument("--pdf-feature-delta-bytes", type=int, default=None,
                        help="final-head PDF feature binary delta for the registered 1–3% mid-band rule")
    parser.add_argument("--expected-source-sha", required=True)
    args = parser.parse_args()

    manifest_path = Path(args.manifest)
    root = Path(args.root).resolve()
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    manifest_bytes = manifest_path.read_bytes()
    manifest = json.loads(manifest_bytes)
    manifest_sha = hashlib.sha256(manifest_bytes).hexdigest()
    outputs = manifest.get("outputs", [])
    if (
        len(outputs) != 21
        or manifest.get("output_count") != 21
        or {row.get("id") for row in outputs} != EXPECTED_IDS
        or len({row.get("id") for row in outputs}) != 21
    ):
        raise SystemExit("frozen PDF manifest must contain exactly the 21 owner-pre-registered outputs")
    if manifest.get("manifest_frozen_before_gain") is not True:
        raise SystemExit("PDF input manifest was not frozen before gain was measured")
    if manifest.get("source_decision_comment") != "6498702204":
        raise SystemExit("manifest does not identify the owner source decision")
    expected_manifest_sha = manifest.get("expected_repository_source_sha", manifest.get("repository_source_sha"))
    if expected_manifest_sha != args.expected_source_sha:
        raise SystemExit(
            f"frozen manifest expected SHA {expected_manifest_sha!r}, "
            f"not the measurement SHA {args.expected_source_sha!r}"
        )

    try:
        pdf_audit = load_pdf_measure(Path(args.pdf_measure))
        pdf_audit_error = None
    except Exception as error:
        pdf_audit = None
        pdf_audit_error = f"{type(error).__name__}: {error}"
    qpdf = str(Path(args.qpdf).resolve())
    tzim = str(Path(args.tzim).resolve())
    qpdf_version = run([qpdf, "--version"], [])
    tool_log: list[dict] = [{
        "argv": [qpdf, "--version"],
        "returncode": qpdf_version.returncode,
        "stdout": qpdf_version.stdout,
        "stderr": qpdf_version.stderr,
    }]

    rows: list[dict] = []
    for order, item in enumerate(outputs, start=1):
        row = {
            "order": order,
            "id": item.get("id"),
            "path": item.get("path"),
            "producer": item.get("producer"),
            "generation_status": item.get("status"),
            "generation_row": item,
            "frozen_manifest_sha256": manifest_sha,
            "classification": "generation_failed",
            "reason": item.get("error", "generation did not produce a file"),
            "commands": [],
            "render_comparisons": [],
        }
        if item.get("status") != "generated" or item.get("generation_source_identity_valid") is not True:
            if item.get("status") == "generated":
                row["classification"] = "generation_failed"
                row["reason"] = "generation row is not bound to the expected source SHA"
            rows.append(row)
            continue
        source_path = root / item["artifact_path"]
        if not source_path.is_file():
            row["classification"] = "integrity_failed"
            row["reason"] = f"generated artifact missing: {item['artifact_path']}"
            rows.append(row)
            continue
        size, sha = digest(source_path)
        row.update(bytes=size, sha256=sha)
        if size != item.get("bytes") or sha != item.get("sha256"):
            row["classification"] = "integrity_failed"
            row["reason"] = "generated PDF bytes differ from the frozen source manifest"
            rows.append(row)
            continue

        scratch_dir = out / "tool-work"
        scratch_dir.mkdir(exist_ok=True)
        qpdf_check = run([qpdf, "--check", str(source_path)], tool_log)
        row["original_qpdf_check"] = {"returncode": qpdf_check.returncode, "stdout": qpdf_check.stdout, "stderr": qpdf_check.stderr}
        if qpdf_check.returncode != 0:
            xref = run([qpdf, "--show-xref", str(source_path)], tool_log, timeout=60)
            row["qpdf_xref_diagnostic"] = {
                "returncode": xref.returncode,
                "stdout": xref.stdout,
                "stderr": xref.stderr,
            }
            zero_offset_objects = sorted({
                (int(object_number), int(generation))
                for object_number, generation in re.findall(
                    r"\(object (\d+) (\d+)\): object has offset 0", qpdf_check.stderr
                )
            })
            object_diagnostics = {}
            for object_number, generation in zero_offset_objects:
                object_output = run(
                    [qpdf, f"--show-object={object_number},{generation}", str(source_path)],
                    tool_log,
                    timeout=60,
                )
                object_diagnostics[f"{object_number} {generation}"] = {
                    "returncode": object_output.returncode,
                    "stdout": object_output.stdout,
                    "stderr": object_output.stderr,
                }
            row["qpdf_zero_offset_objects"] = object_diagnostics
            try:
                import pymupdf

                document = pymupdf.open(source_path)
                try:
                    references = []
                    reference_patterns = {
                        f"{object_number} {generation}": re.compile(
                            rf"(?<!\d){object_number}\s+{generation}\s+R(?!\d)"
                        )
                        for object_number, generation in zero_offset_objects
                    }
                    for referencing_xref in range(1, document.xref_length()):
                        try:
                            object_text = document.xref_object(referencing_xref, compressed=False)
                        except Exception:
                            continue
                        targets = [
                            object_id for object_id, pattern in reference_patterns.items()
                            if pattern.search(object_text)
                        ]
                        if targets:
                            references.append({
                                "referencing_object": referencing_xref,
                                "targets": targets,
                                "object": object_text,
                            })
                    row["pymupdf_zero_offset_reference_diagnostic"] = {
                        "page_count": len(document),
                        "xref_length": document.xref_length(),
                        "zero_offset_objects": {
                            object_id: document.xref_object(int(object_id.split()[0]), compressed=False)
                            for object_id in object_diagnostics
                        },
                        "references": references,
                    }
                finally:
                    document.close()
            except Exception as error:
                row["pymupdf_zero_offset_reference_diagnostic"] = {
                    "error": f"{type(error).__name__}: {error}"
                }
        if pdf_audit is None:
            status, reason, info = (
                "measurement_dependency_failed",
                "PDF audit instrument unavailable",
                {"classification_error": pdf_audit_error},
            )
        else:
            try:
                status, reason, info = pdf_audit.classify(str(source_path), str(scratch_dir / "pikepdf-check.txt"))
            except Exception as error:
                status, reason, info = "refused", "classification_error", {"classification_error": f"{type(error).__name__}: {error}"}
        row.update(classification=status, reason=reason, classification_info=info)
        if pdf_audit_error:
            row["pdf_audit_import_error"] = pdf_audit_error
        if qpdf_check.returncode != 0:
            row["classification"] = "refused"
            row["reason"] = "qpdf_check_failed"
        if status == "accepted" and qpdf_check.returncode == 0:
            try:
                stream_count, flate_bytes, flate_after = pdf_audit.faithful(str(source_path))
                row.update(
                    streams=stream_count,
                    flate_bytes=flate_bytes,
                    flate_after=flate_after,
                    faithful_pct=100.0 * (flate_bytes - flate_after) / size if size else 0.0,
                )
            except Exception as error:
                row["faithful_error"] = f"{type(error).__name__}: {error}"

            tag = f"{order:02d}-{item['id'].replace('/', '-') }"
            allowed = out / "tool-work" / f"{tag}.allowed.pdf"
            repack = out / "tool-work" / f"{tag}.repack.pdf"
            for label, destination, object_stream_mode in (
                ("allowed", allowed, "preserve"),
                ("repack", repack, "generate"),
            ):
                argv = [
                    qpdf, "--recompress-flate", "--compression-level=9",
                    f"--object-streams={object_stream_mode}", str(source_path), str(destination),
                ]
                command = run(argv, tool_log)
                row[f"qpdf_{label}_command"] = argv
                row[f"qpdf_{label}_run"] = {"returncode": command.returncode, "stdout": command.stdout, "stderr": command.stderr}
                if command.returncode == 0 and destination.is_file():
                    out_size, out_sha = digest(destination)
                    row[f"qpdf_{label}_bytes"] = out_size
                    row[f"qpdf_{label}_sha256"] = out_sha
                    check = run([qpdf, "--check", str(destination)], tool_log)
                    row[f"qpdf_{label}_check"] = {"returncode": check.returncode, "stdout": check.stdout, "stderr": check.stderr}
                else:
                    row[f"qpdf_{label}_check"] = {
                        "returncode": command.returncode if command.returncode != 0 else 255,
                        "stdout": command.stdout,
                        "stderr": command.stderr,
                        "error": "qpdf reported success but produced no comparator file" if command.returncode == 0 else None,
                    }
            if row.get("qpdf_allowed_run", {}).get("returncode") == 0 and row.get("qpdf_allowed_bytes") is not None:
                row["qpdf_pct"] = 100.0 * (size - row["qpdf_allowed_bytes"]) / size if size else 0.0
                row["repack_pct"] = 100.0 * (size - row["qpdf_repack_bytes"]) / size if size and row.get("qpdf_repack_bytes") is not None else None
                try:
                    row["payloads_equal"] = pdf_audit.payloads(str(source_path)) == pdf_audit.payloads(str(allowed))
                except Exception as error:
                    row["payloads_equal"] = False
                    row["payload_compare_error"] = f"{type(error).__name__}: {error}"

        # Exercise the actual handler on every output. The file is a private copy;
        # the generated source artifact is never modified.
        tzim_dir = out / "tzim-work"
        tzim_dir.mkdir(exist_ok=True)
        candidate = tzim_dir / f"{order:02d}.pdf"
        shutil.copyfile(source_path, candidate)
        before_size, before_sha = digest(candidate)
        argv = [tzim, "--formats", "pdf", "--jobs", "1", "--json", "--no-progress", str(candidate)]
        tzim_run = run(argv, tool_log)
        tzim_record = {"argv": argv, "returncode": tzim_run.returncode, "stdout": tzim_run.stdout, "stderr": tzim_run.stderr}
        json_rows = []
        for line in tzim_run.stdout.splitlines():
            try:
                json_rows.append(json.loads(line))
            except json.JSONDecodeError:
                continue
        file_row = next((entry for entry in json_rows if entry.get("path") == str(candidate)), None)
        tzim_record["file_row"] = file_row
        after_size, after_sha = digest(candidate)
        tzim_record.update(bytes=after_size, sha256=after_sha)
        if tzim_run.returncode != 0 or file_row is None:
            tzim_record["status"] = "failed"
            tzim_record["error"] = tzim_run.stderr.strip() or "tzim returned non-zero or emitted no file row"
        else:
            tzim_record["status"] = file_row.get("status", "unknown")
            if after_sha != before_sha and (after_size >= before_size or tzim_record["status"] != "optimized"):
                tzim_record["error"] = "tzim changed bytes without a strictly smaller optimized result"
            if tzim_record["status"] == "optimized":
                tzim_record["gain_pct"] = 100.0 * (before_size - after_size) / before_size if before_size else 0.0
        row["tzim"] = tzim_record
        if tzim_record.get("error"):
            row["tzim_measurement_error"] = tzim_record["error"]
        if after_sha != before_sha:
            candidate_check = run([qpdf, "--check", str(candidate)], tool_log)
            row["tzim_qpdf_check"] = {"returncode": candidate_check.returncode, "stdout": candidate_check.stdout, "stderr": candidate_check.stderr}
        else:
            row["tzim_qpdf_check"] = {
                "returncode": None,
                "not_run": True,
                "reason": "Tzim left the PDF unchanged; the original qpdf check is recorded separately",
            }

        # The render sample is fixed for every generated document, not selected
        # after gains are known. For each available changed candidate, render the
        # exact first/middle/last page indices from the original document.
        try:
            import pymupdf
            doc = pymupdf.open(source_path)
            page_count = len(doc)
            doc.close()
            indices = sample_indices(page_count)
            original_render = render_pages(source_path, indices)
            row["render_sample"] = {"pre_registered": "first,middle,last", "indices_zero_based": indices, "original": original_render}
            comparisons = []
            if after_sha != before_sha:
                candidate_render = render_pages(candidate, indices)
                comparisons.append({"candidate": "tzim", "compared": True,
                                    "identical": candidate_render["pages"] == original_render["pages"] and candidate_render["samples"] == original_render["samples"],
                                    "candidate_render": candidate_render})
            if status == "accepted" and row.get("qpdf_allowed_run", {}).get("returncode") == 0 and row.get("qpdf_allowed_bytes") is not None:
                allowed_path = out / "tool-work" / f"{order:02d}-{item['id'].replace('/', '-') }.allowed.pdf"
                if allowed_path.is_file():
                    qpdf_render = render_pages(allowed_path, indices)
                    comparisons.append({"candidate": "qpdf-permitted", "compared": True,
                                        "identical": qpdf_render["pages"] == original_render["pages"] and qpdf_render["samples"] == original_render["samples"],
                                        "candidate_render": qpdf_render})
            row["render_comparisons"] = comparisons
        except Exception as error:
            row["render_sample"] = {"pre_registered": "first,middle,last", "error": f"{type(error).__name__}: {error}"}
            row["render_error"] = f"{type(error).__name__}: {error}"

        rows.append(row)
        print(f"[{order}/21] {item['path']}: {row['classification']} / {row.get('tzim', {}).get('status', 'not-run')}", flush=True)

    rows_path = out / "raw-rows.jsonl"
    with rows_path.open("w", encoding="utf-8") as stream:
        for row in rows:
            stream.write(json.dumps(row, sort_keys=True) + "\n")
    (out / "qpdf-and-tzim-tool-output.jsonl").write_text(
        "".join(json.dumps(item, sort_keys=True) + "\n" for item in tool_log), encoding="utf-8"
    )
    aggregate_result = aggregate(rows)
    aggregate_result.update({
        "source_manifest_sha256": manifest_sha,
        "measurement_script": ".github/scripts/fixed-pdf-measure.py",
        "existing_audit_instrument": ".github/scripts/pdf-measure.py",
        "qpdf_version": (
            qpdf_version.stdout.strip()
            if qpdf_version.returncode == 0
            else f"unavailable (exit {qpdf_version.returncode}): {qpdf_version.stderr or qpdf_version.stdout}"
        ),
        "qpdf_version_returncode": qpdf_version.returncode,
        "pikepdf_version": optional_version("pikepdf", "__version__"),
        "pymupdf_version": optional_version("pymupdf", "VersionBind"),
        "pdf_audit_import_error": pdf_audit_error,
        "qpdf_source": {
            "tag": "v12.4.2",
            "tag_object": "ceb29d85877b04a049dd4f6623cf4507c728d10e",
            "archive_sha256": "88ddeb5f25c6e9156f3113fc3994d3a5e0cdfeee398cd09dd00fabc9dac9f573",
        },
    })
    aggregate_result["pre_registered_threshold_disposition"] = threshold_disposition(
        rows, aggregate_result, args.pdf_feature_delta_bytes
    )
    aggregate_path = out / "aggregate.json"
    aggregate_path.write_text(json.dumps(aggregate_result, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    def distribution_line(label: str, key: str) -> str:
        values = aggregate_result[key]
        if values["n"] == 0:
            return f"- {label}: n=0 (empty; no percentage is reported)."
        return (
            f"- {label}: n={values['n']}, median={values['median_upper']}%, "
            f"Q1={values['q1_index']}%, Q3={values['q3_index']}%, "
            f"share > 0={100.0 * values['share_positive']:.2f}%."
        )

    threshold_result = aggregate_result["pre_registered_threshold_disposition"]
    faithful_rows_markdown = []
    for faithful_row in accepted_faithful_rows(rows):
        faithful_pct = faithful_row.get("faithful_pct")
        faithful_pct_text = str(faithful_pct) if isinstance(faithful_pct, (int, float)) else "not measured"
        faithful_rows_markdown.append(
            f"| `{faithful_row.get('id', 'unknown')}` | {faithful_row.get('bytes', '—')} | "
            f"`{faithful_row.get('sha256', '—')}` | {faithful_row.get('streams', '—')} | "
            f"{faithful_row.get('flate_bytes', '—')} | {faithful_row.get('flate_after', '—')} | "
            f"{faithful_pct_text} | {faithful_row.get('faithful_error') or '—'} |"
        )
    markdown = [
        "# Fixed #57 PDF measurement",
        "",
        f"- Frozen source manifest SHA-256: `{manifest_sha}`",
        f"- Rows: {len(rows)} (all 21 pre-registered outputs are retained)",
        f"- Classification counts: `{json.dumps(aggregate_result['statuses'], sort_keys=True)}`",
        f"- Narrow-slice accepted: {aggregate_result['accepted_n']} ({100.0 * aggregate_result['accepted_n'] / len(rows):.2f}% of the frozen corpus).",
        f"- Refusals by reason: `{json.dumps(aggregate_result['refusal_reasons'], sort_keys=True)}`",
        distribution_line("Pre-registered accepted-subset faithful_pct (flate2 level 9)", "faithful_pct"),
        distribution_line("Permitted qpdf distribution (object streams preserved)", "qpdf_allowed_pct"),
        distribution_line("Forbidden qpdf repack comparator distribution (object streams generated)", "forbidden_repack_pct"),
        f"- Pre-registered #57 disposition: **{threshold_result['status']}** — {threshold_result['decision']}. "
        f"Metric: `{threshold_result['metric']}`; accepted share {threshold_result.get('accepted_share_of_corpus_pct')}%; "
        f"faithful median {threshold_result.get('faithful_median_pct')}%.",
        f"- Tzim optimized: {aggregate_result['tzim_optimized_n']}/{len(rows)} rows; "
        f"actual savings {aggregate_result['tzim_saved_bytes']:,} bytes from "
        f"{aggregate_result['tzim_optimized_source_bytes']:,} optimized-input bytes "
        f"({aggregate_result['tzim_weighted_gain_pct']}% weighted).",
        f"- Per-file gain among optimized rows: median {aggregate_result['tzim_gain_pct']['median']}%, "
        f"mean {aggregate_result['tzim_gain_pct']['mean']}%.",
        f"- Render differences: {aggregate_result['render_differences']}",
        f"- qpdf candidate check failures: {aggregate_result['qpdf_candidate_check_failures']}",
        "",
        "## Accepted-subset `faithful_pct` rows — raw recomputation inputs",
        "",
        "The table below is posted directly in this PR report. Each value uses the registered formula `100 * (flate_bytes - flate_after) / bytes`; all accepted rows are shown, including a missing value/error if one occurred.",
        "",
        "| Fixed ID | Input bytes | Input SHA-256 | Streams | `flate_bytes` | `flate_after` | `faithful_pct` | Error |",
        "| --- | ---: | --- | ---: | ---: | ---: | ---: | --- |",
        *faithful_rows_markdown,
        "",
        "`raw-rows.jsonl` and `qpdf-and-tzim-tool-output.jsonl` retain every result and command. "
        "The source manifest was fixed before any gain was computed. The forbidden repack output is a comparator only; it is not an optimization proposal.",
        "",
        "## Pre-registered per-document identities and checks",
        "",
        "The table is a compact review view; full commands, manifests, render hashes and independent recomputation remain in the evidence artifact.",
        "",
        "| Fixed ID | Input bytes / SHA-256 | Classification | Tzim status; output bytes / SHA-256 | Tzim gain | qpdf candidate check | MuPDF first/middle/last |",
        "| --- | ---: | --- | --- | ---: | --- | --- |",
    ]
    for row in rows:
        tzim = row.get("tzim") or {}
        qpdf_check = row.get("tzim_qpdf_check") or {}
        original_qpdf = row.get("original_qpdf_check") or {}
        if qpdf_check.get("returncode") is not None:
            qpdf_status = f"exit {qpdf_check['returncode']}"
        elif qpdf_check.get("not_run"):
            qpdf_status = f"unchanged; source exit {original_qpdf.get('returncode')}"
        else:
            qpdf_status = "not run"
        tzim_render = next(
            (comparison for comparison in row.get("render_comparisons", []) if comparison.get("candidate") == "tzim"),
            None,
        )
        if tzim_render is not None:
            render_status = "identical" if tzim_render.get("identical") else "DIFFERENT"
        elif row.get("render_error"):
            render_status = f"error: {row['render_error']}"
        elif tzim.get("status") and tzim.get("sha256") == row.get("sha256"):
            render_status = "unchanged; no candidate render"
        else:
            render_status = "not run"
        gain_pct = tzim.get("gain_pct")
        gain_status = f"{gain_pct:.6f}%" if isinstance(gain_pct, (int, float)) else "—"
        markdown.append(
            f"| `{row.get('id', 'unknown')}` | {row.get('bytes', '—')} / `{row.get('sha256', '—')}` "
            f"| {row.get('classification', 'missing')} | {tzim.get('status', 'not-run')}; "
            f"{tzim.get('bytes', '—')} / `{tzim.get('sha256', '—')}` | {gain_status} | "
            f"{qpdf_status} | {render_status} |"
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
        tzim = row.get("tzim") or {}
        if tzim.get("status") != "failed" and not tzim.get("error"):
            continue
        diagnostics.extend([
            f"### Tzim handler failure: {row.get('id', row.get('path', 'unknown-input'))}",
            f"- Fixed source path: `{row.get('path')}`",
            f"- Error: {diagnostic_excerpt(tzim.get('error'))}",
            f"- Return code: `{tzim.get('returncode')}`",
            f"- Command: `{json.dumps(tzim.get('argv', []), ensure_ascii=False)}`",
        ])
        for label in ("stderr", "stdout"):
            text = diagnostic_excerpt(tzim.get(label))
            if text != "(empty)":
                diagnostics.extend([f"- {label}:", "", "```text", text, "```"])
        diagnostics.append("")
    qpdf_diagnostics = []
    for row in rows:
        check = row.get("original_qpdf_check") or {}
        if check.get("returncode") in (None, 0):
            continue
        qpdf_diagnostics.extend([
            f"### qpdf original check: {row.get('id', row.get('path', 'unknown-input'))}",
            f"- Fixed source path: `{row.get('path')}`",
            f"- Return code: `{check.get('returncode')}`",
        ])
        for label in ("stdout", "stderr"):
            text = diagnostic_excerpt(check.get(label))
            if text != "(empty)":
                qpdf_diagnostics.extend([f"- {label}:", "", "```text", text, "```"])
        xref = row.get("qpdf_xref_diagnostic") or {}
        xref_text = diagnostic_excerpt(xref.get("stdout"), limit=1800)
        if xref_text != "(empty)":
            qpdf_diagnostics.extend(["- qpdf `--show-xref`:", "", "```text", xref_text, "```"])
        for object_id, object_output in (row.get("qpdf_zero_offset_objects") or {}).items():
            object_text = diagnostic_excerpt(object_output.get("stdout"), limit=1200)
            object_error = diagnostic_excerpt(object_output.get("stderr"), limit=500)
            qpdf_diagnostics.extend([
                f"- qpdf `--show-object={object_id}`:", "", "```text",
                object_text if object_error == "(empty)" else f"{object_text}\n{object_error}",
                "```",
            ])
        pymupdf_xrefs = row.get("pymupdf_zero_offset_reference_diagnostic")
        if pymupdf_xrefs:
            xref_text = diagnostic_excerpt(json.dumps(pymupdf_xrefs, indent=2, ensure_ascii=False), limit=5000)
            qpdf_diagnostics.extend(["- PyMuPDF references to zero-offset objects:", "", "```json", xref_text, "```"])
        qpdf_diagnostics.append("")

    if diagnostics:
        markdown.extend(["", "## Per-input handler diagnostics", "", *diagnostics])
    if qpdf_diagnostics:
        markdown.extend(["", "## Independent qpdf diagnostics", "", *qpdf_diagnostics])
    (out / "summary.md").write_text("\n".join(markdown) + "\n", encoding="utf-8")

    failures = []
    for row in rows:
        if row.get("generation_status") != "generated":
            failures.append(f"{row.get('path')}: generation failed ({row.get('reason')})")
        if row.get("classification") == "measurement_dependency_failed":
            failures.append(f"{row.get('path')}: measurement dependency unavailable ({row.get('reason')})")
        if row.get("classification") in ("integrity_failed", "tzim_failed"):
            failures.append(f"{row.get('path')}: integrity/handler status {row.get('classification')}")
        if row.get("faithful_error"):
            failures.append(f"{row.get('path')}: faithful measurement failed ({row['faithful_error']})")
        if row.get("render_error"):
            failures.append(f"{row.get('path')}: render failed ({row['render_error']})")
        if any(not comparison.get("identical", True) for comparison in row.get("render_comparisons", [])):
            failures.append(f"{row.get('path')}: first/middle/last render differed")
        if row.get("original_qpdf_check", {}).get("returncode") not in (None, 0):
            failures.append(f"{row.get('path')}: qpdf check of the original PDF failed")
        if row.get("qpdf_allowed_check", {}).get("returncode") not in (None, 0):
            failures.append(f"{row.get('path')}: qpdf allowed candidate check failed")
        if row.get("qpdf_repack_check", {}).get("returncode") not in (None, 0):
            failures.append(f"{row.get('path')}: qpdf forbidden repack check failed")
        if row.get("tzim_qpdf_check", {}).get("returncode") not in (None, 0):
            failures.append(f"{row.get('path')}: qpdf check of the actual Tzim output failed")
        if row.get("tzim", {}).get("status") == "failed" or row.get("tzim", {}).get("error"):
            failures.append(f"{row.get('path')}: tzim failed ({row.get('tzim', {}).get('error')})")
    if aggregate_result["render_differences"]:
        failures.append("aggregate render comparison has non-zero differences")
    if aggregate_result["pre_registered_threshold_disposition"]["status"] == "not_applied":
        failures.append(
            "pre-registered #57 threshold remains unapplied: "
            + aggregate_result["pre_registered_threshold_disposition"]["decision"]
        )
    if failures:
        print("\n".join(f"BLOCKER: {message}" for message in failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
