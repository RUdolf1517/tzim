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
            manifest = GATE.corpus_manifest(str(self.root), [case], "residual")

        self.assertEqual(manifest["scope"], "residual")
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
        self.assertEqual(manifest["prior_run"]["run_id"], "40551071072")
        self.assertEqual(manifest["prior_run"]["sha"], "0232d92b3171b4b7af5cdb4910f852a66308e7b4")
        self.assertFalse(manifest["prior_run"]["replayed"])
        self.assertTrue(manifest["prior_run"]["context_only"])

    def test_acceptance_matrix_binds_all_three_hashes_and_strict_size(self):
        name = "residual_matrix_fixture.fits"
        identity = {"role": "PRIMARY", "bitpix": -32, "naxis": 2}
        result = {
            "name": name,
            "input_sha256": "a" * 64,
            "candidate_sha256": "b" * 64,
            "binary_sha256": "c" * 64,
            "bytes": {"before": 5760, "after": 4320},
            "tzim_exit": 0,
            "tzim": {"status": "optimized"},
            "source_matrix": [identity],
            "candidate_matrix": [identity],
            "candidate_acceptance": True,
            "strict_size": {"improved": True},
            "problems": [],
        }
        matrix = GATE.acceptance_matrix_summary(
            [{"name": name, "matrix": identity}], [result], "residual"
        )
        row = matrix["rows"][0]
        self.assertTrue(matrix["complete"])
        self.assertTrue(row["accepted"])
        self.assertTrue(row["identity_bound"])
        self.assertEqual(row["input_sha256"], "a" * 64)
        self.assertEqual(row["candidate_sha256"], "b" * 64)
        self.assertEqual(row["binary_sha256"], "c" * 64)
        self.assertFalse(matrix["inherited_run_replayed"])

        result.pop("binary_sha256")
        incomplete = GATE.acceptance_matrix_summary(
            [{"name": name, "matrix": identity}], [result], "residual"
        )
        self.assertFalse(incomplete["complete"])
        self.assertFalse(incomplete["rows"][0]["identity_bound"])

    def test_residual_reports_bind_scope_binary_and_runtime_boundary(self):
        summary = {
            "run": {
                "scope": "residual",
                "requested_sha": "a" * 40,
                "checked_out_sha": "a" * 40,
                "run_id": "1234",
                "run_attempt": "1",
            },
            "binary": {"path": "/tmp/tzim", "bytes": 123, "sha256": "b" * 64},
            "environment": {"python": "3.12", "platform": "Linux", "machine": "x86_64"},
            "tools": {},
            "fixtures": [],
            "controls": [],
            "acceptance_matrix": {
                "scope": "residual",
                "complete": True,
                "observed": {"bitpix": [-32, 8]},
                "rows": [],
            },
            "verification_scope": {
                "runtime": "runtime checks",
                "ci_only": "independent CI read-back",
                "limitation": "not a full-format guarantee",
            },
            "ok": True,
        }
        summary_text = GATE.render_summary(summary)
        diagnostics = GATE.render_diagnostics(summary, self.evidence)
        self.assertIn("residual scope", summary_text)
        self.assertIn("b" * 64, summary_text)
        self.assertIn("independent CI read-back", summary_text)
        self.assertIn("\n", diagnostics)
        self.assertIn("context only", diagnostics)
        self.assertIn("not full-format admission", diagnostics)
        self.assertNotIn("The external fixture did not run", diagnostics)

    def test_residual_diagnostics_include_matrix_errors_and_unchanged_refusals(self):
        hash_a = "a" * 64
        hash_b = "b" * 64
        summary = {
            "run": {
                "scope": "residual",
                "requested_sha": "c" * 40,
                "checked_out_sha": "c" * 40,
                "run_id": "4567",
                "run_attempt": "1",
            },
            "binary": {"path": "/tmp/tzim", "bytes": 123, "sha256": hash_b},
            "fixtures": [
                {
                    "name": "residual_bitpix16_naxis999_primary.fits",
                    "expect": "untouched",
                    "status": "skipped",
                    "input_sha256": hash_a,
                    "candidate_sha256": hash_a,
                    "tzim": {
                        "status": "skipped",
                        "note": "Z-form keyword ZNAXIS100 does not fit in eight columns",
                    },
                    "problems": [],
                }
            ],
            "controls": [],
            "acceptance_matrix": {
                "scope": "residual",
                "complete": False,
                "observed": {"bitpix": [-32, 8]},
                "rows": [
                    {
                        "fixture": "residual_bitpix64_naxis4_primary.fits",
                        "tzim_status": "optimized",
                        "tzim_note": None,
                        "input_sha256": hash_a,
                        "candidate_sha256": hash_b,
                        "binary_sha256": hash_b,
                        "strict_size": True,
                        "independent_acceptance": False,
                        "accepted": False,
                        "problems": ["funpack -C exited 1: BITPIX=64 mismatch"],
                    }
                ],
            },
        }

        diagnostics = GATE.render_diagnostics(summary, self.evidence)
        self.assertIn("funpack -C exited 1: BITPIX=64 mismatch", diagnostics)
        self.assertIn("Z-form keyword ZNAXIS100 does not fit in eight columns", diagnostics)
        self.assertIn("byte-identical", diagnostics)
        self.assertIn(hash_a, diagnostics)

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

    def test_residual_scope_contains_only_additive_issue_78_rows(self):
        fixture_root = self.root / "fixtures"
        all_cases = GATE.build_fixtures(str(fixture_root))
        selected = GATE.select_fixtures(all_cases, "residual")
        names = {case["name"] for case in selected}
        self.assertIn("residual_bitpix8_naxis1_primary.fits", names)
        self.assertIn("residual_bitpix16_naxis99_primary.fits", names)
        self.assertIn("residual_bitpix16_naxis999_primary.fits", names)
        self.assertIn("residual_theap_non_target.fits", names)
        self.assertNotIn("int16_primary.fits", names)
        self.assertNotIn("float32_specials.fits", names)
        matrix_cases = [case for case in selected if case.get("matrix")]
        self.assertEqual(
            {case["matrix"]["bitpix"] for case in matrix_cases},
            {8, 16, 32, 64, -32, -64},
        )
        self.assertEqual(
            {case["matrix"]["naxis"] for case in matrix_cases},
            {1, 3, 4, 99},
        )
        high_rank = next(
            case for case in selected
            if case["name"] == "residual_bitpix16_naxis999_primary.fits"
        )
        self.assertEqual(high_rank["expect"], "untouched")
        self.assertEqual(high_rank["status"], "skipped")
        self.assertEqual(high_rank["required_note"], "does not fit in eight columns")
        manifest = GATE.corpus_manifest(str(fixture_root), selected, "residual")
        manifest_names = {fixture["name"] for fixture in manifest["fixtures"]}
        self.assertEqual(manifest_names, names)
        manifest_high_rank = next(
            fixture for fixture in manifest["fixtures"]
            if fixture["name"] == "residual_bitpix16_naxis999_primary.fits"
        )
        self.assertEqual(manifest_high_rank["expectation"], "untouched")
        self.assertEqual(manifest_high_rank["required_note"], "does not fit in eight columns")
        self.assertFalse(manifest["prior_run"]["replayed"])
        self.assertFalse(manifest["external_astropy"]["included"])

    def test_residual_matrix_and_float_specials_parse_as_declared(self):
        fixture_root = self.root / "fixtures"
        cases = GATE.build_fixtures(str(fixture_root))
        matrix_cases = [case for case in cases if case.get("matrix")]
        self.assertEqual(len(matrix_cases), 6)
        for case in matrix_cases:
            hdus = GATE.parse((fixture_root / case["name"]).read_bytes())
            images = [hdu for hdu in hdus if hdu.kind() in ("PRIMARY", "IMAGE") and hdu.axes()]
            self.assertEqual(len(images), 1, case["name"])
            image = images[0]
            expected = case["matrix"]
            self.assertEqual(image.kind(), expected["role"], case["name"])
            self.assertEqual(image.int("BITPIX"), expected["bitpix"], case["name"])
            self.assertEqual(image.int("NAXIS"), expected["naxis"], case["name"])
            if expected["floating_storage"]:
                presence = GATE.float_specials_present(image.data, expected["bitpix"])
                self.assertTrue(all(presence.get(key) for key in expected["floating_storage"]), case["name"])

    def test_residual_rank_99_is_the_write_boundary_and_rank_999_is_a_refusal(self):
        fixture_root = self.root / "fixtures"
        cases = GATE.build_fixtures(str(fixture_root))
        accepted = next(
            case for case in cases
            if case["name"] == "residual_bitpix16_naxis99_primary.fits"
        )
        accepted_hdus = GATE.parse((fixture_root / accepted["name"]).read_bytes())
        self.assertEqual(accepted_hdus[0].int("NAXIS"), 99)
        self.assertEqual(len(accepted_hdus[0].axes()), 99)
        self.assertTrue(accepted["matrix"])

        refused = next(
            case for case in cases
            if case["name"] == "residual_bitpix16_naxis999_primary.fits"
        )
        refused_hdus = GATE.parse((fixture_root / refused["name"]).read_bytes())
        self.assertEqual(refused_hdus[0].int("NAXIS"), 999)
        self.assertEqual(len(refused_hdus[0].axes()), 999)
        self.assertEqual(refused["expect"], "untouched")
        self.assertEqual(refused["status"], "skipped")
        self.assertEqual(refused["required_note"], "does not fit in eight columns")

    def test_random_groups_and_theap_fixtures_have_expected_data_boundaries(self):
        fixture_root = self.root / "fixtures"
        GATE.build_fixtures(str(fixture_root))
        groups = GATE.parse((fixture_root / "residual_random_groups.fits").read_bytes())
        self.assertEqual(len(groups), 1)
        self.assertEqual(groups[0].value("GROUPS"), "T")
        self.assertEqual(len(groups[0].data), 528)
        theap = GATE.parse((fixture_root / "residual_theap_non_target.fits").read_bytes())
        table = next(hdu for hdu in theap if hdu.kind() == "BINTABLE")
        self.assertEqual(table.int("THEAP"), 16)
        self.assertEqual(len(table.data), 24)

    def test_float_storage_mutations_survive_gzip_but_change_pixel_bytes(self):
        source = GATE.float32_residual_specials(100)
        candidate = self.root / "float-candidate.fits"
        candidate.write_bytes(GATE.empty_primary() + GATE.tiles_fixture("GZIP_2", [source], 4))
        self.assertEqual(GATE.inflate_tiles(str(candidate)), source)

        for name, old, new in (
            ("nan", bytes.fromhex("7fc00001"), bytes.fromhex("7fc00002")),
            ("signed-zero", bytes.fromhex("00000000"), bytes.fromhex("80000000")),
        ):
            mutated = self.root / (name + ".fits")
            GATE.mutate_zimage_candidate(
                str(candidate),
                str(mutated),
                payload_mutator=lambda data, bitpix, old=old, new=new: GATE._mutate_float_word(
                    data, bitpix, old, new
                ),
            )
            actual = GATE.inflate_tiles(str(mutated))
            self.assertNotEqual(actual, source, name)
            self.assertEqual(len(actual), len(source), name)

    def test_descriptor_bounds_and_gzip_crc_mutations_are_refused(self):
        source = struct.pack(">16h", *range(16))
        candidate = self.root / "int-candidate.fits"
        candidate.write_bytes(GATE.empty_primary() + GATE.tiles_fixture("GZIP_1", [source], 2))

        def offset_past_heap(descriptors, heap, rows):
            descriptors[4:8] = struct.pack(">I", len(heap) + 1)
            return dict(offset=len(heap) + 1)

        bad_offset = self.root / "bad-offset.fits"
        GATE.mutate_zimage_candidate(
            str(candidate), str(bad_offset), descriptor_mutator=offset_past_heap
        )
        with self.assertRaisesRegex(ValueError, "lies outside its heap"):
            GATE.inflate_tiles(str(bad_offset))

        bad_count = self.root / "bad-count.fits"
        GATE.mutate_zimage_candidate(
            str(candidate),
            str(bad_count),
            descriptor_mutator=lambda descriptors, heap, rows: (
                descriptors.__setitem__(slice(0, 4), struct.pack(">I", len(heap) + 1))
                or dict(length=len(heap) + 1)
            ),
        )
        with self.assertRaisesRegex(ValueError, "lies outside its heap"):
            GATE.inflate_tiles(str(bad_count))

        negative_offset = self.root / "negative-offset.fits"
        GATE.mutate_zimage_candidate(
            str(candidate),
            str(negative_offset),
            descriptor_mutator=lambda descriptors, heap, rows: (
                descriptors.__setitem__(slice(4, 8), struct.pack(">i", -1))
                or dict(offset=-1)
            ),
        )
        with self.assertRaisesRegex(ValueError, "lies outside its heap"):
            GATE.inflate_tiles(str(negative_offset))

        zero_length = self.root / "zero-length.fits"
        GATE.mutate_zimage_candidate(
            str(candidate),
            str(zero_length),
            descriptor_mutator=lambda descriptors, heap, rows: (
                descriptors.__setitem__(slice(0, 4), struct.pack(">I", 0))
                or dict(length=0)
            ),
        )
        with self.assertRaisesRegex(ValueError, "lies outside its heap"):
            GATE.inflate_tiles(str(zero_length))

        bad_crc = self.root / "bad-crc.fits"
        GATE.mutate_zimage_candidate(
            str(candidate), str(bad_crc), compressed_member_mutator=GATE._damage_gzip_crc
        )
        with self.assertRaises((OSError, EOFError)):
            GATE.inflate_tiles(str(bad_crc))

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
        with self.assertRaisesRegex(ValueError, "descriptors and PCOUNT"):
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
