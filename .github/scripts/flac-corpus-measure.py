#!/usr/bin/env python3
"""Measure the pre-registered CC0 FLAC testbench set with external references.

The expanded input manifest is written before Tzim runs. No source selection or
input substitution depends on a compression result. FLAC bytes and decoded PCM
are temporary; only identities, tool output, raw rows, and aggregates are kept.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import time
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_MANIFEST = ROOT / ".github/data/flac-corpus.json"
MAX_LOG_CHARS = 16_000
METADATA_OPTIONS = [
    ("md5", "--show-md5sum"),
    ("sample_rate", "--show-sample-rate"),
    ("channels", "--show-channels"),
    ("bits_per_sample", "--show-bps"),
    ("total_samples", "--show-total-samples"),
]


def digest_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def digest_file(path: Path) -> tuple[int, str]:
    sha = hashlib.sha256()
    size = 0
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            size += len(chunk)
            sha.update(chunk)
    return size, sha.hexdigest()


def digest_pcm(path: Path) -> dict:
    sha = hashlib.sha256()
    md5 = hashlib.md5(usedforsecurity=False)
    size = 0
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            size += len(chunk)
            sha.update(chunk)
            md5.update(chunk)
    return {"bytes": size, "sha256": sha.hexdigest(), "md5": md5.hexdigest()}


def git_blob_sha1(data: bytes) -> str:
    return hashlib.sha1(b"blob " + str(len(data)).encode("ascii") + b"\0" + data).hexdigest()


def clipped(raw: bytes | None) -> tuple[str, int, str]:
    data = raw or b""
    text = data.decode("utf-8", "replace")
    if len(text) > MAX_LOG_CHARS:
        text = text[:MAX_LOG_CHARS] + "\n...[truncated]"
    return text, len(data), digest_bytes(data)


def record_event(log, event: dict) -> None:
    log.write(json.dumps(event, sort_keys=True, ensure_ascii=False) + "\n")
    log.flush()


def run_logged(
    log,
    name: str,
    argv: list[str],
    *,
    timeout: int = 300,
    stdout_path: Path | None = None,
) -> dict:
    event: dict = {"name": name, "argv": argv}
    try:
        if stdout_path is None:
            result = subprocess.run(
                argv,
                cwd=ROOT,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=timeout,
                check=False,
            )
            stdout_raw = result.stdout
        else:
            stdout_path.parent.mkdir(parents=True, exist_ok=True)
            with stdout_path.open("wb") as output:
                result = subprocess.run(
                    argv,
                    cwd=ROOT,
                    stdout=output,
                    stderr=subprocess.PIPE,
                    timeout=timeout,
                    check=False,
                )
            stdout_raw = b""
            if stdout_path.is_file():
                event["stdout_file"] = str(stdout_path)
                event["stdout_file_bytes"], event["stdout_file_sha256"] = digest_file(stdout_path)
        stdout_text, stdout_bytes, stdout_sha = clipped(stdout_raw)
        stderr_text, stderr_bytes, stderr_sha = clipped(result.stderr)
        event.update(
            return_code=result.returncode,
            stdout=stdout_text,
            stdout_bytes=stdout_bytes,
            stdout_sha256=stdout_sha,
            stderr=stderr_text,
            stderr_bytes=stderr_bytes,
            stderr_sha256=stderr_sha,
        )
        record_event(log, event)
        return event
    except subprocess.TimeoutExpired as error:
        stdout_text, stdout_bytes, stdout_sha = clipped(error.stdout)
        stderr_text, stderr_bytes, stderr_sha = clipped(error.stderr)
        event.update(
            return_code=None,
            timed_out=True,
            timeout_seconds=timeout,
            stdout=stdout_text,
            stdout_bytes=stdout_bytes,
            stdout_sha256=stdout_sha,
            stderr=stderr_text,
            stderr_bytes=stderr_bytes,
            stderr_sha256=stderr_sha,
            error=f"TimeoutExpired after {timeout} seconds",
        )
        record_event(log, event)
        return event
    except OSError as error:
        event.update(return_code=None, error=f"{type(error).__name__}: {error}")
        record_event(log, event)
        return event


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False, sort_keys=True) + "\n", encoding="utf-8")


def read_jsonl(path: Path) -> list[dict]:
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def source_urls(manifest: dict, source: dict) -> tuple[str, str]:
    repo = manifest["corpus"]["repository"]
    commit = manifest["corpus"]["commit"]
    path = source["path"]
    encoded_path = quote(path, safe="/")
    blob_url = f"https://github.com/{repo}/blob/{commit}/{encoded_path}"
    api_url = f"https://api.github.com/repos/{repo}/contents/{encoded_path}?ref={commit}"
    return blob_url, api_url


def fetch_sources(manifest: dict, input_dir: Path, tool_log) -> tuple[list[dict], dict[str, Path], list[dict]]:
    input_dir.mkdir(parents=True, exist_ok=True)
    rows: list[dict] = []
    paths: dict[str, Path] = {}
    blockers: list[dict] = []
    repo = manifest["corpus"]["repository"]
    commit = manifest["corpus"]["commit"]
    for source in manifest["inputs"]:
        blob_url, api_url = source_urls(manifest, source)
        endpoint = f"repos/{repo}/contents/{quote(source['path'], safe='/')}?ref={commit}"
        target = input_dir / f"{source['id']}.flac"
        event = run_logged(
            tool_log,
            f"fetch:{source['id']}",
            ["gh", "api", "--header", "Accept: application/vnd.github.raw", endpoint],
            timeout=180,
            stdout_path=target,
        )
        row = {
            **source,
            "source_url": blob_url,
            "fetch_url": api_url,
            "fetch_status": "fetch_failed",
            "observed_bytes": None,
            "observed_sha256": None,
            "observed_git_blob_sha1": None,
        }
        failure = event.get("return_code") != 0 or event.get("error") or event.get("timed_out")
        if not failure and target.is_file():
            size, sha = digest_file(target)
            raw = target.read_bytes()
            blob_sha = git_blob_sha1(raw)
            row.update(observed_bytes=size, observed_sha256=sha, observed_git_blob_sha1=blob_sha)
            if size == source["size"] and sha == source["sha256"] and blob_sha == source["git_blob_sha1"]:
                row["fetch_status"] = "fetched"
                paths[source["id"]] = target
            else:
                row["fetch_status"] = "identity_mismatch"
                row["error"] = (
                    f"observed {size} bytes / SHA-256 {sha} / Git blob {blob_sha}; "
                    f"expected {source['size']} / {source['sha256']} / {source['git_blob_sha1']}"
                )
        else:
            row["error"] = event.get("error") or event.get("stderr") or f"gh api returned {event.get('return_code')}"
        if row["fetch_status"] != "fetched":
            blocker = {
                "id": source["id"],
                "path": source["path"],
                "url": api_url,
                "error": row.get("error", "source identity mismatch"),
                "owner_action": (
                    "Restore access to this exact pinned CC0 source or confirm its availability; "
                    "do not substitute another FLAC file."
                ),
            }
            row["owner_action"] = blocker["owner_action"]
            blockers.append(blocker)
        rows.append(row)
    return rows, paths, blockers


def tool_version(log, name: str, command: str) -> dict:
    if shutil.which(command) is None:
        event = {"name": f"version:{name}", "argv": [command, "--version"], "return_code": None, "error": f"{command} not found"}
        record_event(log, event)
        return event
    return run_logged(log, f"version:{name}", [command, "--version"], timeout=30)


def generate_derived(
    manifest: dict,
    source_paths: dict[str, Path],
    source_rows: list[dict],
    derived_dir: Path,
    tool_log,
    flac_version: dict,
) -> tuple[list[dict], dict[str, Path], list[dict]]:
    rows: list[dict] = []
    paths: dict[str, Path] = {}
    blockers: list[dict] = []
    source_by_id = {row["id"]: row for row in source_rows}
    derived_dir.mkdir(parents=True, exist_ok=True)

    for parent in manifest["derived_sources"]:
        parent_id = parent["parent_id"]
        parent_path = source_paths.get(parent_id)
        parent_row = source_by_id[parent_id]
        wav_path = derived_dir / f"{parent['id']}.wav"
        generation_events: list[dict] = []
        error: str | None = None
        if parent_path is None:
            error = f"pinned parent source {parent_id} was not fetched and verified"
        elif flac_version.get("return_code") != 0:
            error = "the external flac reference tool is unavailable"
        else:
            decode = run_logged(
                tool_log,
                f"derive-decode:{parent['id']}",
                ["flac", "--decode", "--force", "--silent", "--output-name", str(wav_path), str(parent_path)],
                timeout=600,
            )
            generation_events.append({"name": decode["name"], "argv": decode["argv"], "return_code": decode.get("return_code")})
            if decode.get("return_code") != 0 or not wav_path.is_file():
                error = decode.get("stderr") or decode.get("error") or "flac did not produce the parent WAV"

        for derived in (row for row in manifest["derived_inputs"] if row["parent_id"] == parent_id):
            target = derived_dir / f"{derived['id']}.flac"
            row = {
                **derived,
                "kind": "derived",
                "handler_expectation": "valid-input",
                "parent_url": parent_row.get("source_url"),
                "parent_fetch_url": parent_row.get("fetch_url"),
                "parent_sha256": parent_row.get("observed_sha256"),
                "reference_flac_version": flac_version.get("stdout", ""),
                "generation_commands": list(generation_events),
                "generation_status": "generation_failed",
                "bytes": None,
                "sha256": None,
            }
            if error is None:
                level = derived["flac_compression_level"]
                command = [
                    "flac", "--silent", "--force", f"--compression-level-{level}",
                    "--output-name", str(target), str(wav_path),
                ]
                event = run_logged(tool_log, f"derive:{derived['id']}", command, timeout=600)
                row["generation_commands"].append({
                    "name": event["name"], "argv": event["argv"], "return_code": event.get("return_code")
                })
                if event.get("return_code") == 0 and target.is_file():
                    size, sha = digest_file(target)
                    row.update(generation_status="generated", bytes=size, sha256=sha)
                    paths[derived["id"]] = target
                else:
                    row["error"] = event.get("stderr") or event.get("error") or "flac did not produce the derived input"
            else:
                row["error"] = error
            if row["generation_status"] != "generated":
                blocker = {
                    "id": derived["id"],
                    "path": parent["parent_id"],
                    "url": parent_row.get("fetch_url"),
                    "error": row.get("error", "derived input generation failed"),
                    "owner_action": (
                        "Restore the pinned parent and external flac toolchain; retain this level row and do not "
                        "replace it with a gain-selected input."
                    ),
                }
                row["owner_action"] = blocker["owner_action"]
                blockers.append(blocker)
            rows.append(row)
    return rows, paths, blockers


def parse_md5(value: object) -> str | None:
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
            or (
                source_md5 is not None
                and candidate_md5 is not None
                and candidate_md5 == source_md5
            )
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


def metaflac_summary(log, file_id: str, label: str, path: Path) -> dict:
    argv = ["metaflac"]
    for _, option in METADATA_OPTIONS:
        argv.append(option)
    argv.append(str(path))
    event = run_logged(log, f"metaflac:{file_id}:{label}", argv, timeout=120)
    lines = [line.strip() for line in event.get("stdout", "").splitlines() if line.strip()]
    values: dict[str, str | None] = {}
    if event.get("return_code") == 0 and len(lines) == len(METADATA_OPTIONS):
        values = {field: lines[index] for index, (field, _) in enumerate(METADATA_OPTIONS)}
    elif event.get("return_code") == 0:
        values = {"unparsed_output": event.get("stdout", "")}
    return {
        "return_code": event.get("return_code"),
        "values": values,
        "stdout": event.get("stdout", ""),
        "stderr": event.get("stderr", ""),
    }


def flac_test(log, file_id: str, label: str, path: Path) -> dict:
    event = run_logged(log, f"flac-test:{file_id}:{label}", ["flac", "--test", "--silent", str(path)], timeout=600)
    return {
        "return_code": event.get("return_code"),
        "stdout": event.get("stdout", ""),
        "stderr": event.get("stderr", ""),
        "error": event.get("error"),
    }


def read_metadata_blocks(path: Path) -> list[tuple[int, bytes]]:
    data = path.read_bytes()
    if not data.startswith(b"fLaC"):
        raise ValueError("missing fLaC marker")
    position = 4
    blocks: list[tuple[int, bytes]] = []
    for _ in range(4096):
        if position + 4 > len(data):
            raise ValueError("truncated metadata block header")
        first = data[position]
        last = bool(first & 0x80)
        kind = first & 0x7F
        length = int.from_bytes(data[position + 1 : position + 4], "big")
        start = position + 4
        end = start + length
        if end > len(data):
            raise ValueError(f"metadata block {kind} extends beyond the file")
        blocks.append((kind, data[start:end]))
        position = end
        if last:
            return blocks
    raise ValueError("more than 4096 metadata blocks")


def compare_metadata(original: Path, candidate: Path) -> dict:
    before = read_metadata_blocks(original)
    after = read_metadata_blocks(candidate)
    kinds_before = [kind for kind, _ in before]
    kinds_after = [kind for kind, _ in after]
    if kinds_before != kinds_after:
        raise ValueError(f"metadata block order changed: {kinds_before} -> {kinds_after}")
    changed_preserved: list[int] = []
    for index, ((kind_before, payload_before), (kind_after, payload_after)) in enumerate(zip(before, after, strict=True)):
        if kind_before != kind_after:
            raise ValueError(f"metadata block {index} type changed")
        if kind_before not in (0, 3) and payload_before != payload_after:
            changed_preserved.append(index)
    if changed_preserved:
        raise ValueError(f"non-STREAMINFO/SEEKTABLE metadata changed at block indexes {changed_preserved}")
    return {"block_types": kinds_before, "block_count": len(kinds_before), "preserved_other_blocks": True}


def decode_raw_pcm(log, file_id: str, label: str, source: Path, target: Path) -> dict:
    argv = [
        "flac", "--decode", "--force", "--silent", "--stdout", "--force-raw-format",
        "--endian=little", "--sign=signed", str(source),
    ]
    event = run_logged(log, f"pcm-decode:{file_id}:{label}", argv, timeout=1200, stdout_path=target)
    if event.get("return_code") != 0 or not target.is_file():
        return {"return_code": event.get("return_code"), "error": event.get("stderr") or event.get("error")}
    return {"return_code": 0, **digest_pcm(target)}


def run_tzim(log, tzim: Path, file_id: str, label: str, path: Path) -> tuple[dict, dict | None, float]:
    argv = [
        str(tzim), "--formats", "flac", "--jobs", "1", "--json", "--no-progress", str(path)
    ]
    start = time.perf_counter()
    event = run_logged(log, f"tzim:{file_id}:{label}", argv, timeout=1800)
    elapsed_ms = (time.perf_counter() - start) * 1000
    file_rows = []
    for line in event.get("stdout", "").splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if value.get("type") == "file":
            file_rows.append(value)
    file_row = file_rows[0] if len(file_rows) == 1 else None
    return event, file_row, elapsed_ms


def measure_one(
    log,
    entry: dict,
    source_path: Path,
    tzim: Path,
    work_root: Path,
) -> dict:
    file_id = entry["id"]
    work_dir = work_root / "run" / file_id
    work_dir.mkdir(parents=True, exist_ok=True)
    working_file = work_dir / f"{file_id}.flac"
    shutil.copyfile(source_path, working_file)
    before_bytes, before_sha = digest_file(working_file)
    expected = entry["handler_expectation"]

    source_test = flac_test(log, file_id, "source", source_path)
    source_meta = metaflac_summary(log, file_id, "source", source_path)
    blockers: list[dict] = []
    expected_input_sha = entry.get("observed_sha256", entry.get("sha256"))
    if expected_input_sha is not None and before_sha != expected_input_sha:
        blockers.append({"code": "measurement_copy_differs_from_frozen_input", "expected": expected_input_sha, "observed": before_sha})
    if expected == "valid-input" and source_test.get("return_code") != 0:
        blockers.append({"code": "reference_flac_rejected_rewritable_source", "detail": source_test})
    if expected == "valid-input" and source_meta.get("return_code") != 0:
        blockers.append({"code": "metaflac_rejected_rewritable_source", "detail": source_meta})

    event, file_row, elapsed_ms = run_tzim(log, tzim, file_id, "first-pass", working_file)
    after_bytes, after_sha = digest_file(working_file)
    status = file_row.get("status") if file_row else "no_file_row"
    if expected == "valid-input" and status not in {"optimized", "already_optimal"}:
        blockers.append({
            "code": "valid_input_expectation_not_met",
            "expected": "valid-input",
            "accepted_statuses": ["optimized", "already_optimal"],
            "observed_status": status,
        })
    if file_row is None:
        blockers.append({"code": "tzim_missing_file_row", "return_code": event.get("return_code")})
    if expected != "invalid-control" and status == "failed":
        blockers.append({"code": "unexpected_tzim_failure_on_reference_valid_input", "error": file_row.get("error") if file_row else event.get("stderr")})
    if expected != "invalid-control" and event.get("return_code") != 0:
        blockers.append({"code": "tzim_nonzero_exit_for_noncorrupt_input", "return_code": event.get("return_code")})
    if expected == "invalid-control" and status not in ("failed", "skipped"):
        blockers.append({"code": "corrupt_control_not_reported_as_failure_or_refusal", "status": status})
    if status == "optimized":
        if after_bytes >= before_bytes:
            blockers.append({"code": "optimized_result_not_strictly_smaller", "before": before_bytes, "after": after_bytes})
        if after_sha == before_sha:
            blockers.append({"code": "optimized_status_but_bytes_identical"})
    elif after_bytes != before_bytes or after_sha != before_sha:
        blockers.append({"code": "original_changed_without_optimized_status", "before_sha256": before_sha, "after_sha256": after_sha})
    if expected == "invalid-control" and status == "optimized":
        blockers.append({"code": "invalid_control_was_optimized"})

    candidate_checks: dict | None = None
    second_pass: dict | None = None
    if status == "optimized":
        candidate_test = flac_test(log, file_id, "candidate", working_file)
        candidate_meta = metaflac_summary(log, file_id, "candidate", working_file)
        candidate_checks = {
            "flac_test": candidate_test,
            "metaflac": candidate_meta,
        }
        if candidate_test.get("return_code") != 0:
            blockers.append({"code": "external_flac_test_rejected_candidate", "detail": candidate_test})
        if candidate_meta.get("return_code") != 0:
            blockers.append({"code": "external_metaflac_rejected_candidate", "detail": candidate_meta})
        source_values = source_meta.get("values") or {}
        candidate_values = candidate_meta.get("values") or {}
        identity_fields = ("sample_rate", "channels", "bits_per_sample", "total_samples")
        if not all(field in source_values and field in candidate_values for field in identity_fields):
            blockers.append({
                "code": "metaflac_streaminfo_identity_unparsed",
                "source": source_values,
                "candidate": candidate_values,
            })
        elif any(source_values[field] != candidate_values[field] for field in identity_fields):
            blockers.append({
                "code": "STREAMINFO_sample_identity_differs_in_metaflac",
                "source": {field: source_values[field] for field in identity_fields},
                "candidate": {field: candidate_values[field] for field in identity_fields},
            })
        source_streaminfo_md5 = parse_md5(source_values.get("md5"))
        candidate_streaminfo_md5 = parse_md5(candidate_values.get("md5"))
        if source_streaminfo_md5 is None or candidate_streaminfo_md5 is None:
            blockers.append({
                "code": "metaflac_streaminfo_md5_unparsed",
                "source": source_values.get("md5"),
                "candidate": candidate_values.get("md5"),
            })
        try:
            candidate_checks["metadata_structure"] = compare_metadata(source_path, working_file)
        except (OSError, ValueError) as error:
            blockers.append({"code": "metadata_preservation_mismatch", "detail": str(error)})

        original_pcm = work_dir / "source.pcm"
        candidate_pcm = work_dir / "candidate.pcm"
        original_pcm_result = decode_raw_pcm(log, file_id, "source", source_path, original_pcm)
        candidate_pcm_result = decode_raw_pcm(log, file_id, "candidate", working_file, candidate_pcm)
        candidate_checks["external_pcm"] = {
            "source": original_pcm_result,
            "candidate": candidate_pcm_result,
        }
        if original_pcm_result.get("return_code") != 0 or candidate_pcm_result.get("return_code") != 0:
            blockers.append({"code": "external_pcm_decode_failed", "detail": candidate_checks["external_pcm"]})
        elif original_pcm_result.get("sha256") != candidate_pcm_result.get("sha256") or original_pcm_result.get("bytes") != candidate_pcm_result.get("bytes"):
            blockers.append({"code": "external_pcm_mismatch", "detail": candidate_checks["external_pcm"]})

        external_pcm_md5 = original_pcm_result.get("md5") if original_pcm_result.get("return_code") == 0 else None
        md5_check = streaminfo_md5_check(
            source_streaminfo_md5, candidate_streaminfo_md5, external_pcm_md5
        )
        candidate_checks["streaminfo_md5"] = md5_check
        if not md5_check["ok"]:
            blockers.append({"code": "STREAMINFO_MD5_does_not_match_independent_PCM", "detail": md5_check})

        second_copy = work_dir / "second-pass.flac"
        shutil.copyfile(working_file, second_copy)
        second_event, second_row, second_ms = run_tzim(log, tzim, file_id, "second-pass", second_copy)
        second_bytes, second_sha = digest_file(second_copy)
        second_status = second_row.get("status") if second_row else "no_file_row"
        second_pass = {
            "return_code": second_event.get("return_code"),
            "status": second_status,
            "duration_ms": round(second_ms, 3),
            "bytes": second_bytes,
            "sha256": second_sha,
        }
        if second_row is None or second_event.get("return_code") != 0 or second_status == "optimized" or second_sha != after_sha or second_bytes != after_bytes:
            blockers.append({"code": "second_pass_not_stable", "detail": second_pass})

    return {
        "id": file_id,
        "kind": entry["kind"],
        "handler_expectation": expected,
        "source_path": entry.get("path"),
        "source_url": entry.get("source_url", entry.get("parent_url")),
        "source_sha256": entry.get("sha256"),
        "before_bytes": before_bytes,
        "before_sha256": before_sha,
        "status": status,
        "tzim_return_code": event.get("return_code"),
        "tzim_file_row": file_row,
        "tzim_duration_ms": round(elapsed_ms, 3),
        "after_bytes": after_bytes,
        "after_sha256": after_sha,
        "saved_bytes": max(0, before_bytes - after_bytes),
        "preservation_check_passed": (
            status == "optimized" and after_bytes < before_bytes and after_sha != before_sha
        ) or (
            status != "optimized" and before_bytes == after_bytes and before_sha == after_sha
        ),
        "reference_flac_test": source_test,
        "reference_metaflac": source_meta,
        "candidate_checks": candidate_checks,
        "second_pass": second_pass,
        "blockers": blockers,
    }


def valid_input_expectation_failures(rows: list[dict], expanded: dict) -> list[dict]:
    registered = {
        row.get("id"): row
        for row in expanded.get("inputs", []) + expanded.get("derived_inputs", [])
    }
    failures = []
    for row in rows:
        entry = registered.get(row.get("id"))
        if entry is None or entry.get("handler_expectation") != "valid-input":
            continue
        status = row.get("status", "unknown")
        if status not in {"optimized", "already_optimal"}:
            failures.append({
                "id": row.get("id"),
                "code": "valid_input_expectation_not_met",
                "expected": "valid-input",
                "accepted_statuses": ["optimized", "already_optimal"],
                "observed_status": status,
            })
    return failures


def aggregate_rows(rows: list[dict], expanded: dict) -> dict:
    counts = Counter(row.get("status", "unknown") for row in rows)
    timed = [float(row["tzim_duration_ms"]) for row in rows if row.get("tzim_duration_ms") is not None]
    before = sum(int(row.get("before_bytes") or 0) for row in rows)
    after = sum(int(row.get("after_bytes") or row.get("before_bytes") or 0) for row in rows)
    optimized = [row for row in rows if row.get("status") == "optimized"]
    candidate_checks = [row.get("candidate_checks") or {} for row in rows]
    positive = [100.0 * row["saved_bytes"] / row["before_bytes"] for row in optimized if row.get("before_bytes")]
    all_blockers = [
        {"id": row.get("id"), **blocker}
        for row in rows
        for blocker in row.get("blockers", [])
    ]
    expectation_failures = valid_input_expectation_failures(rows, expanded)
    blocker_keys = {(blocker.get("id"), blocker.get("code")) for blocker in all_blockers}
    all_blockers.extend(
        failure for failure in expectation_failures
        if (failure.get("id"), failure.get("code")) not in blocker_keys
    )
    return {
        "schema": 1,
        "expected_rows": len(expanded.get("inputs", [])) + len(expanded.get("derived_inputs", [])),
        "rows": len(rows),
        "status_counts": dict(sorted(counts.items())),
        "handler_expectation_counts": dict(
            sorted(Counter(row.get("handler_expectation", "unknown") for row in rows).items())
        ),
        "source_fetch_statuses": dict(sorted(Counter(row.get("fetch_status", "derived") for row in expanded.get("inputs", [])).items())),
        "derived_generation_statuses": dict(sorted(Counter(row.get("generation_status", "unknown") for row in expanded.get("derived_inputs", [])).items())),
        "input_bytes": before,
        "after_bytes": after,
        "saved_bytes": before - after,
        "optimized_rows": len(optimized),
        "optimized_median_gain_pct": round(statistics.median(positive), 6) if positive else None,
        "optimized_mean_gain_pct": round(statistics.fmean(positive), 6) if positive else None,
        "tzim_total_seconds": round(sum(timed) / 1000, 3),
        "tzim_median_ms": round(statistics.median(timed), 3) if timed else None,
        "external_flac_source_passes": sum(row.get("reference_flac_test", {}).get("return_code") == 0 for row in rows),
        "external_flac_candidate_passes": sum((checks.get("flac_test") or {}).get("return_code") == 0 for checks in candidate_checks),
        "external_metaflac_source_passes": sum(row.get("reference_metaflac", {}).get("return_code") == 0 for row in rows),
        "external_metaflac_candidate_passes": sum((checks.get("metaflac") or {}).get("return_code") == 0 for checks in candidate_checks),
        "external_pcm_pairs_equal": sum(
            bool(((checks.get("external_pcm") or {}).get("source") or {}).get("sha256"))
            and ((checks.get("external_pcm") or {}).get("source") or {}).get("sha256")
            == ((checks.get("external_pcm") or {}).get("candidate") or {}).get("sha256")
            for checks in candidate_checks
        ),
        "external_streaminfo_md5_checks_passed": sum(
            (checks.get("streaminfo_md5") or {}).get("ok") is True
            for checks in candidate_checks
        ),
        "external_streaminfo_md5_checks_run": sum(
            "streaminfo_md5" in checks
            for checks in candidate_checks
        ),
        "valid_input_expectation_failures": expectation_failures,
        "blockers": all_blockers,
    }


def write_summary(
    path: Path,
    manifest: dict,
    expanded: dict,
    rows: list[dict],
    aggregate: dict,
    tool_versions: dict,
) -> None:
    gain_line = (
        f"- Gain among optimized rows: median {aggregate['optimized_median_gain_pct']}%, mean {aggregate['optimized_mean_gain_pct']}% (no input selection was made by these results)."
        if aggregate["optimized_rows"]
        else "- Gain among optimized rows: not applicable (zero optimized rows)."
    )
    lines = [
        "# Fixed FLAC corpus measurement",
        "",
        f"- Source manifest: `{manifest['corpus']['repository']}` at `{manifest['corpus']['commit']}` (CC0-1.0).",
        f"- Static FLAC identities: {len(manifest['inputs'])}; level-derived rows: {len(manifest['derived_inputs'])}; total rows: {aggregate['rows']}.",
        f"- Static source bytes: {sum(row['size'] for row in manifest['inputs']):,}; exact per-file Git blob SHA-1 and SHA-256 are in `expanded-manifest.json`.",
        f"- Expanded manifest SHA-256, frozen before Tzim measurement: `{expanded['expanded_manifest_sha256']}`.",
        f"- `flac --version`: `{tool_versions['flac'].get('stdout', '').strip() or tool_versions['flac'].get('error', 'unavailable')}`.",
        f"- `metaflac --version`: `{tool_versions['metaflac'].get('stdout', '').strip() or tool_versions['metaflac'].get('error', 'unavailable')}`.",
        "- The 0/5/8 reference-level variants are generated from the pre-registered 16-bit and 24-bit sources before any candidate gain is computed; every generated byte count and hash is frozen in the expanded manifest.",
        "- No corpus bytes or decoded PCM are included in the evidence bundle.",
        "",
        "## Results",
        "",
        f"- Handler expectations: `{json.dumps(aggregate['handler_expectation_counts'], sort_keys=True)}`.",
        f"- Tzim statuses: `{json.dumps(aggregate['status_counts'], sort_keys=True)}`.",
        f"- Tzim optimized: {aggregate['optimized_rows']} rows; total bytes {aggregate['input_bytes']:,} → {aggregate['after_bytes']:,}; saved {aggregate['saved_bytes']:,} bytes.",
        gain_line,
        f"- Tzim first-pass time: total {aggregate['tzim_total_seconds']}s; median {aggregate['tzim_median_ms']}ms/file.",
        f"- External `flac --test` source passes: {aggregate['external_flac_source_passes']}/{aggregate['rows']}; candidate passes: {aggregate['external_flac_candidate_passes']}.",
        f"- External `metaflac` source reads: {aggregate['external_metaflac_source_passes']}/{aggregate['rows']}; candidate reads: {aggregate['external_metaflac_candidate_passes']}.",
        f"- Independently decoded PCM pairs byte-identical: {aggregate['external_pcm_pairs_equal']}.",
        f"- Independent PCM / STREAMINFO MD5 checks: {aggregate['external_streaminfo_md5_checks_passed']}/{aggregate['external_streaminfo_md5_checks_run']} optimized rows passed (zero input MD5 sentinels are checked against candidate PCM, not treated as recorded digests).",
        "- `raw-rows.jsonl`, `tool-output.jsonl`, the frozen expanded manifest and independently recomputed aggregate retain all rows and commands.",
        "",
        "## Registered failures and measurement blockers",
    ]
    fetch_failures = [row for row in expanded.get("inputs", []) if row.get("fetch_status") != "fetched"]
    generation_failures = [row for row in expanded.get("derived_inputs", []) if row.get("generation_status") != "generated"]
    if not fetch_failures and not generation_failures and not aggregate["blockers"]:
        lines.append("No registered fetch, generation or measurement blocker was reported.")
    for row in fetch_failures:
        lines.append(f"- **Fetch {row['id']}** — `{row['fetch_url']}` — {row.get('error', 'unknown fetch error')}. Owner action: {row.get('owner_action', 'restore exact pinned source; do not substitute')}.")
    for row in generation_failures:
        lines.append(f"- **Generate {row['id']}** — parent `{row['parent_id']}` (`{row.get('parent_fetch_url', 'URL unavailable')}`) — {row.get('error', 'unknown generation error')}. Owner action: {row.get('owner_action', 'restore the pinned parent and toolchain')}.")
    for blocker in aggregate["blockers"]:
        lines.append(f"- **{blocker['id']} / {blocker.get('code', 'blocker')}** — {json.dumps(blocker, sort_keys=True, ensure_ascii=False)}")

    lines.extend([
        "",
        "## Per-row fixed input identities and verification disposition",
        "",
        "All 43 pre-registered rows are shown, including refusals, skips and unavailable inputs. No row is selected or removed based on gain.",
        "",
        "| Fixed ID | Expectation | Input bytes / SHA-256 | Tzim status; output bytes / SHA-256 | Saved bytes | Tzim note | External source / candidate | PCM and STREAMINFO MD5 | Second pass |",
        "| --- | --- | --- | --- | ---: | --- | --- | --- | --- |",
    ])
    expanded_by_id = {
        row.get("id"): row
        for row in expanded.get("inputs", []) + expanded.get("derived_inputs", [])
    }

    def compact(value: object, maximum: int = 120) -> str:
        text = str(value or "—").replace("|", "\\|").replace("\r", " ").replace("\n", " ").strip()
        return text if len(text) <= maximum else text[: maximum - 1] + "…"

    def check_status(value: dict | None) -> str:
        if not value:
            return "not run"
        code = value.get("return_code")
        return "pass" if code == 0 else f"exit {code}" if code is not None else "unavailable"

    for row in rows:
        entry = expanded_by_id.get(row.get("id"), {})
        file_row = row.get("tzim_file_row") or {}
        checks = row.get("candidate_checks") or {}
        source_flac = check_status(row.get("reference_flac_test"))
        source_meta = check_status(row.get("reference_metaflac"))
        candidate_flac = check_status(checks.get("flac_test"))
        candidate_meta = check_status(checks.get("metaflac"))
        if candidate_flac == "not run" and candidate_meta == "not run":
            external_status = f"source flac {source_flac} / metaflac {source_meta}; candidate not run"
        else:
            external_status = (
                f"source flac {source_flac} / metaflac {source_meta}; "
                f"candidate flac {candidate_flac} / metaflac {candidate_meta}"
            )
        md5 = checks.get("streaminfo_md5") or {}
        if md5:
            md5_status = "PASS" if md5.get("ok") is True else "FAIL"
            if md5.get("input_zero_md5_sentinel"):
                md5_detail = f"{md5_status}; zero input sentinel; candidate/PCM {md5.get('candidate_streaminfo_md5')}"
            else:
                md5_detail = (
                    f"{md5_status}; source {md5.get('source_streaminfo_md5')}; "
                    f"candidate {md5.get('candidate_streaminfo_md5')}; PCM {md5.get('external_pcm_md5')}"
                )
        else:
            md5_detail = "not run (no optimized candidate)"
        second = row.get("second_pass") or {}
        second_status = (
            f"{second.get('status', 'unknown')}; {second.get('bytes', '—')} / {compact(second.get('sha256'), 16)}"
            if second
            else "not run"
        )
        source_size = row.get("before_bytes")
        source_sha = row.get("before_sha256") or entry.get("observed_sha256") or entry.get("sha256")
        after_size = row.get("after_bytes")
        after_sha = row.get("after_sha256")
        note = file_row.get("note") or file_row.get("error") or row.get("error") or "—"
        lines.append(
            f"| `{compact(row.get('id'), 80)}` | {compact(row.get('handler_expectation', entry.get('handler_expectation')))} "
            f"| {source_size if source_size is not None else '—'} / `{compact(source_sha, 64)}` "
            f"| {compact(row.get('status'))}; {after_size if after_size is not None else '—'} / `{compact(after_sha, 64)}` "
            f"| {row.get('saved_bytes', 0)} | {compact(note, 300)} | {compact(external_status)} | {compact(md5_detail)} "
            f"| {compact(second_status, 80)} |"
        )

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--tzim", type=Path, required=True)
    parser.add_argument("--work-root", type=Path, required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    work_root = args.work_root.resolve()
    evidence_dir = args.evidence_dir.resolve()
    input_dir = work_root / "inputs"
    derived_dir = work_root / "derived"
    tool_log_path = evidence_dir / "tool-output.jsonl"
    evidence_dir.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(args.manifest, evidence_dir / "source-manifest.json")
    static_manifest_bytes, static_manifest_sha = digest_file(args.manifest)

    expanded: dict = {
        "schema": 1,
        "corpus": manifest["corpus"],
        "source_manifest_path": ".github/data/flac-corpus.json",
        "source_manifest_bytes": static_manifest_bytes,
        "source_manifest_sha256": static_manifest_sha,
        "run_started_utc": datetime.now(timezone.utc).isoformat(),
        "runner": {"platform": platform.platform(), "python": sys.version},
        "frozen_before_tzim_measurement": True,
        "inputs": [],
        "derived_inputs": [],
    }
    fetch_blockers: list[dict] = []
    generation_blockers: list[dict] = []
    rows: list[dict] = []
    with tool_log_path.open("w", encoding="utf-8") as tool_log:
        tool_versions = {
            "flac": tool_version(tool_log, "flac", "flac"),
            "metaflac": tool_version(tool_log, "metaflac", "metaflac"),
            "tzim": run_logged(tool_log, "version:tzim", [str(args.tzim.resolve()), "--version"], timeout=30),
            "gh": tool_version(tool_log, "gh", "gh"),
        }
        static_rows, source_paths, fetch_blockers = fetch_sources(manifest, input_dir, tool_log)
        derived_rows, derived_paths, generation_blockers = generate_derived(
            manifest, source_paths, static_rows, derived_dir, tool_log, tool_versions["flac"]
        )
        expanded["tool_versions"] = tool_versions
        expanded["inputs"] = static_rows
        expanded["derived_inputs"] = derived_rows
        expanded["expanded_manifest_frozen_utc"] = datetime.now(timezone.utc).isoformat()
        frozen_path = evidence_dir / "expanded-manifest.json"
        canonical = json.dumps(expanded, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        expanded["expanded_manifest_sha256"] = digest_bytes(canonical)
        write_json(frozen_path, expanded)

        expanded_by_id = {row["id"]: row for row in static_rows + derived_rows}
        for entry in static_rows + derived_rows:
            file_id = entry["id"]
            source_path = source_paths.get(file_id) or derived_paths.get(file_id)
            if source_path is None or entry.get("fetch_status") not in (None, "fetched") or entry.get("generation_status") not in (None, "generated"):
                row = {
                    "id": file_id,
                    "kind": entry.get("kind"),
                    "handler_expectation": entry.get("handler_expectation"),
                    "source_path": entry.get("path"),
                    "source_url": entry.get("fetch_url", entry.get("parent_fetch_url", entry.get("parent_url"))),
                    "before_bytes": None,
                    "before_sha256": None,
                    "status": "input_unavailable",
                    "tzim_return_code": None,
                    "tzim_file_row": None,
                    "tzim_duration_ms": None,
                    "after_bytes": None,
                    "after_sha256": None,
                    "saved_bytes": 0,
                    "blockers": [],
                }
                if entry.get("fetch_status") not in (None, "fetched"):
                    row["blockers"].append({"code": "pinned_source_not_fetched", "error": entry.get("error")})
                if entry.get("generation_status") not in (None, "generated"):
                    row["blockers"].append({"code": "derived_input_generation_failed", "error": entry.get("error")})
                rows.append(row)
                continue
            entry_with_url = dict(entry)
            if entry.get("kind") == "derived":
                parent = expanded_by_id[entry["parent_id"]]
                entry_with_url["parent_url"] = parent.get("source_url", parent.get("fetch_url"))
                entry_with_url["sha256"] = entry.get("sha256")
            result = measure_one(tool_log, entry_with_url, source_path, args.tzim.resolve(), work_root)
            result["source_sha256"] = entry.get("observed_sha256", entry.get("sha256"))
            rows.append(result)

    raw_rows_path = evidence_dir / "raw-rows.jsonl"
    raw_rows_path.write_text("".join(json.dumps(row, sort_keys=True, ensure_ascii=False) + "\n" for row in rows), encoding="utf-8")
    aggregate = aggregate_rows(rows, expanded)
    aggregate["fetch_blockers"] = fetch_blockers
    aggregate["generation_blockers"] = generation_blockers
    write_json(evidence_dir / "aggregate.json", aggregate)
    write_summary(evidence_dir / "summary.md", manifest, expanded, rows, aggregate, tool_versions)

    print(f"rows={aggregate['rows']} statuses={json.dumps(aggregate['status_counts'], sort_keys=True)}")
    print(f"bytes={aggregate['input_bytes']}->{aggregate['after_bytes']} saved={aggregate['saved_bytes']}")
    print(f"optimized={aggregate['optimized_rows']} blockers={len(aggregate['blockers']) + len(fetch_blockers) + len(generation_blockers)}")
    return 1 if aggregate["blockers"] or fetch_blockers or generation_blockers or aggregate["rows"] != aggregate["expected_rows"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
