#!/usr/bin/env python3
"""Exercise failure-preserving merge behavior for the fixed PDF manifest."""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("merge-fixed-pdf-manifests.py")
SOURCE_SHA = "a" * 40


class MergeFixedPdfManifestTests(unittest.TestCase):
    def run_merge(self, root: Path, ubuntu: Path, quartz: Path, out: Path) -> tuple[subprocess.CompletedProcess, dict]:
        result = subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--artifact-root",
                str(root),
                "--ubuntu-manifest",
                str(ubuntu),
                "--quartz-manifest",
                str(quartz),
                "--expected-source-sha",
                SOURCE_SHA,
                "--output",
                str(out),
            ],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        return result, json.loads(out.read_text(encoding="utf-8"))

    @staticmethod
    def write_manifest(path: Path, kind: str, source_sha: str, outputs: list[dict]) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            json.dumps(
                {
                    "schema": "tzim-fixed-pdf-generation-v1",
                    "kind": kind,
                    "repository_source_sha": source_sha,
                    "environment": {"test": True},
                    "outputs": outputs,
                }
            ),
            encoding="utf-8",
        )

    def test_missing_generator_manifests_retain_all_21_failure_rows(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result, manifest = self.run_merge(
                root / "artifacts",
                root / "missing-ubuntu.json",
                root / "missing-quartz.json",
                root / "out/frozen.json",
            )
            rows = manifest["outputs"]
            self.assertEqual(result.returncode, 1)
            self.assertEqual(manifest["output_count"], 21)
            self.assertEqual(len(rows), 21)
            self.assertEqual({row["status"] for row in rows}, {"failed"})
            self.assertEqual({row["generation_source_identity_valid"] for row in rows}, {False})
            self.assertEqual({item["manifest_status"] for item in manifest["generation_manifests"]}, {"missing"})
            self.assertEqual(manifest["expected_repository_source_sha"], SOURCE_SHA)

    def test_one_generated_file_keeps_20_other_fixed_rows(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / "artifacts/fixed-pdf-quartz/outputs/quartz/readme.pdf"
            artifact.parent.mkdir(parents=True)
            payload = b"fixed test PDF bytes"
            artifact.write_bytes(payload)
            quartz_manifest = root / "quartz.json"
            self.write_manifest(
                quartz_manifest,
                "quartz",
                SOURCE_SHA,
                [
                    {
                        "id": "quartz/readme",
                        "path": "quartz/readme.pdf",
                        "status": "generated",
                        "bytes": len(payload),
                        "sha256": hashlib.sha256(payload).hexdigest(),
                    }
                ],
            )
            result, manifest = self.run_merge(
                root / "artifacts",
                root / "missing-ubuntu.json",
                quartz_manifest,
                root / "out/frozen.json",
            )
            rows = {row["id"]: row for row in manifest["outputs"]}
            self.assertEqual(result.returncode, 1)
            self.assertEqual(len(rows), 21)
            self.assertEqual(rows["quartz/readme"]["status"], "generated")
            self.assertEqual(
                rows["quartz/readme"]["artifact_path"],
                "fixed-pdf-quartz/outputs/quartz/readme.pdf",
            )
            self.assertEqual(sum(row["status"] == "failed" for row in rows.values()), 20)

    def test_wrong_generator_source_sha_never_accepts_its_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / "artifacts/fixed-pdf-quartz/outputs/quartz/readme.pdf"
            artifact.parent.mkdir(parents=True)
            payload = b"wrong source PDF bytes"
            artifact.write_bytes(payload)
            quartz_manifest = root / "quartz.json"
            self.write_manifest(
                quartz_manifest,
                "quartz",
                "b" * 40,
                [
                    {
                        "id": "quartz/readme",
                        "path": "quartz/readme.pdf",
                        "status": "generated",
                        "bytes": len(payload),
                        "sha256": hashlib.sha256(payload).hexdigest(),
                    }
                ],
            )
            result, manifest = self.run_merge(
                root / "artifacts",
                root / "missing-ubuntu.json",
                quartz_manifest,
                root / "out/frozen.json",
            )
            quartz_rows = [row for row in manifest["outputs"] if row["id"].startswith("quartz/")]
            self.assertEqual(result.returncode, 1)
            self.assertEqual(len(quartz_rows), 6)
            self.assertEqual({row["status"] for row in quartz_rows}, {"failed"})
            self.assertEqual({row["generation_source_identity_valid"] for row in quartz_rows}, {False})
            self.assertTrue(all("source SHA" in row.get("error", "") for row in quartz_rows))

    def test_malformed_manifest_still_has_exact_failure_rows(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            malformed = root / "ubuntu.json"
            malformed.write_text("not json", encoding="utf-8")
            result, manifest = self.run_merge(
                root / "artifacts",
                malformed,
                root / "missing-quartz.json",
                root / "out/frozen.json",
            )
            ubuntu_rows = [row for row in manifest["outputs"] if row["id"].startswith(("latex/", "libreoffice/", "scans/"))]
            self.assertEqual(result.returncode, 1)
            self.assertEqual(len(ubuntu_rows), 15)
            self.assertEqual({row["status"] for row in ubuntu_rows}, {"failed"})
            self.assertTrue(all("JSONDecodeError" in row.get("error", "") for row in ubuntu_rows))


if __name__ == "__main__":
    unittest.main()
