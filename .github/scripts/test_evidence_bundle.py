from __future__ import annotations

import importlib.util
import json
import shutil
import subprocess
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path

SCRIPT = Path(__file__).with_name("evidence-bundle.py")
SPEC = importlib.util.spec_from_file_location("evidence_bundle", SCRIPT)
evidence_bundle = importlib.util.module_from_spec(SPEC)
assert SPEC and SPEC.loader
SPEC.loader.exec_module(evidence_bundle)


class EvidenceBundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.source = self.root / "source"
        self.source.mkdir()
        subprocess.run(["git", "init", "-q", str(self.source)], check=True)
        subprocess.run(["git", "-C", str(self.source), "config", "user.name", "Evidence Test"], check=True)
        subprocess.run(["git", "-C", str(self.source), "config", "user.email", "evidence@example.invalid"], check=True)
        (self.source / "tracked.txt").write_text("source at exact SHA\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(self.source), "add", "tracked.txt"], check=True)
        subprocess.run(["git", "-C", str(self.source), "commit", "-qm", "fixture"], check=True)
        self.sha = subprocess.run(
            ["git", "-C", str(self.source), "rev-parse", "HEAD"],
            check=True,
            text=True,
            stdout=subprocess.PIPE,
        ).stdout.strip()
        self.inputs = self.root / "artifact-inputs"
        for name, text in (
            ("fits-heasarc-evidence", "HEASARC result\n"),
            ("measure-repro-evidence", "measurement DIFFERENT\n"),
        ):
            directory = self.inputs / name
            directory.mkdir(parents=True)
            (directory / "summary.txt").write_text(text, encoding="utf-8")
            (directory / "run-identity.json").write_text(
                json.dumps(
                    {
                        "requested_sha": self.sha,
                        "checked_out_sha": self.sha,
                        "run_id": "12345",
                        "run_attempt": "1",
                    }
                ) + "\n",
                encoding="utf-8",
            )
        self.output = self.root / "bundle-output"

    def tearDown(self):
        self.temp.cleanup()

    def arguments(self, *, requested_sha=None, measurement_result="failure"):
        return Namespace(
            input_root=str(self.inputs),
            source_root=str(self.source),
            output_dir=str(self.output),
            repository="example/tzim",
            requested_sha=requested_sha or self.sha,
            run_id="12345",
            run_attempt="1",
            run_url="https://github.com/example/tzim/actions/runs/12345",
            heasarc_result="success",
            measurement_result=measurement_result,
            heasarc_requested_sha=self.sha,
            heasarc_checked_out_sha=self.sha,
            measurement_requested_sha=self.sha,
            measurement_checked_out_sha=self.sha,
        )

    def test_bundle_is_deterministic_and_verifies_its_manifest(self):
        first = evidence_bundle.create_bundle(self.arguments())
        archive = self.output / "evidence-bundle.tar.gz"
        first_bytes = archive.read_bytes()
        self.assertTrue(first["identity_matches"])
        self.assertEqual(evidence_bundle.verify_archive(archive), [])

        second = evidence_bundle.create_bundle(self.arguments())
        self.assertEqual(first["archive_sha256"], second["archive_sha256"])
        self.assertEqual(first_bytes, archive.read_bytes())
        self.assertEqual((self.output / "bundle-self-check.txt").read_text().splitlines()[0], "PASS")

    def test_measurement_failure_is_recorded_not_promoted(self):
        result = evidence_bundle.create_bundle(self.arguments(measurement_result="failure"))
        self.assertEqual(result["gate_results"]["measure-repro"], "failure")
        self.assertEqual(evidence_bundle.verify_archive(self.output / "evidence-bundle.tar.gz"), [])
        manifest = (self.output / "bundle-manifest.json").read_text(encoding="utf-8")
        self.assertIn('"measure-repro": "failure"', manifest)

    def test_gate_checkout_mismatch_is_not_hidden(self):
        args = self.arguments()
        args.measurement_checked_out_sha = "0" * 40
        result = evidence_bundle.create_bundle(args)
        self.assertFalse(result["identity_matches"])
        manifest = (self.output / "bundle-manifest.json").read_text(encoding="utf-8")
        self.assertIn('"artifact_identity_matches": false', manifest)
        self.assertEqual(evidence_bundle.verify_archive(self.output / "evidence-bundle.tar.gz"), [])

    def test_missing_gate_artifact_is_explicit(self):
        shutil.rmtree(self.inputs / "measure-repro-evidence")
        result = evidence_bundle.create_bundle(self.arguments())
        self.assertFalse(result["artifact_presence"]["measure-repro-evidence"])
        self.assertTrue(
            (self.output / "evidence/gates/measure-repro-evidence/MISSING.txt").is_file()
        )
        self.assertEqual(evidence_bundle.verify_archive(self.output / "evidence-bundle.tar.gz"), [])

    def test_sha_mismatch_remains_visible_in_bundle_result(self):
        result = evidence_bundle.create_bundle(self.arguments(requested_sha="0" * 40))
        self.assertFalse(result["identity_matches"])
        self.assertEqual(evidence_bundle.verify_archive(self.output / "evidence-bundle.tar.gz"), [])


if __name__ == "__main__":
    unittest.main()
