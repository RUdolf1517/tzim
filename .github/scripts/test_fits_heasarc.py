#!/usr/bin/env python3
"""Regression tests for controlled refusal of malformed FITS candidates."""

from __future__ import annotations

import hashlib
import importlib.util
import os
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(
    os.environ.get(
        "TZIM_FITS_HEASARC_SCRIPT",
        str(Path(__file__).with_name("fits-heasarc.py")),
    )
)
SPEC = importlib.util.spec_from_file_location("fits_heasarc_under_test", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot import FITS gate from %s" % SCRIPT)
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)


class ControlledCandidateRefusalTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="tzim-fits-gate-tests-")
        self.root = Path(self.temporary.name)
        self.evidence_root = self.root / "evidence"
        self.scratch = self.root / "scratch"
        self.scratch.mkdir()
        self.evidence = GATE.Evidence(str(self.evidence_root))

    def tearDown(self):
        self.temporary.cleanup()

    def write_basic_image(self):
        original_path = self.root / "original.fits"
        original_path.write_bytes(
            GATE.image_hdu(16, [8, 1], struct.pack(">8h", *range(8)), primary=True)
        )
        return original_path

    def write_candidate_with_table_card(self, keyword, value):
        candidate = bytearray(GATE.empty_primary() + GATE.compressed_table_fixture())
        hdus = GATE.parse(bytes(candidate))
        table = hdus[1]
        card_index = next(
            index for index, card in enumerate(table.cards)
            if card[:8].strip() == keyword
        )
        start = table.start + card_index * GATE.CARD
        candidate[start:start + GATE.CARD] = GATE.int_card(keyword, value)
        path = self.root / ("candidate-%s.fits" % keyword.lower())
        path.write_bytes(candidate)
        return path

    def assert_o1_invalid_card_is_a_controlled_refusal(self, keyword, value, tag):
        original = self.write_basic_image()
        candidate = self.write_candidate_with_table_card(keyword, value)
        reconstructed = self.root / ("funpack-%s.fits" % tag)
        reconstructed.write_bytes(original.read_bytes())

        with mock.patch.object(GATE, "fitsverify", return_value=(0, "fitsverify ok\n")):
            with mock.patch.object(
                GATE,
                "funpack",
                return_value=(0, "funpack returned success\n", str(reconstructed)),
            ):
                result = GATE.check_candidate(
                    str(original), str(candidate), {"images": 1}, str(self.scratch),
                    self.evidence, tag,
                )

        self.assertIs(result["accepted"], False)
        self.assertEqual(result["controlled_failure"]["stage"], "parse tzim candidate")
        self.assertIn(keyword, result["controlled_failure"]["reason"])
        self.assertNotIn("Traceback", result["problems"][0])
        self.assertTrue(
            (self.evidence_root / ("controlled-refusal-%s.candidate.fits" % tag)).is_file()
        )
        self.assertTrue((self.evidence_root / ("controlled-refusal-%s.json" % tag)).is_file())

    def test_o1_negative_pcount_is_a_recorded_refusal(self):
        self.assert_o1_invalid_card_is_a_controlled_refusal("PCOUNT", -1, "o1-pcount-negative")

    def test_o1_unsupported_bitpix_is_a_recorded_refusal(self):
        self.assert_o1_invalid_card_is_a_controlled_refusal("BITPIX", 0, "o1-bitpix-zero")

    def test_o1_zero_gcount_is_a_recorded_refusal(self):
        self.assert_o1_invalid_card_is_a_controlled_refusal("GCOUNT", 0, "o1-gcount-zero")

    def test_corpus_manifest_binds_fixture_hash_and_exact_run_identity(self):
        fixture = self.root / "manifest-fixture.fits"
        fixture.write_bytes(b"pinned fixture bytes")
        case = {"name": fixture.name, "expect": "untouched", "status": "skipped"}
        with mock.patch.dict(
            os.environ,
            {
                "REQUESTED_SHA": "a" * 40,
                "CHECKED_OUT_SHA": "a" * 40,
                "GITHUB_RUN_ID": "1234",
                "GITHUB_RUN_ATTEMPT": "2",
            },
        ):
            manifest = GATE.corpus_manifest(str(self.root), [case])

        self.assertEqual(manifest["requested_sha"], "a" * 40)
        self.assertEqual(manifest["checked_out_sha"], "a" * 40)
        self.assertEqual(manifest["run_id"], "1234")
        self.assertEqual(manifest["run_attempt"], "2")
        self.assertEqual(manifest["fixtures"][0]["bytes"], len(b"pinned fixture bytes"))
        self.assertEqual(
            manifest["fixtures"][0]["sha256"],
            hashlib.sha256(b"pinned fixture bytes").hexdigest(),
        )
        self.assertFalse(manifest["external_astropy"]["included"])

    def test_unparseable_tzim_candidate_is_a_recorded_refusal(self):
        original = self.write_basic_image()
        candidate = self.root / "candidate.fits"
        candidate.write_bytes(b"not a complete FITS record")

        result = GATE.check_candidate(
            str(original), str(candidate), {"images": 1}, str(self.scratch),
            self.evidence, "bad-header",
        )

        self.assertIs(result["accepted"], False)
        self.assertEqual(result["controlled_failure"]["stage"], "parse tzim candidate")
        self.assertIn("ValueError", result["controlled_failure"]["reason"])
        self.assertIn("shorter than one 2880-byte record", result["controlled_failure"]["reason"])
        self.assertTrue(
            (self.evidence_root / "controlled-refusal-bad-header.candidate.fits").is_file()
        )
        self.assertTrue((self.evidence_root / "controlled-refusal-bad-header.json").is_file())
        self.assertEqual(
            result["controlled_failure"]["candidate_sha256"],
            hashlib.sha256(candidate.read_bytes()).hexdigest(),
        )
        self.assertNotIn("Traceback", result["problems"][0])

    def test_unparseable_funpack_reconstruction_is_a_recorded_refusal(self):
        original = self.write_basic_image()
        candidate = self.root / "candidate.fits"
        candidate.write_bytes(GATE.empty_primary() + GATE.compressed_table_fixture())
        reconstructed = self.root / "funpack-output.fits"
        reconstructed.write_bytes(b"malformed funpack output")

        with mock.patch.object(GATE, "fitsverify", return_value=(0, "fitsverify ok\n")):
            with mock.patch.object(
                GATE,
                "funpack",
                return_value=(0, "funpack returned success\n", str(reconstructed)),
            ):
                result = GATE.check_candidate(
                    str(original), str(candidate), {"images": 1}, str(self.scratch),
                    self.evidence, "bad-funpack-output",
                )

        self.assertIs(result["accepted"], False)
        self.assertEqual(
            result["controlled_failure"]["stage"], "parse funpack -C reconstruction"
        )
        self.assertIn("ValueError", result["controlled_failure"]["reason"])
        self.assertTrue(
            (self.evidence_root / "controlled-refusal-bad-funpack-output.funpack.fits").is_file()
        )
        self.assertTrue(
            (self.evidence_root / "controlled-refusal-bad-funpack-output.json").is_file()
        )
        self.assertNotIn("Traceback", result["problems"][0])

    def test_short_descriptor_is_a_recorded_tile_inflation_refusal(self):
        candidate = bytearray(GATE.empty_primary() + GATE.compressed_table_fixture())
        hdus = GATE.parse(bytes(candidate))
        table = hdus[1]
        for keyword, value in (("NAXIS1", 4), ("PCOUNT", 0)):
            card_index = next(
                index for index, card in enumerate(table.cards)
                if card[:8].strip() == keyword
            )
            start = table.start + card_index * GATE.CARD
            candidate[start:start + GATE.CARD] = GATE._card(
                "%-8s= %20d" % (keyword, value)
            )

        path = self.root / "short-descriptor.fits"
        path.write_bytes(candidate)
        self.assertEqual(len(GATE.parse(path.read_bytes())), 2)
        with self.assertRaises(struct.error):
            GATE.inflate_tiles(str(path))

        tiles, refusal = GATE.safe_inflate_tiles(
            str(path), self.evidence, "short-descriptor"
        )
        self.assertIsNone(tiles)
        self.assertIsNotNone(refusal)
        self.assertIs(refusal["accepted"], False)
        self.assertEqual(
            refusal["controlled_failure"]["stage"], "candidate tile inflation"
        )
        self.assertIn("error", refusal["controlled_failure"]["exception_type"].lower())
        self.assertTrue(
            (self.evidence_root / "controlled-refusal-short-descriptor.candidate.fits").is_file()
        )
        self.assertTrue(
            (self.evidence_root / "controlled-refusal-short-descriptor.json").is_file()
        )
        self.assertNotIn("Traceback", refusal["problems"][0])


if __name__ == "__main__":
    unittest.main(verbosity=2)
