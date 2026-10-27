#!/usr/bin/env python3
"""Create a deterministic final-SHA evidence bundle from gate artifacts.

The bundle contains the exact source inventory and the evidence artifacts emitted
by the final-SHA HEASARC and measurement jobs. Gate failures are recorded, never
converted to success. Missing artifacts are represented explicitly so the bundle
still explains an incomplete run.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tarfile
from typing import Iterable

SCHEMA_VERSION = 1
GATE_ARTIFACTS = ("fits-heasarc-evidence", "measure-repro-evidence")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def git_value(root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(root), *args],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return result.stdout.decode("ascii").strip()


def source_inventory(root: Path) -> tuple[str, list[dict[str, object]]]:
    """Hash every tracked source file at the checked-out commit."""
    raw = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z"],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ).stdout
    paths = sorted(item.decode("utf-8", "surrogateescape") for item in raw.split(b"\0") if item)
    rows: list[dict[str, object]] = []
    inventory_digest = hashlib.sha256()
    for name in paths:
        path = root / name
        if not path.is_file() or path.is_symlink():
            raise ValueError("tracked source file is missing or not regular: %s" % name)
        digest = sha256_file(path)
        size = path.stat().st_size
        rows.append({"path": name, "bytes": size, "sha256": digest})
        inventory_digest.update(name.encode("utf-8", "surrogateescape") + b"\0")
        inventory_digest.update(digest.encode("ascii") + b"\0")
    return inventory_digest.hexdigest(), rows


def _payload_rows(root: Path, *, omit: Iterable[str] = ()) -> list[dict[str, object]]:
    omitted = set(omit)
    rows: list[dict[str, object]] = []
    for path in sorted(item for item in root.rglob("*") if item.is_file()):
        if path.is_symlink():
            raise ValueError("evidence artifact contains a symlink: %s" % path)
        relative = path.relative_to(root).as_posix()
        if relative in omitted:
            continue
        rows.append({"path": relative, "bytes": path.stat().st_size, "sha256": sha256_file(path)})
    return rows


def _write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _write_checksums(root: Path) -> None:
    rows = _payload_rows(root, omit=("SHA256SUMS",))
    text = "".join("%s  %s\n" % (row["sha256"], row["path"]) for row in rows)
    (root / "SHA256SUMS").write_text(text, encoding="utf-8")


def verify_directory(root: Path) -> list[str]:
    """Verify the bundle directory's inventory and source-manifest binding."""
    problems: list[str] = []
    checksums = root / "SHA256SUMS"
    if not checksums.is_file():
        return ["SHA256SUMS is missing"]
    expected: dict[str, str] = {}
    for line in checksums.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        try:
            digest, name = line.split("  ", 1)
        except ValueError:
            problems.append("malformed checksum line: %r" % line)
            continue
        expected[name] = digest
    actual_names = {
        path.relative_to(root).as_posix()
        for path in root.rglob("*")
        if path.is_file() and path != checksums
    }
    if actual_names != set(expected):
        problems.append("SHA256SUMS paths do not match the bundle payload paths")
    for name, digest in expected.items():
        path = root / name
        if not path.is_file():
            problems.append("checksum target is missing: %s" % name)
        elif sha256_file(path) != digest:
            problems.append("checksum mismatch: %s" % name)
    try:
        source_manifest = json.loads((root / "source-inventory.json").read_text(encoding="utf-8"))
        rows = source_manifest["files"]
        digest = hashlib.sha256()
        for row in rows:
            digest.update(row["path"].encode("utf-8", "surrogateescape") + b"\0")
            digest.update(row["sha256"].encode("ascii") + b"\0")
        if digest.hexdigest() != source_manifest["inventory_sha256"]:
            problems.append("source inventory digest does not match its file records")
    except (OSError, ValueError, KeyError, TypeError) as error:
        problems.append("source inventory is unreadable: %s" % error)
    return problems


def _tar_add(tar: tarfile.TarFile, source: Path, archive_name: str) -> None:
    info = tarfile.TarInfo(archive_name)
    info.size = source.stat().st_size
    info.mode = 0o644
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    info.mtime = 0
    info.pax_headers = {}
    with source.open("rb") as handle:
        tar.addfile(info, handle)


def write_deterministic_tar(root: Path, destination: Path) -> None:
    """Write a deterministic gzip-compressed POSIX PAX archive.

    PAX carries long UTF-8 paths in a length-prefixed ``path`` record; the
    logical member name is preserved exactly rather than truncated or renamed.
    Member order, PAX headers, tar metadata, and gzip timestamps are normalized
    so independently created bundles have identical bytes.
    """
    destination.parent.mkdir(parents=True, exist_ok=True)
    with destination.open("wb") as raw:
        with gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0, compresslevel=9) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as tar:
                for path in sorted(item for item in root.rglob("*") if item.is_file()):
                    relative = path.relative_to(root).as_posix()
                    archive_name = "evidence/" + relative
                    logical_name = PurePosixPath(archive_name)
                    if (
                        logical_name.is_absolute()
                        or ".." in logical_name.parts
                        or logical_name.as_posix() != archive_name
                    ):
                        raise ValueError("unsafe bundle path: %s" % relative)
                    _tar_add(tar, path, archive_name)


def verify_archive(path: Path) -> list[str]:
    """Inspect the finished tar without extracting untrusted paths."""
    problems: list[str] = []
    try:
        with tarfile.open(path, "r:gz") as archive:
            members = archive.getmembers()
            names = [member.name for member in members]
            if len(names) != len(set(names)):
                problems.append("archive contains duplicate paths")
            payloads: dict[str, bytes] = {}
            for member in members:
                name = PurePosixPath(member.name)
                if name.is_absolute() or ".." in name.parts or not member.isfile():
                    problems.append("unsafe or non-regular archive entry: %s" % member.name)
                    continue
                stream = archive.extractfile(member)
                if stream is None:
                    problems.append("archive entry cannot be read: %s" % member.name)
                    continue
                payloads[member.name] = stream.read()
            checksum_bytes = payloads.get("evidence/SHA256SUMS")
            if checksum_bytes is None:
                problems.append("archive has no evidence/SHA256SUMS")
            else:
                listed: dict[str, str] = {}
                for line in checksum_bytes.decode("utf-8").splitlines():
                    if not line.strip():
                        continue
                    try:
                        digest, name = line.split("  ", 1)
                    except ValueError:
                        problems.append("malformed archive checksum line: %r" % line)
                        continue
                    listed["evidence/" + name] = digest
                actual = set(payloads) - {"evidence/SHA256SUMS"}
                if actual != set(listed):
                    problems.append("archive checksum paths do not match archive payload paths")
                for name, digest in listed.items():
                    if name in payloads and sha256_bytes(payloads[name]) != digest:
                        problems.append("archive checksum mismatch: %s" % name)
            manifest_bytes = payloads.get("evidence/bundle-manifest.json")
            if manifest_bytes is None:
                problems.append("archive has no evidence/bundle-manifest.json")
            else:
                manifest = json.loads(manifest_bytes.decode("utf-8"))
                if manifest.get("schema_version") != SCHEMA_VERSION:
                    problems.append("unsupported bundle manifest schema")
                if not manifest.get("requested_sha") or not manifest.get("checked_out_sha"):
                    problems.append("bundle manifest omits requested or checked-out SHA")
                manifest_rows = manifest.get("payloads")
                if not isinstance(manifest_rows, list):
                    problems.append("bundle manifest has no payload inventory")
                else:
                    expected_payloads: set[str] = set()
                    for row in manifest_rows:
                        try:
                            name = "evidence/" + row["path"]
                            data = payloads[name]
                            expected_payloads.add(name)
                            if len(data) != row["bytes"] or sha256_bytes(data) != row["sha256"]:
                                problems.append("manifest payload mismatch: %s" % name)
                        except (KeyError, TypeError):
                            problems.append("manifest payload is missing or malformed: %r" % row)
                    actual_payloads = set(payloads) - {
                        "evidence/SHA256SUMS", "evidence/bundle-manifest.json"
                    }
                    if actual_payloads != expected_payloads:
                        problems.append("bundle manifest paths do not match archive payload paths")
            source_bytes = payloads.get("evidence/source-inventory.json")
            if source_bytes is None:
                problems.append("archive has no evidence/source-inventory.json")
            else:
                try:
                    source_manifest = json.loads(source_bytes.decode("utf-8"))
                    digest = hashlib.sha256()
                    for row in source_manifest["files"]:
                        digest.update(row["path"].encode("utf-8", "surrogateescape") + b"\0")
                        digest.update(row["sha256"].encode("ascii") + b"\0")
                    if digest.hexdigest() != source_manifest["inventory_sha256"]:
                        problems.append("archive source inventory digest does not match its file records")
                except (KeyError, TypeError, UnicodeError, ValueError) as error:
                    problems.append("archive source inventory is unreadable: %s" % error)
    except (OSError, tarfile.TarError, UnicodeError, ValueError, json.JSONDecodeError) as error:
        problems.append("bundle archive is unreadable: %s" % error)
    return problems


def create_bundle(args: argparse.Namespace) -> dict[str, object]:
    source_root = Path(args.source_root).resolve()
    input_root = Path(args.input_root).resolve()
    output_root = Path(args.output_dir).resolve()
    output_root.mkdir(parents=True, exist_ok=True)
    bundle_root = output_root / "evidence"
    if bundle_root.exists():
        shutil.rmtree(bundle_root)
    bundle_root.mkdir(parents=True)

    checked_out = git_value(source_root, "rev-parse", "HEAD")
    tree_sha = git_value(source_root, "rev-parse", "HEAD^{tree}")
    requested = args.requested_sha.lower()
    gate_identities = {
        "fits-heasarc": {
            "requested_sha": args.heasarc_requested_sha,
            "checked_out_sha": args.heasarc_checked_out_sha,
        },
        "measure-repro": {
            "requested_sha": args.measurement_requested_sha,
            "checked_out_sha": args.measurement_checked_out_sha,
        },
    }
    valid_requested = bool(re.fullmatch(r"[0-9a-f]{40}", requested))
    identity_matches = valid_requested and requested == checked_out.lower()
    for identity in gate_identities.values():
        gate_requested = str(identity["requested_sha"] or "").lower()
        gate_checked_out = str(identity["checked_out_sha"] or "").lower()
        identity_matches = identity_matches and gate_requested == requested and gate_checked_out == requested
    source_digest, source_rows = source_inventory(source_root)
    _write_json(
        bundle_root / "source-inventory.json",
        {"source_sha": checked_out, "source_tree": tree_sha,
         "inventory_sha256": source_digest, "files": source_rows},
    )

    gate_status = {
        "fits-heasarc": args.heasarc_result,
        "measure-repro": args.measurement_result,
    }
    artifacts: list[dict[str, object]] = []
    gates_root = bundle_root / "gates"
    gates_root.mkdir()
    for artifact_name in GATE_ARTIFACTS:
        source = input_root / artifact_name
        destination = gates_root / artifact_name
        if source.is_dir():
            shutil.copytree(source, destination, symlinks=False)
            files = _payload_rows(destination)
            artifacts.append({"name": artifact_name, "present": True, "files": len(files),
                              "bytes": sum(int(row["bytes"]) for row in files)})
        else:
            destination.mkdir()
            (destination / "MISSING.txt").write_text(
                "The workflow artifact %s was not available to the bundler.\n" % artifact_name,
                encoding="utf-8",
            )
            artifacts.append({"name": artifact_name, "present": False, "files": 0, "bytes": 0})

    gate_identity_matches = True
    for gate_name, artifact_name in (
        ("fits-heasarc", "fits-heasarc-evidence"),
        ("measure-repro", "measure-repro-evidence"),
    ):
        gate = gate_identities[gate_name]
        identity_path = gates_root / artifact_name / "run-identity.json"
        try:
            document = json.loads(identity_path.read_text(encoding="utf-8"))
        except (OSError, ValueError, TypeError) as error:
            document = {"error": "unreadable run identity: %s" % error}
        if not isinstance(document, dict):
            document = {"error": "run identity is not a JSON object"}
        document_matches = (
            str(document.get("requested_sha", "")).lower() == requested
            and str(document.get("checked_out_sha", "")).lower() == requested
            and str(document.get("run_id", "")) == str(args.run_id)
            and str(document.get("run_attempt", "")) == str(args.run_attempt)
            and str(gate["requested_sha"] or "").lower() == requested
            and str(gate["checked_out_sha"] or "").lower() == requested
        )
        gate["artifact_identity"] = document
        gate["artifact_identity_matches"] = document_matches
        gate_identity_matches = gate_identity_matches and document_matches
    identity_matches = identity_matches and gate_identity_matches

    manifest: dict[str, object] = {
        "schema_version": SCHEMA_VERSION,
        "repository": args.repository,
        "requested_sha": args.requested_sha,
        "checked_out_sha": checked_out,
        "checked_out_tree": tree_sha,
        "source_inventory_sha256": source_digest,
        "identity_matches": identity_matches,
        "gate_identities": gate_identities,
        "run_id": args.run_id,
        "run_attempt": args.run_attempt,
        "run_url": args.run_url,
        "gate_results": gate_status,
        "gate_artifacts": artifacts,
        "payloads": [],
        "verification": "The bundle is checked internally after packaging; download the archive and run SHA256SUMS verification before attaching it to a draft Release.",
    }
    readme = [
        "# Final-SHA evidence bundle",
        "",
        "- Repository: `%s`" % args.repository,
        "- Requested SHA: `%s`" % args.requested_sha,
        "- Checked-out SHA: `%s`" % checked_out,
        "- Source tree: `%s`" % tree_sha,
        "- Run: `%s` (attempt %s)" % (args.run_id, args.run_attempt),
        "- Run URL: %s" % args.run_url,
        "- HEASARC result: `%s`" % args.heasarc_result,
        "- Measurement result: `%s`" % args.measurement_result,
        "- Source inventory SHA-256: `%s`" % source_digest,
        "- HEASARC requested/checked-out SHA: `%s` / `%s`" % (
            args.heasarc_requested_sha, args.heasarc_checked_out_sha),
        "- Measurement requested/checked-out SHA: `%s` / `%s`" % (
            args.measurement_requested_sha, args.measurement_checked_out_sha),
        "- Gate checkout identities match the requested SHA: `%s`" % identity_matches,
        "",
        "The measurement result is the gate's actual conclusion. A `DIFFERENT` result remains a failure;",
        "the evidence bundle does not reinterpret or waive it.",
        "",
        "Contents are under `gates/`. Verify with `sha256sum -c SHA256SUMS` before attaching this bundle",
        "to a draft Release.",
    ]
    (bundle_root / "README.md").write_text("\n".join(readme) + "\n", encoding="utf-8")
    payloads = _payload_rows(bundle_root, omit=("bundle-manifest.json", "SHA256SUMS"))
    manifest["payloads"] = payloads
    _write_json(bundle_root / "bundle-manifest.json", manifest)
    _write_checksums(bundle_root)
    directory_problems = verify_directory(bundle_root)
    if directory_problems:
        raise RuntimeError("evidence directory failed self-check: " + "; ".join(directory_problems))

    archive_path = output_root / "evidence-bundle.tar.gz"
    write_deterministic_tar(bundle_root, archive_path)
    archive_problems = verify_archive(archive_path)
    self_check_path = output_root / "bundle-self-check.txt"
    if archive_problems:
        self_check_path.write_text("FAILED\n" + "\n".join(archive_problems) + "\n", encoding="utf-8")
        raise RuntimeError("evidence archive failed self-check: " + "; ".join(archive_problems))
    self_check_path.write_text(
        "PASS\ninternal SHA256SUMS and source manifest verified after archive creation\n",
        encoding="utf-8",
    )
    archive_digest = sha256_file(archive_path)
    (output_root / "evidence-bundle.tar.gz.sha256").write_text(
        "%s  evidence-bundle.tar.gz\n" % archive_digest, encoding="utf-8"
    )
    shutil.copyfile(bundle_root / "bundle-manifest.json", output_root / "bundle-manifest.json")
    shutil.copyfile(bundle_root / "README.md", output_root / "bundle-manifest.md")
    shutil.copyfile(bundle_root / "SHA256SUMS", output_root / "SHA256SUMS")
    return {
        "archive": str(archive_path),
        "archive_sha256": archive_digest,
        "identity_matches": identity_matches,
        "gate_results": gate_status,
        "artifact_presence": {row["name"]: row["present"] for row in artifacts},
        "self_check": "PASS",
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--input-root", required=True, help="directory containing downloaded gate artifacts")
    parser.add_argument("--source-root", default=".", help="checkout of the requested final SHA")
    parser.add_argument("--output-dir", required=True, help="where the archive and sidecars are written")
    parser.add_argument("--repository", required=True)
    parser.add_argument("--requested-sha", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--run-attempt", required=True)
    parser.add_argument("--run-url", required=True)
    parser.add_argument("--heasarc-result", required=True)
    parser.add_argument("--measurement-result", required=True)
    parser.add_argument("--heasarc-requested-sha", required=True)
    parser.add_argument("--heasarc-checked-out-sha", required=True)
    parser.add_argument("--measurement-requested-sha", required=True)
    parser.add_argument("--measurement-checked-out-sha", required=True)
    args = parser.parse_args(argv)
    try:
        result = create_bundle(args)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print("::error::evidence bundle failed: %s" % error, file=sys.stderr)
        return 1
    _write_json(Path(args.output_dir) / "bundle-result.json", result)
    print(json.dumps(result, indent=2, sort_keys=True))
    artifacts_present = all(result["artifact_presence"].values())
    return 0 if result["identity_matches"] and artifacts_present else 1


if __name__ == "__main__":
    sys.exit(main())
