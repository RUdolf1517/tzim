#!/usr/bin/env python3
"""Merge both PDF generators and freeze all 21 owner-registered inputs."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

SOURCE_SLUGS = (
    "readme",
    "changelog",
    "guarantees",
    "formats",
    "roadmap",
    "measurement-status",
)
SCAN_SLUGS = ("c03-29", "linn", "typewriter")
EXPECTED_IDS = {
    f"{category}/{slug}"
    for category in ("latex", "libreoffice", "quartz")
    for slug in SOURCE_SLUGS
} | {f"scans/{slug}" for slug in SCAN_SLUGS}

PRODUCERS = {
    "latex": "Pandoc + pdflatex",
    "libreoffice": "Pandoc Markdown-to-DOCX + headless LibreOffice",
    "quartz": "AppKit/Quartz native PDFContext",
    "scans": "pinned source-image conversion or lossless embedded-image extraction",
}


def sha256(path: Path) -> tuple[int, str]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            size += len(block)
            digest.update(block)
    return size, digest.hexdigest()


def expected_ids(kind: str) -> tuple[str, ...]:
    if kind == "ubuntu":
        return tuple(
            [f"{category}/{slug}" for category in ("latex", "libreoffice") for slug in SOURCE_SLUGS]
            + [f"scans/{slug}" for slug in SCAN_SLUGS]
        )
    if kind == "quartz":
        return tuple(f"quartz/{slug}" for slug in SOURCE_SLUGS)
    raise ValueError(f"unexpected PDF generation kind: {kind}")


def expected_path(identifier: str) -> str:
    return f"{identifier}.pdf"


def failure_row(identifier: str, error: str) -> dict:
    category = identifier.split("/", 1)[0]
    return {
        "id": identifier,
        "path": expected_path(identifier),
        "producer": PRODUCERS.get(category, "unknown fixed PDF producer"),
        "status": "failed",
        "error": error,
    }


def read_generation(
    path: Path,
    expected_kind: str,
    artifact_name: str,
    artifact_root: Path,
    expected_source_sha: str,
) -> tuple[dict, dict]:
    """Read one generator result, replacing a missing/malformed result with fixed failure rows."""
    raw: bytes | None = None
    error: str | None = None
    try:
        raw = path.read_bytes()
        document = json.loads(raw)
    except Exception as exc:
        document = None
        error = f"{type(exc).__name__}: {exc}"

    manifest_sha = hashlib.sha256(raw).hexdigest() if raw is not None else None
    valid_document = (
        isinstance(document, dict)
        and document.get("schema") == "tzim-fixed-pdf-generation-v1"
        and document.get("kind") == expected_kind
        and isinstance(document.get("outputs"), list)
    )
    if not valid_document:
        if error is None:
            error = (
                "generation manifest has the wrong schema, kind, or outputs type: "
                f"{path} (expected kind {expected_kind!r})"
            )
        manifest_status = "invalid" if raw is not None else "missing"
        document = {}
        generated_rows: list[object] = []
    else:
        manifest_status = "available"
        generated_rows = document["outputs"]

    observed_source_sha = document.get("repository_source_sha") if valid_document else None
    source_identity_valid = observed_source_sha == expected_source_sha if valid_document else False
    if valid_document and not source_identity_valid:
        source_error = (
            "generation repository source SHA does not match the fixed measurement SHA: "
            f"observed {observed_source_sha!r}, expected {expected_source_sha!r}"
        )
        error = f"{error}; {source_error}" if error else source_error

    rows_by_id: dict[str, list[dict]] = {}
    anomalies: list[str] = []
    allowed = set(expected_ids(expected_kind))
    for index, row in enumerate(generated_rows):
        if not isinstance(row, dict):
            anomalies.append(f"output row {index} is not an object")
            continue
        identifier = row.get("id")
        if identifier not in allowed:
            anomalies.append(f"unexpected output id at row {index}: {identifier!r}")
            continue
        rows_by_id.setdefault(identifier, []).append(row)

    output_rows: list[dict] = []
    for identifier in expected_ids(expected_kind):
        candidates = rows_by_id.get(identifier, [])
        if len(candidates) != 1:
            reason = (
                "generation row missing from manifest"
                if not candidates
                else f"duplicate generation rows for fixed ID {identifier!r}"
            )
            row = failure_row(identifier, reason)
        else:
            row = dict(candidates[0])
            declared_path = row.get("path")
            fixed_path = expected_path(identifier)
            if declared_path != fixed_path:
                row["declared_path"] = declared_path
                row["status"] = "failed"
                row["error"] = (
                    f"generation row path does not match the fixed manifest: "
                    f"{declared_path!r}, expected {fixed_path!r}"
                )
            row["id"] = identifier
            row["path"] = fixed_path

        row["generation_repository_source_sha"] = observed_source_sha
        row["generation_source_identity_valid"] = source_identity_valid
        if manifest_sha is not None:
            row["generation_manifest_sha256"] = manifest_sha
        if not valid_document:
            row["status"] = "failed"
            row["error"] = error or "generation manifest unavailable"
        elif not source_identity_valid:
            row["status"] = "failed"
            previous = row.get("error")
            row["error"] = f"{error}; existing generation error: {previous}" if previous else error

        row["artifact_path"] = f"{artifact_name}/outputs/{expected_path(identifier)}"
        if row.get("status") == "generated":
            target = artifact_root / row["artifact_path"]
            if not target.is_file():
                row["status"] = "failed"
                row["error"] = f"artifact file missing: {row['artifact_path']}"
            else:
                size, digest = sha256(target)
                if size != row.get("bytes") or digest != row.get("sha256"):
                    row["status"] = "failed"
                    row["error"] = f"artifact hash/size differs from generation record: {row['artifact_path']}"
        output_rows.append(row)

    summary = {
        "kind": expected_kind,
        "manifest_status": manifest_status,
        "manifest_sha256": manifest_sha,
        "manifest_path": str(path),
        "repository_source_sha": observed_source_sha,
        "expected_repository_source_sha": expected_source_sha,
        "source_identity_valid": source_identity_valid,
        "environment": document.get("environment") if valid_document else None,
        "source_archive": document.get("source_archive") if valid_document else None,
        "error": error,
        "anomalies": anomalies,
    }
    return {"summary": summary, "outputs": output_rows}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact-root", required=True)
    parser.add_argument("--ubuntu-manifest", required=True)
    parser.add_argument("--quartz-manifest", required=True)
    parser.add_argument("--expected-source-sha", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    root = Path(args.artifact_root).resolve()
    ubuntu = read_generation(
        Path(args.ubuntu_manifest).resolve(), "ubuntu", "fixed-pdf-ubuntu", root, args.expected_source_sha
    )
    quartz = read_generation(
        Path(args.quartz_manifest).resolve(), "quartz", "fixed-pdf-quartz", root, args.expected_source_sha
    )
    outputs = sorted(ubuntu["outputs"] + quartz["outputs"], key=lambda row: row["id"])
    ids = [row["id"] for row in outputs]
    if len(ids) != 21 or set(ids) != EXPECTED_IDS or len(set(ids)) != 21:
        raise AssertionError(f"internal fixed-manifest construction error: {len(ids)} rows, IDs={ids}")

    generation_summaries = [ubuntu["summary"], quartz["summary"]]
    manifest = {
        "schema": "tzim-fixed-pdf-frozen-manifest-v1",
        "source_decision_comment": "6498702204",
        "manifest_frozen_before_gain": True,
        "repository_source_sha": args.expected_source_sha,
        "expected_repository_source_sha": args.expected_source_sha,
        "generation_manifests": generation_summaries,
        "generation_anomalies": {
            item["kind"]: item["anomalies"]
            for item in generation_summaries
            if item["anomalies"]
        },
        "output_count": len(outputs),
        "outputs": outputs,
    }
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    output.with_suffix(output.suffix + ".sha256").write_text(f"{digest}  {output.name}\n", encoding="ascii")

    failures = [row for row in outputs if row.get("status") != "generated"]
    failed_manifests = [
        item for item in generation_summaries
        if item["manifest_status"] != "available" or not item["source_identity_valid"] or item["anomalies"]
    ]
    print(f"frozen all {len(outputs)} fixed PDF source rows; manifest sha256={digest}")
    for item in failed_manifests:
        print(
            f"FAILED {item['kind']} generation manifest: "
            f"{item['error'] or item['manifest_status']}; anomalies={item['anomalies']}"
        )
    for row in failures:
        print(f"FAILED {row['path']}: {row.get('error', 'generation failed')}")
    return 1 if failures or failed_manifests else 0


if __name__ == "__main__":
    raise SystemExit(main())
