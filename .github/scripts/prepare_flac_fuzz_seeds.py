#!/usr/bin/env python3
"""Fetch the pinned CC0 FLAC seeds into ignored cargo-fuzz working directories."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / ".github/data/flac-corpus.json"


def git_blob_sha1(data: bytes) -> str:
    return hashlib.sha1(b"blob " + str(len(data)).encode("ascii") + b"\0" + data).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    parser.add_argument("--roundtrip-dir", type=Path, required=True)
    parser.add_argument("--strip-dir", type=Path, required=True)
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    corpus = manifest["corpus"]
    rows = {row["id"]: row for row in manifest["inputs"]}
    args.roundtrip_dir.mkdir(parents=True, exist_ok=True)
    args.strip_dir.mkdir(parents=True, exist_ok=True)
    failures: list[str] = []
    for row_id in manifest["fuzz_seed_ids"]:
        row = rows[row_id]
        path = quote(row["path"], safe="/")
        endpoint = (
            f"repos/{corpus['repository']}/contents/{path}?ref={corpus['commit']}"
        )
        result = subprocess.run(
            ["gh", "api", "--header", "Accept: application/vnd.github.raw", endpoint],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        if result.returncode != 0:
            error = result.stderr.decode("utf-8", "replace").strip()
            failures.append(
                f"{row_id}: https://api.github.com/{endpoint}: {error}; "
                "restore access to the exact pinned CC0 source; do not substitute a seed"
            )
            continue
        data = result.stdout
        observed = hashlib.sha256(data).hexdigest()
        blob = git_blob_sha1(data)
        if len(data) != row["size"] or observed != row["sha256"] or blob != row["git_blob_sha1"]:
            failures.append(
                f"{row_id}: https://github.com/{corpus['repository']}/blob/{corpus['commit']}/{path}: "
                f"observed bytes={len(data)} sha256={observed} git_blob={blob}; "
                f"expected bytes={row['size']} sha256={row['sha256']} git_blob={row['git_blob_sha1']}; "
                "restore the exact pinned source; do not substitute a seed"
            )
            continue
        filename = f"flac-{row_id}.flac"
        (args.roundtrip_dir / filename).write_bytes(data)
        (args.strip_dir / filename).write_bytes(data)
    if failures:
        for failure in failures:
            print(f"ERROR: {failure}")
        return 1
    total = sum(rows[row_id]["size"] for row_id in manifest["fuzz_seed_ids"])
    print(
        f"seeded {len(manifest['fuzz_seed_ids'])} exact FLAC inputs ({total} bytes) "
        "into roundtrip and strip"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
