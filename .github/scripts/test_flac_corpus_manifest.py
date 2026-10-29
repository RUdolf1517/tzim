#!/usr/bin/env python3
"""Offline integrity checks for the fixed, provenance-pinned FLAC corpus."""

from __future__ import annotations

import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST_PATH = ROOT / ".github/data/flac-corpus.json"


class FlacCorpusManifestTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
        cls.corpus = cls.manifest["corpus"]
        cls.inputs = cls.manifest["inputs"]
        cls.derived = cls.manifest["derived_inputs"]

    def test_manifest_pins_repository_commit_and_license(self) -> None:
        self.assertEqual(self.manifest["schema"], 1)
        self.assertEqual(self.corpus["repository"], "ietf-wg-cellar/flac-test-files")
        self.assertEqual(self.corpus["commit"], "aa7b0c6cf32994c106ae517a08134c28a96ff5b2")
        self.assertEqual(self.corpus["license"], "CC0-1.0")
        self.assertEqual(self.corpus["license_path"], "LICENSE.txt")
        self.assertIn(self.corpus["commit"], self.corpus["repository_url"])

    def test_fixed_corpus_has_20_to_50_rows_without_outcome_selection(self) -> None:
        rows = self.inputs + self.derived
        self.assertGreaterEqual(len(rows), 20)
        self.assertLessEqual(len(rows), 50)
        self.assertEqual(len(rows), 43)
        self.assertEqual(len({row["id"] for row in rows}), len(rows))
        self.assertEqual(self.manifest["derived_levels"], [0, 5, 8])
        self.assertEqual(len(self.manifest["derived_sources"]), 2)
        self.assertEqual(
            {row["flac_compression_level"] for row in self.derived},
            {0, 5, 8},
        )
        self.assertEqual({row["sample_format"] for row in self.derived}, {
            "16-bit, stereo, 44.1 kHz", "24-bit stereo, 96 kHz"
        })
        self.assertTrue(all("before Tzim measurement" in row["purpose"] for row in self.derived))

    def test_each_external_input_has_fixed_content_identity(self) -> None:
        ids: set[str] = set()
        paths: set[str] = set()
        for row in self.inputs:
            with self.subTest(row=row["id"]):
                self.assertRegex(row["id"], r"^[a-z0-9-]+$")
                self.assertIn(row["kind"], {"subset", "uncommon", "faulty"})
                self.assertTrue(row["path"].endswith(".flac"))
                self.assertGreater(row["size"], 0)
                self.assertRegex(row["git_blob_sha1"], r"^[0-9a-f]{40}$")
                self.assertRegex(row["sha256"], r"^[0-9a-f]{64}$")
                self.assertIn(row["handler_expectation"], {
                    "valid-input", "documented-refusal", "invalid-control"
                })
                self.assertTrue(row["purpose"])
                self.assertNotIn(row["path"], paths)
                ids.add(row["id"])
                paths.add(row["path"])
        self.assertEqual(len(self.inputs), 37)
        self.assertEqual(
            {row["kind"] for row in self.inputs}, {"subset", "uncommon", "faulty"}
        )
        self.assertEqual(
            sum(row["size"] for row in self.inputs), 97_233_688
        )
        self.assertTrue(all(source["parent_id"] in ids for source in self.manifest["derived_sources"]))
        self.assertTrue(set(self.manifest["fuzz_seed_ids"]).issubset(ids))
        self.assertLessEqual(
            sum(row["size"] for row in self.inputs if row["id"] in self.manifest["fuzz_seed_ids"]),
            10_000_000,
        )

    def test_declared_coverage_includes_contract_edges_and_metadata(self) -> None:
        coverage = "\n".join(self.manifest["coverage"])
        for required in (
            "8-, 16-, and 24-bit",
            "mono, stereo",
            "levels 0, 5, and 8",
            "variable-blocksize",
            "PADDING, SEEKTABLE, PICTURE, VORBIS_COMMENT, CUESHEET",
            "corrupt STREAMINFO",
            "malformed metadata length",
        ):
            with self.subTest(required=required):
                self.assertIn(required, coverage)

    def test_derived_manifest_rows_match_the_declared_levels(self) -> None:
        expected_ids = [
            f"{source['id']}-{level}"
            for source in self.manifest["derived_sources"]
            for level in self.manifest["derived_levels"]
        ]
        self.assertEqual([row["id"] for row in self.derived], expected_ids)
        parent_ids = {source["parent_id"] for source in self.manifest["derived_sources"]}
        self.assertTrue(all(row["parent_id"] in parent_ids for row in self.derived))


if __name__ == "__main__":
    unittest.main()
