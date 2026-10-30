#!/usr/bin/env python3
"""Test exact FITS diagnostics, positive-control wiring and unchanged threshold gates."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import numpy as np
from astropy.io import fits

ROOT = Path(__file__).resolve().parents[2]


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FixedFitsMeasureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.fits_measure = load_module("fixed_test_fits_measure", ROOT / ".github/scripts/fits-measure.py")
        cls.measure = load_module("fixed_test_measure", ROOT / ".github/scripts/fixed-fits-measure.py")
        cls.recompute = load_module("fixed_test_recompute", ROOT / ".github/scripts/recompute-fixed-fits.py")
        cls.measure.fits_measure = cls.fits_measure

    def test_compressed_primary_uses_compimagehdu_even_without_zimage_in_view_header(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.fits"
            candidate = root / "candidate.fits"
            image = fits.PrimaryHDU(data=np.arange(96, dtype=np.int16).reshape(8, 12))
            image.header["OBJECT"] = "round-trip fixture"
            image.header["EPOCH"] = 2000
            image.writeto(source)
            self.fits_measure.rebuild(str(source), str(candidate))

            with fits.open(candidate, memmap=False, do_not_scale_image_data=True, uint=False) as hdus:
                self.assertEqual(len(hdus), 2)
                self.assertIsInstance(hdus[1], fits.CompImageHDU)
                # Astropy presents the logical image header, not the raw ZIMAGE table header.
                self.assertIsNone(hdus[1].header.get("ZIMAGE"))

            result = self.measure.compare_fits(source, candidate)
            self.assertTrue(result["ok"], result)
            self.assertEqual(result["primary_offset"], 1)
            self.assertEqual(result["original_hdus"], 1)
            self.assertEqual(result["candidate_hdus"], 2)
            self.assertEqual(result["hdu_checks"][0]["reason"], "array-bytes-and-user-header-cards-identical")

    def test_an_ordinary_image_extension_is_not_accepted_as_the_primary_compression_offset(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.fits"
            candidate = root / "candidate.fits"
            data = np.arange(96, dtype=np.int16).reshape(8, 12)
            fits.PrimaryHDU(data=data).writeto(source)
            fits.HDUList([fits.PrimaryHDU(), fits.ImageHDU(data=data)]).writeto(candidate)

            result = self.measure.compare_fits(source, candidate)
            self.assertFalse(result["ok"])
            self.assertEqual(result["reason"], "hdu_count")
            self.assertEqual(result["offset"], 0)
            self.assertFalse(result["primary_offset_probe"]["second_hdu_is_compressed_image"])

    def test_reference_funpack_requires_literal_header_records_and_exact_array_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.fits"
            reconstructed = root / "reconstructed.fits"
            data = np.arange(96, dtype=np.int16).reshape(8, 12)
            hdu = fits.PrimaryHDU(data=data)
            hdu.header.append(("OBJECT", "fixed-source", "literal-card fixture"))
            hdu.writeto(source)
            shutil.copyfile(source, reconstructed)

            exact = self.measure.compare_reference_funpack(source, reconstructed)
            self.assertTrue(exact["ok"], exact)
            self.assertTrue(exact["hdu_checks"][0]["header_record_equality"])
            self.assertTrue(exact["hdu_checks"][0]["storage_payload_equality"])

            changed_header = fits.PrimaryHDU(data=data)
            changed_header.header.append(("OBJECT", "changed-source", "literal-card fixture"))
            changed_header.writeto(reconstructed, overwrite=True)
            different = self.measure.compare_reference_funpack(source, reconstructed)
            self.assertFalse(different["ok"], different)
            hdu = different["hdu_checks"][0]
            self.assertFalse(hdu["header_record_equality"])
            self.assertTrue(hdu["storage_payload_equality"])
            self.assertEqual(hdu["failure_class"], "header_only")
            card = hdu["header_card_differences"][0]
            self.assertEqual(card["hdu_index"], 0)
            self.assertEqual(card["keyword"], "OBJECT")
            self.assertEqual(card["mismatch_kind"], "header_card")
            self.assertIsInstance(card["card_index"], int)
            self.assertIsInstance(card["first_differing_byte_offset"], int)
            self.assertEqual(len(card["before_card"]), 80)
            self.assertEqual(len(card["after_card"]), 80)
            self.assertEqual(len(card["before_card_hex"]), 160)
            self.assertEqual(len(card["after_card_hex"]), 160)
            self.assertTrue(different["diagnostics_complete"])

    def test_reference_funpack_diagnostics_separate_payload_only_and_combined_failures(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.fits"
            reconstructed = root / "reconstructed.fits"
            data = np.arange(96, dtype=np.int16).reshape(8, 12)
            hdu = fits.PrimaryHDU(data=data)
            hdu.header.append(("OBJECT", "fixed-source", "literal-card fixture"))
            hdu.writeto(source)

            shutil.copyfile(source, reconstructed)
            with fits.open(reconstructed, mode="update", memmap=False) as hdus:
                data_offset = int(hdus[0]._data_offset)
            with reconstructed.open("r+b") as stream:
                stream.seek(data_offset)
                first = stream.read(1)
                stream.seek(data_offset)
                stream.write(bytes([first[0] ^ 0x01]))
            payload_only = self.measure.compare_reference_funpack(source, reconstructed)
            payload_check = payload_only["hdu_checks"][0]
            self.assertEqual(payload_check["failure_class"], "payload_only")
            self.assertTrue(payload_check["header_record_equality"])
            self.assertFalse(payload_check["storage_payload_equality"])
            payload_mismatch = next(
                item for item in payload_only["mismatch_records"] if item["mismatch_kind"] == "storage_payload"
            )
            self.assertEqual(payload_mismatch["hdu_index"], 0)
            self.assertIsNone(payload_mismatch["card_index"])
            self.assertEqual(payload_mismatch["first_differing_byte_offset"], 0)

            changed = fits.PrimaryHDU(data=data)
            changed.header.append(("OBJECT", "changed-source", "literal-card fixture"))
            changed.writeto(reconstructed, overwrite=True)
            with fits.open(reconstructed, mode="update", memmap=False) as hdus:
                data_offset = int(hdus[0]._data_offset)
            with reconstructed.open("r+b") as stream:
                stream.seek(data_offset)
                first = stream.read(1)
                stream.seek(data_offset)
                stream.write(bytes([first[0] ^ 0x01]))
            combined = self.measure.compare_reference_funpack(source, reconstructed)
            self.assertEqual(combined["hdu_checks"][0]["failure_class"], "combined")
            self.assertFalse(combined["hdu_checks"][0]["header_record_equality"])
            self.assertFalse(combined["hdu_checks"][0]["storage_payload_equality"])

    def test_reference_funpack_does_not_filter_checksum_cards(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.fits"
            reconstructed = root / "reconstructed.fits"
            fits.PrimaryHDU(data=np.arange(24, dtype=np.int16).reshape(4, 6)).writeto(source, checksum=True)
            shutil.copyfile(source, reconstructed)
            with fits.open(source, memmap=False) as hdus:
                card_offsets = [
                    int(hdus[0]._header_offset) + hdus[0].header.index(keyword) * 80 + 11
                    for keyword in ("CHECKSUM", "DATASUM")
                ]
            with reconstructed.open("r+b") as stream:
                for byte_offset in card_offsets:
                    stream.seek(byte_offset)
                    original_byte = stream.read(1)
                    stream.seek(byte_offset)
                    stream.write(b"Z" if original_byte != b"Z" else b"Y")
            result = self.measure.compare_reference_funpack(source, reconstructed)
            self.assertFalse(result["ok"])
            self.assertEqual(result["hdu_checks"][0]["failure_class"], "header_only")
            changed_keywords = {
                item["original_keyword"]
                for item in result["hdu_checks"][0]["header_card_differences"]
            }
            self.assertTrue({"CHECKSUM", "DATASUM"}.issubset(changed_keywords))

    def test_fits_measure_records_the_exact_candidate_identity_for_each_product(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.fits"
            candidate_dir = root / "reference-candidates"
            output_rows = root / "audit-rows.jsonl"
            manifest_path = root / "manifest.json"
            fits.PrimaryHDU(data=np.arange(120, dtype=np.int16).reshape(10, 12)).writeto(source)
            source_bytes = source.stat().st_size
            source_sha = hashlib.sha256(source.read_bytes()).hexdigest()
            manifest_path.write_text(json.dumps({
                "manifest_digest": "fixture-manifest",
                "corpora": [{
                    "name": "fixed-fixture",
                    "format": "fits",
                    "entries": [{
                        "path": "source.fits",
                        "bytes": source_bytes,
                        "sha256": source_sha,
                        "product_id": "fixture-product",
                    }],
                }],
            }), encoding="utf-8")
            argv = [
                "fits-measure.py",
                "--root", str(root),
                "--manifest", str(manifest_path),
                "--candidate-dir", str(candidate_dir),
                "--out", str(output_rows),
            ]
            with mock.patch.object(sys, "argv", argv):
                self.fits_measure.main()

            row = json.loads(output_rows.read_text(encoding="utf-8").splitlines()[0])
            identity = row["reference_candidate"]
            candidate = candidate_dir / identity["filename"]
            self.assertEqual(identity["product_id"], "fixture-product")
            self.assertTrue(candidate.is_file())
            self.assertEqual(identity["bytes"], candidate.stat().st_size)
            self.assertEqual(identity["sha256"], hashlib.sha256(candidate.read_bytes()).hexdigest())
            self.assertTrue(row["exact"], row)

    def test_empty_tier_distribution_is_explicit_and_not_zero_percent(self) -> None:
        result = self.measure.distribution_stats([])
        self.assertEqual(result["n"], 0)
        self.assertEqual(result["status"], "empty")
        self.assertIsNone(result["median"])
        self.assertIsNone(result["share_positive"])

    def test_positive_control_uses_external_funpack_C_and_same_exact_comparison(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            logs = []

            def fake_run(argv, recorded, timeout=1800, cwd=None):
                row = {"argv": argv, "returncode": 0, "stdout": "ok\\n", "stderr": ""}
                if cwd is not None:
                    row["cwd"] = str(cwd)
                if argv[0] == "fpack":
                    source = Path(cwd) / argv[1]
                    shutil.copyfile(source, Path(str(source) + ".fz"))
                elif argv[0] == "funpack":
                    output = Path(argv[argv.index("-O") + 1])
                    packed = Path(argv[-1])
                    shutil.copyfile(packed, output)
                recorded.append(row)
                return row

            with mock.patch.object(self.measure, "run", side_effect=fake_run):
                control = self.measure.run_funpack_positive_control(
                    fpack="fpack", funpack="funpack", fitsverify="fitsverify", work=root, logs=logs
                )
            self.assertEqual(control["status"], "passed", control)
            self.assertTrue(control["literal_header_storage_array_comparison"]["ok"])
            self.assertTrue(self.measure.positive_control_passed(control), control)
            self.assertIn("-C", control["funpack_invocation"])
            self.assertFalse(control["fpack_float_array_oracle_used"])
            self.assertTrue(control["literal_header_storage_array_comparison"]["diagnostics_complete"])
            self.assertEqual(len(logs), 3)

    def test_reference_fitsverify_allows_only_exact_warning_only_source_baseline(self) -> None:
        source = {
            "returncode": 1,
            "stdout": "*** Warning: Keyword #17, EPOCH is deprecated. Use EQUINOX instead.\n",
            "stderr": "",
        }
        candidate = {
            "returncode": 1,
            "stdout": "*** Warning: Keyword #29, EPOCH is deprecated. Use EQUINOX instead.\n",
            "stderr": "",
        }
        self.assertEqual(
            self.measure.reference_fitsverify_status(source, candidate)["status"],
            "passed_inherited_warnings",
        )
        candidate["stdout"] += "*** Warning: another new warning\n"
        self.assertEqual(
            self.measure.reference_fitsverify_status(source, candidate)["status"],
            "failed",
        )

    def test_fits_threshold_requires_all_reference_candidate_gates(self) -> None:
        def control(status: str) -> dict:
            passed = status == "passed"
            return {
                "control_id": "cfitsio-fpack-funpack-C-integer-primary-v1",
                "status": status,
                "ok": passed,
                "fixture_dtype": "int16",
                "fpack_float_array_oracle_used": False,
                "fpack": {"returncode": 0 if passed else 1},
                "fitsverify": {"returncode": 0 if passed else 1},
                "funpack": {"returncode": 0 if passed else 1},
                "fitsverify_invocation": ["fitsverify", "-l", "fixture.fits.fz"],
                "funpack_invocation": ["funpack", "-C", "-O", "out.fits", "fixture.fits.fz"],
                "literal_header_storage_array_comparison": {
                    "diagnostic_schema": "tzim-funpack-roundtrip-diagnostics-v1",
                    "diagnostics_complete": True,
                    "ok": passed,
                },
            }

        rows = [
            {
                "product_id": f"product-{index}",
                "fetch_status": "downloaded",
                "measurement_status": "measured",
                "existing_two_tier_instrument": {"status": "tier2", "exact": True},
                "reference_candidate_validation": {"status": "passed"},
            }
            for index in range(14)
        ]
        tier2 = self.measure.distribution_stats([30.0] * 14)
        empty_tier1 = self.measure.distribution_stats([])
        positive_control = control("passed")
        applied = self.measure.threshold_disposition(rows, empty_tier1, tier2, positive_control)
        independently_applied = self.recompute.threshold_disposition(rows, empty_tier1, tier2, positive_control)
        self.assertEqual(applied, independently_applied)
        self.assertEqual(applied["status"], "applied")
        self.assertIn("Tier 2 clears", applied["decision"])
        self.assertEqual(applied["tier1_threshold"]["result"], "empty set; not evaluated (not 0%)")

        failed_control = control("failed")
        control_blocked = self.measure.threshold_disposition(rows, empty_tier1, tier2, failed_control)
        independently_blocked = self.recompute.threshold_disposition(rows, empty_tier1, tier2, failed_control)
        self.assertEqual(control_blocked, independently_blocked)
        self.assertEqual(control_blocked["status"], "not_applied")
        self.assertTrue(any("positive control" in item for item in control_blocked["blockers"]))

        incomplete_control = control("passed")
        incomplete_control["funpack_invocation"].remove("-C")
        self.assertEqual(
            self.measure.threshold_disposition(rows, empty_tier1, tier2, incomplete_control)["status"],
            "not_applied",
        )
        self.assertEqual(
            self.recompute.threshold_disposition(rows, empty_tier1, tier2, incomplete_control)["status"],
            "not_applied",
        )

        rows[0]["reference_candidate_validation"]["status"] = "failed"
        blocked = self.measure.threshold_disposition(rows, empty_tier1, tier2, positive_control)
        independently_blocked = self.recompute.threshold_disposition(rows, empty_tier1, tier2, positive_control)
        self.assertEqual(blocked, independently_blocked)
        self.assertEqual(blocked["status"], "not_applied")
        self.assertEqual(blocked["decision"], "threshold not applied")
        self.assertTrue(blocked["blockers"])


if __name__ == "__main__":
    unittest.main()
