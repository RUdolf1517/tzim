#!/usr/bin/env python3
"""Test literal FITS diagnostics, both controls and the unchanged threshold gates."""

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

    def _header(self, path: Path) -> tuple[int, bytes, list[bytes], bytes]:
        with fits.open(path, memmap=False, do_not_scale_image_data=True, uint=False) as hdus:
            hdu = hdus[0]
            offset = int(hdu._header_offset)
            raw = self.measure.raw_header(path, hdu)
        cards, padding, end_found = self.measure._header_cards_and_padding(raw)
        if not end_found:
            raise AssertionError(f"fixture {path} has no actual END card")
        return offset, raw, cards, padding

    def _write_header(self, source: Path, destination: Path, cards: list[bytes]) -> None:
        shutil.copyfile(source, destination)
        offset, raw, _old_cards, _old_padding = self._header(source)
        replacement = b"".join(cards)
        if len(replacement) > len(raw):
            raise AssertionError("test replacement exceeded its existing FITS header records")
        replacement += b" " * (len(raw) - len(replacement))
        with destination.open("r+b") as stream:
            stream.seek(offset)
            stream.write(replacement)

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

    def test_reference_funpack_ignores_only_post_END_record_padding(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.fits"
            reconstructed = root / "reconstructed.fits"
            fits.PrimaryHDU(data=np.arange(24, dtype=np.int16).reshape(4, 6)).writeto(source)
            offset, raw, cards, padding = self._header(source)
            self.assertTrue(padding)
            shutil.copyfile(source, reconstructed)
            with reconstructed.open("r+b") as stream:
                stream.seek(offset + len(cards) * 80)
                stream.write(b"X")

            result = self.measure.compare_reference_funpack(source, reconstructed)
            self.assertTrue(result["ok"], result)
            hdu = result["hdu_checks"][0]
            self.assertTrue(hdu["header_cards_through_end_equality"])
            self.assertTrue(hdu["header_record_equality"])
            self.assertFalse(hdu["header_record_bytes_equality"])
            self.assertFalse(hdu["header_padding_equality"])
            self.assertTrue(hdu["header_padding_only_difference"])
            self.assertEqual(hdu["header_card_differences"], [])
            self.assertEqual(hdu["first_actual_difference"]["kind"], "padding_only")
            self.assertEqual(hdu["header_padding_difference"]["original_byte_hex"], "20")
            self.assertEqual(hdu["header_padding_difference"]["reconstructed_byte_hex"], "58")
            self.assertTrue(hdu["storage_payload_equality"])

    def test_reference_funpack_detects_actual_END_and_comment_card_differences(self) -> None:
        for changed_keyword in ("END", "COMMENT"):
            with self.subTest(keyword=changed_keyword), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = root / "source.fits"
                reconstructed = root / "reconstructed.fits"
                hdu = fits.PrimaryHDU(data=np.arange(24, dtype=np.int16).reshape(4, 6))
                hdu.header.append(("COMMENT", "literal comment remains byte-for-byte significant"))
                hdu.writeto(source)
                _offset, _raw, cards, _padding = self._header(source)
                changed_cards = list(cards)
                card_index = (
                    len(cards) - 1
                    if changed_keyword == "END"
                    else next(i for i, card in enumerate(cards) if card[:8] == b"COMMENT ")
                )
                edited = bytearray(changed_cards[card_index])
                if changed_keyword == "END":
                    edited[79] = ord("X")
                else:
                    edited[edited.index(b"significant") + len(b"significant") - 1] = ord("X")
                changed_cards[card_index] = bytes(edited)
                self._write_header(source, reconstructed, changed_cards)

                result = self.measure.compare_reference_funpack(source, reconstructed)
                self.assertFalse(result["ok"], result)
                differences = result["hdu_checks"][0]["header_card_differences"]
                self.assertEqual(len(differences), 1)
                self.assertEqual(differences[0]["kind"], "changed")
                self.assertEqual(differences[0]["keyword"], changed_keyword)
                self.assertEqual(differences[0]["first_differing_byte_offset"], 79 if changed_keyword == "END" else edited.index(ord("X")))
                self.assertEqual(len(differences[0]["before_card_hex"]), 160)
                self.assertEqual(len(differences[0]["after_card_hex"]), 160)

    def test_reference_funpack_keeps_card_spacing_and_duplicate_cards_literal(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.fits"
            reconstructed = root / "reconstructed.fits"
            hdu = fits.PrimaryHDU(data=np.arange(24, dtype=np.int16).reshape(4, 6))
            hdu.header.append(("COMMENT", "spacing fixture"))
            hdu.header.append(("COMMENT", "duplicate fixture"))
            hdu.header.append(("COMMENT", "duplicate fixture"))
            hdu.header.append(("OBJECT", "after duplicates"))
            hdu.writeto(source)
            _offset, _raw, cards, _padding = self._header(source)

            spacing_index = next(
                i for i, card in enumerate(cards) if card[:8] == b"COMMENT " and b"spacing fixture" in card
            )
            spaced_card = cards[spacing_index][:8] + b" " + cards[spacing_index][8:-1]
            changed_cards = list(cards)
            changed_cards[spacing_index] = spaced_card
            self._write_header(source, reconstructed, changed_cards)
            spacing_result = self.measure.compare_reference_funpack(source, reconstructed)
            self.assertFalse(spacing_result["ok"], spacing_result)
            spacing_differences = spacing_result["hdu_checks"][0]["header_card_differences"]
            self.assertEqual(len(spacing_differences), 1)
            self.assertEqual(spacing_differences[0]["kind"], "changed")
            self.assertEqual(spacing_differences[0]["keyword"], "COMMENT")
            self.assertEqual(spacing_differences[0]["first_differing_byte_offset"], 8)

            duplicate_indexes = [
                i for i, card in enumerate(cards)
                if card[:8] == b"COMMENT " and b"duplicate fixture" in card
            ]
            self.assertEqual(len(duplicate_indexes), 2)
            dropped_index = duplicate_indexes[1]
            self._write_header(source, reconstructed, cards[:dropped_index] + cards[dropped_index + 1 :])
            duplicate_result = self.measure.compare_reference_funpack(source, reconstructed)
            self.assertFalse(duplicate_result["ok"], duplicate_result)
            duplicate_differences = duplicate_result["hdu_checks"][0]["header_card_differences"]
            self.assertEqual(len(duplicate_differences), 1)
            self.assertEqual(duplicate_differences[0]["kind"], "dropped")
            self.assertEqual(duplicate_differences[0]["keyword"], "COMMENT")
            self.assertEqual(duplicate_differences[0]["before_card"], cards[dropped_index].decode("ascii"))

    def test_reference_funpack_keeps_CHECKSUM_and_DATASUM_literal(self) -> None:
        for changed_keyword in ("CHECKSUM", "DATASUM"):
            with self.subTest(keyword=changed_keyword), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = root / "source.fits"
                reconstructed = root / "reconstructed.fits"
                fits.PrimaryHDU(data=np.arange(24, dtype=np.int16).reshape(4, 6)).writeto(
                    source, checksum=True
                )
                _offset, _raw, cards, _padding = self._header(source)
                card_index = next(
                    i for i, card in enumerate(cards) if card[:8] == changed_keyword.encode("ascii").ljust(8)
                )
                edited = bytearray(cards[card_index])
                value_start = edited.index(b"'", edited.index(b"=")) + 1
                edited[value_start] = ord("0") if edited[value_start] != ord("0") else ord("1")
                changed_cards = list(cards)
                changed_cards[card_index] = bytes(edited)
                self._write_header(source, reconstructed, changed_cards)

                result = self.measure.compare_reference_funpack(source, reconstructed)
                self.assertFalse(result["ok"], result)
                check = result["hdu_checks"][0]
                self.assertFalse(check["header_cards_through_end_equality"])
                self.assertTrue(check["storage_payload_equality"])
                self.assertEqual(len(check["header_card_differences"]), 1)
                difference = check["header_card_differences"][0]
                self.assertEqual(difference["kind"], "changed")
                self.assertEqual(difference["keyword"], changed_keyword)
                self.assertEqual(difference["before_card_hex"], cards[card_index].hex())
                self.assertEqual(difference["after_card_hex"], bytes(edited).hex())

    def test_reference_funpack_sequence_alignment_reports_inserted_and_dropped_cards(self) -> None:
        for action in ("insert", "drop"):
            with self.subTest(action=action), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = root / "source.fits"
                reconstructed = root / "reconstructed.fits"
                hdu = fits.PrimaryHDU(data=np.arange(24, dtype=np.int16).reshape(4, 6))
                hdu.header.append(("OBJECT", "fixed-source"))
                hdu.header.append(("BUNIT", "count/s"))
                hdu.header.append(("OBSERVER", "test-observer"))
                hdu.writeto(source)
                _offset, _raw, cards, _padding = self._header(source)
                bunit_index = next(i for i, card in enumerate(cards) if card[:8] == b"BUNIT   ")
                if action == "insert":
                    inserted_card = b"COMMENT inserted once for sequence alignment".ljust(80, b" ")
                    changed_cards = cards[:bunit_index] + [inserted_card] + cards[bunit_index:]
                    expected_kind = "inserted"
                    expected_keyword = "COMMENT"
                else:
                    changed_cards = cards[:bunit_index] + cards[bunit_index + 1 :]
                    expected_kind = "dropped"
                    expected_keyword = "BUNIT"
                self._write_header(source, reconstructed, changed_cards)

                result = self.measure.compare_reference_funpack(source, reconstructed)
                self.assertFalse(result["ok"], result)
                hdu_check = result["hdu_checks"][0]
                differences = hdu_check["header_card_differences"]
                self.assertEqual(len(differences), 1, differences)
                self.assertEqual(hdu_check["header_card_difference_counts"], {
                    "inserted": int(action == "insert"),
                    "dropped": int(action == "drop"),
                    "changed": 0,
                })
                self.assertEqual(differences[0]["kind"], expected_kind)
                self.assertEqual(differences[0]["keyword"], expected_keyword)
                if action == "insert":
                    self.assertIsNone(differences[0]["original_card_index"])
                    self.assertIsInstance(differences[0]["reconstructed_card_index"], int)
                    self.assertIsNone(differences[0]["before_card"])
                    self.assertEqual(len(differences[0]["after_card"]), 80)
                else:
                    self.assertIsInstance(differences[0]["original_card_index"], int)
                    self.assertIsNone(differences[0]["reconstructed_card_index"])
                    self.assertEqual(len(differences[0]["before_card"]), 80)
                    self.assertIsNone(differences[0]["after_card"])

    def test_reference_funpack_does_not_treat_a_real_blank_card_as_padding(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.fits"
            reconstructed = root / "reconstructed.fits"
            hdu = fits.PrimaryHDU(data=np.arange(24, dtype=np.int16).reshape(4, 6))
            hdu.header.append(("OBJECT", "blank-card fixture"))
            hdu.writeto(source)
            _offset, _raw, cards, _padding = self._header(source)
            blank_index = len(cards) - 1
            with_blank = cards[:blank_index] + [b" " * 80] + cards[blank_index:]
            self._write_header(source, reconstructed, with_blank)
            result = self.measure.compare_reference_funpack(reconstructed, source)
            self.assertFalse(result["ok"], result)
            difference = result["hdu_checks"][0]["header_card_differences"][0]
            self.assertEqual(difference["kind"], "dropped")
            self.assertEqual(difference["keyword"], "<blank>")
            self.assertEqual(difference["before_card"], " " * 80)
            self.assertEqual(len(difference["before_card_hex"]), 160)

    def test_reference_funpack_card_order_changes_do_not_cascade_after_the_reordered_cards(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.fits"
            reconstructed = root / "reconstructed.fits"
            hdu = fits.PrimaryHDU(data=np.arange(24, dtype=np.int16).reshape(4, 6))
            hdu.header.append(("OBJECT", "order fixture"))
            hdu.header.append(("BUNIT", "count/s"))
            hdu.header.append(("OBSERVER", "test-observer"))
            hdu.writeto(source)
            _offset, _raw, cards, _padding = self._header(source)
            object_index = next(i for i, card in enumerate(cards) if card[:8] == b"OBJECT  ")
            bunit_index = next(i for i, card in enumerate(cards) if card[:8] == b"BUNIT   ")
            reordered = list(cards)
            reordered[object_index], reordered[bunit_index] = reordered[bunit_index], reordered[object_index]
            self._write_header(source, reconstructed, reordered)

            result = self.measure.compare_reference_funpack(source, reconstructed)
            self.assertFalse(result["ok"], result)
            differences = result["hdu_checks"][0]["header_card_differences"]
            self.assertLessEqual(len(differences), 2, differences)
            self.assertNotIn("END", {item["original_keyword"] for item in differences})
            self.assertNotIn("OBSERVER", {item["original_keyword"] for item in differences})
            self.assertTrue(any(item["original_keyword"] in {"OBJECT", "BUNIT"} for item in differences))

    def test_untransformed_candidate_preservation_remains_raw_byte_strict_including_padding(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.fits"
            candidate = root / "candidate.fits"
            fits.PrimaryHDU(data=np.arange(24, dtype=np.int16).reshape(4, 6)).writeto(source)
            offset, _raw, cards, _padding = self._header(source)
            shutil.copyfile(source, candidate)
            with candidate.open("r+b") as stream:
                stream.seek(offset + len(cards) * 80)
                stream.write(b"X")

            result = self.measure.compare_fits(source, candidate)
            self.assertFalse(result["ok"], result)
            check = result["hdu_checks"][0]
            self.assertFalse(check["transformed"])
            self.assertFalse(check["raw_hdu_equal"])
            self.assertTrue(check["arrays_equal"])
            self.assertTrue(check["user_header_cards_equal"])

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

    def test_reference_rebuild_preserves_raw_cards_and_nontransformed_hdus(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "literal-source.fits"
            candidate = root / "literal-reference.fits"
            primary_data = np.arange(48 * 64, dtype=np.int16).reshape(48, 64)
            primary_data[0, 0] = -32768
            primary = fits.PrimaryHDU(data=primary_data)
            primary.header["BLANK"] = -32768
            primary.header["OBJECT"] = ("literal primary", "preserve primary user comment")
            primary.header.append(("HISTORY", "literal source history card"))
            extension = fits.ImageHDU(data=np.arange(32 * 40, dtype=np.int16).reshape(32, 40), name="SCI")
            extension.header["OBJECT"] = ("literal extension", "preserve extension user comment")
            extension.header["EXTVER"] = (3, "source extension version comment")
            extension.header["EXTLEVEL"] = (2, "source extension level comment")
            extension.header["BUNIT"] = ("Jy/pixel", "source unit comment")
            extension.header["BSCALE"] = (2.0, "source scale comment")
            extension.header["BZERO"] = (10.0, "source zero comment")
            fits.HDUList([primary, extension]).writeto(source, overwrite=True, checksum=True)

            eligible_images, mapping = self.fits_measure.rebuild(str(source), str(candidate))
            exact, reason = self.fits_measure.verify(str(source), str(candidate), mapping)
            self.assertEqual(eligible_images, 2)
            self.assertTrue(exact, reason)

            source_layouts = self.fits_measure._hdu_layouts(source)
            candidate_layouts = self.fits_measure._hdu_layouts(candidate)
            source_bytes = source.read_bytes()
            candidate_bytes = candidate.read_bytes()
            source_header = source_bytes[source_layouts[0][0] : source_layouts[0][1]]
            candidate_header = candidate_bytes[candidate_layouts[1][0] : candidate_layouts[1][1]]
            source_cards, _ = self.fits_measure._header_cards(source_header)
            candidate_cards, _ = self.fits_measure._header_cards(candidate_header)
            candidate_card_set = set(candidate_cards)
            for source_card in source_cards:
                keyword = self.fits_measure._card_keyword(source_card)
                mapped = self.fits_measure._mapped_image_card(keyword)
                if mapped and keyword != "END":
                    expected = mapped.encode("ascii").ljust(8, b" ") + source_card[8:]
                    self.assertIn(expected, candidate_card_set, (keyword, expected))
            for expected_keyword in ("BLANK", "OBJECT", "HISTORY"):
                source_card = next(card for card in source_cards if self.fits_measure._card_keyword(card) == expected_keyword)
                self.assertIn(source_card, candidate_card_set)
            candidate_extension_header = candidate_bytes[candidate_layouts[2][0] : candidate_layouts[2][1]]
            source_extension_header = source_bytes[source_layouts[1][0] : source_layouts[1][1]]
            source_extension_cards, _ = self.fits_measure._header_cards(source_extension_header)
            candidate_extension_cards, _ = self.fits_measure._header_cards(candidate_extension_header)
            self.assertIn(
                next(card for card in source_extension_cards if self.fits_measure._card_keyword(card) == "EXTNAME"),
                set(candidate_extension_cards),
            )
            for expected_keyword in ("OBJECT", "EXTVER", "EXTLEVEL", "BUNIT", "BSCALE", "BZERO"):
                self.assertIn(
                    next(card for card in source_extension_cards if self.fits_measure._card_keyword(card) == expected_keyword),
                    set(candidate_extension_cards),
                    expected_keyword,
                )

            # An ineligible empty primary with a checksum is copied as its exact
            # original FITS record, not rewritten with stale checksum cards.
            empty_source = root / "empty-primary-source.fits"
            empty_candidate = root / "empty-primary-reference.fits"
            empty_primary = fits.PrimaryHDU()
            empty_primary.header["OBJECT"] = ("empty primary", "TESS-like uncompressed primary")
            image = fits.ImageHDU(data=np.arange(64 * 96, dtype=np.int32).reshape(64, 96), name="SCI")
            fits.HDUList([empty_primary, image]).writeto(empty_source, overwrite=True, checksum=True)
            count, empty_mapping = self.fits_measure.rebuild(str(empty_source), str(empty_candidate))
            self.assertEqual(count, 1)
            empty_source_layout = self.fits_measure._hdu_layouts(empty_source)[0]
            empty_candidate_layout = self.fits_measure._hdu_layouts(empty_candidate)[0]
            empty_source_bytes = empty_source.read_bytes()
            empty_candidate_bytes = empty_candidate.read_bytes()
            self.assertEqual(
                empty_source_bytes[empty_source_layout[0] : empty_source_layout[2]],
                empty_candidate_bytes[empty_candidate_layout[0] : empty_candidate_layout[2]],
            )
            empty_exact, empty_reason = self.fits_measure.verify(
                str(empty_source), str(empty_candidate), empty_mapping
            )
            self.assertTrue(empty_exact, empty_reason)

    def test_reference_repair_restores_a_missing_literal_user_card(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "missing-user-source.fits"
            candidate = root / "missing-user-candidate.fits"
            image = fits.PrimaryHDU(data=np.arange(256, dtype=np.int16).reshape(16, 16))
            image.header["OBJECT"] = ("restored", "must be restored exactly")
            image.header.append(("HISTORY", "literal source history"))
            image.writeto(source, overwrite=True, checksum=True)

            # Keep Astropy's initial physical header so the test can model one
            # user card omitted by that writer before the raw-header repair.
            with mock.patch.object(self.fits_measure, "_preserve_reference_hdu_bytes"):
                self.fits_measure.rebuild(str(source), str(candidate))
            source_layout = self.fits_measure._hdu_layouts(source)[0]
            candidate_layout = self.fits_measure._hdu_layouts(candidate)[1]
            source_bytes = source.read_bytes()
            candidate_bytes = candidate.read_bytes()
            source_header = source_bytes[source_layout[0] : source_layout[1]]
            candidate_header = candidate_bytes[candidate_layout[0] : candidate_layout[1]]
            candidate_cards, candidate_padding = self.fits_measure._header_cards(candidate_header)
            missing = next(i for i, card in enumerate(candidate_cards) if self.fits_measure._card_keyword(card) == "OBJECT")
            without_object = candidate_cards[:missing] + candidate_cards[missing + 1 :]
            simulated_writer_header = b"".join(without_object) + b" " * len(candidate_padding)

            repaired = self.fits_measure._repair_compressed_header(
                source_header, simulated_writer_header, "missing-user-source.fits"
            )
            repaired_cards, _ = self.fits_measure._header_cards(repaired)
            source_object = next(
                card for card in self.fits_measure._header_cards(source_header)[0]
                if self.fits_measure._card_keyword(card) == "OBJECT"
            )
            self.assertIn(source_object, repaired_cards)
            self.assertEqual(len(repaired), len(candidate_header))

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
                    source = Path(cwd) / argv[-1]
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

    def test_fits_threshold_requires_reference_gates_and_both_controls(self) -> None:
        def positive_control(status: str) -> dict:
            passed = status == "passed"
            return {
                "control_id": "cfitsio-fpack-funpack-C-integer-primary-v1",
                "status": status,
                "ok": passed,
                "fixture_dtype": "int16",
                "fpack_float_array_oracle_used": False,
                "fpack_invocation": ["fpack", "-g", "-q", "0", "fixture.fits"],
                "fpack": {"returncode": 0 if passed else 1},
                "fitsverify": {"returncode": 0 if passed else 1},
                "funpack": {"returncode": 0 if passed else 1},
                "fitsverify_invocation": ["fitsverify", "-l", "fixture.fits.fz"],
                "funpack_invocation": ["funpack", "-C", "-O", "out.fits", "fixture.fits.fz"],
                "literal_header_storage_array_comparison": {
                    "diagnostic_schema": "tzim-funpack-roundtrip-diagnostics-v2",
                    "diagnostics_complete": True,
                    "ok": passed,
                },
            }

        def negative_control(passed: bool = True) -> dict:
            comparison = {
                "diagnostic_schema": "tzim-funpack-roundtrip-diagnostics-v2",
                "diagnostics_complete": True,
                "ok": False,
                "hdu_count_equal": True,
                "attribution": "candidate_comparison",
                "failure_class_counts": {"header_only": 1},
                "hdu_checks": [{"storage_payload_equality": True}],
            }
            return {
                "control_id": "astropy-reference-literal-header-negative-control-v1",
                "status": "passed" if passed else "failed",
                "ok": passed,
                "positive_control_status": "passed",
                "pre_corruption_verifier_exact": True,
                "mutation": {
                    "applied": True,
                    "keyword": "OBJECT",
                    "source_identity_unchanged": True,
                    "payload_bytes_touched": False,
                    "candidate_before_corruption": {"bytes": 2880, "sha256": "before"},
                    "candidate_after_corruption": {"bytes": 2880, "sha256": "after"},
                },
                "negative_gate_status": "failed",
                "negative_fitsverify_status": "passed",
                "negative_funpack_returncode": 0,
                "negative_gate_failure_class_counts": {"header_only": 1},
                "gate_validation": {
                    "status": "failed",
                    "identity_matches_measurement_candidate": True,
                    "source_identity_matches": True,
                    "astropy_verifier_exact": True,
                    "fitsverify": {"status": "passed", "candidate_returncode": 0},
                    "funpack": {"returncode": 0},
                    "reconstructed_array_literal_header_comparison": comparison,
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
        positive = positive_control("passed")
        negative = negative_control()
        self.assertTrue(self.measure.negative_control_passed(negative))
        self.assertTrue(self.recompute.negative_control_passed(negative))
        applied = self.measure.threshold_disposition(rows, empty_tier1, tier2, positive, negative)
        independently_applied = self.recompute.threshold_disposition(rows, empty_tier1, tier2, positive, negative)
        self.assertEqual(applied, independently_applied)
        self.assertEqual(applied["status"], "applied")
        self.assertIn("Tier 2 clears", applied["decision"])
        self.assertEqual(applied["tier1_threshold"]["result"], "empty set; not evaluated (not 0%)")

        failed_positive = positive_control("failed")
        control_blocked = self.measure.threshold_disposition(rows, empty_tier1, tier2, failed_positive, negative)
        independently_blocked = self.recompute.threshold_disposition(rows, empty_tier1, tier2, failed_positive, negative)
        self.assertEqual(control_blocked, independently_blocked)
        self.assertEqual(control_blocked["status"], "not_applied")
        self.assertTrue(any("positive control" in item for item in control_blocked["blockers"]))

        failed_negative = negative_control(passed=False)
        negative_blocked = self.measure.threshold_disposition(rows, empty_tier1, tier2, positive, failed_negative)
        independently_negative_blocked = self.recompute.threshold_disposition(
            rows, empty_tier1, tier2, positive, failed_negative
        )
        self.assertEqual(negative_blocked, independently_negative_blocked)
        self.assertEqual(negative_blocked["status"], "not_applied")
        self.assertTrue(any("negative control" in item for item in negative_blocked["blockers"]))

        incomplete_positive = positive_control("passed")
        incomplete_positive["funpack_invocation"].remove("-C")
        self.assertEqual(
            self.measure.threshold_disposition(rows, empty_tier1, tier2, incomplete_positive, negative)["status"],
            "not_applied",
        )
        self.assertEqual(
            self.recompute.threshold_disposition(rows, empty_tier1, tier2, incomplete_positive, negative)["status"],
            "not_applied",
        )

        rows[0]["reference_candidate_validation"]["status"] = "failed"
        blocked = self.measure.threshold_disposition(rows, empty_tier1, tier2, positive, negative)
        independently_blocked = self.recompute.threshold_disposition(rows, empty_tier1, tier2, positive, negative)
        self.assertEqual(blocked, independently_blocked)
        self.assertEqual(blocked["status"], "not_applied")
        self.assertEqual(blocked["decision"], "threshold not applied")
        self.assertTrue(blocked["blockers"])


if __name__ == "__main__":
    unittest.main()
