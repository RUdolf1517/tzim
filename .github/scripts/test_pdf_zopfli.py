#!/usr/bin/env python3
"""Regression controls for the exact, versioned PDF Zopfli sample."""

import copy
import hashlib
import importlib.util
import json
import os
import random
import tempfile
import unittest
import zlib
from collections import Counter
from unittest import mock

HERE = os.path.dirname(os.path.abspath(__file__))


def load(name):
    spec = importlib.util.spec_from_file_location(
        name.replace("-", "_"), os.path.join(HERE, name)
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


zopfli_measure = load("pdf-zopfli-measure.py")
BASELINE_DIR = os.path.join(HERE, "measure-repro-baselines")
BASELINE = zopfli_measure.BASELINE
ZOPFLI_BASELINE = BASELINE["pdf_zopfli"]
SAMPLE_PATH = os.path.join(BASELINE_DIR, ZOPFLI_BASELINE["sample_manifest_file"])
RESULTS_PATH = os.path.join(BASELINE_DIR, ZOPFLI_BASELINE["results_file"])
CALIBRATION_AGGREGATE_PATH = os.path.join(
    BASELINE_DIR, ZOPFLI_BASELINE["calibration_aggregate_file"]
)


class VersionedSample(unittest.TestCase):
    def setUp(self):
        with open(SAMPLE_PATH, encoding="utf-8") as handle:
            self.sample = json.load(handle)
        self.results = zopfli_measure.load_jsonl(RESULTS_PATH)

    def test_baseline_assets_are_hash_pinned_and_complete(self):
        self.assertEqual(
            zopfli_measure.sha256_file(SAMPLE_PATH),
            ZOPFLI_BASELINE["sample_manifest_sha256"],
        )
        self.assertEqual(
            zopfli_measure.sha256_file(RESULTS_PATH),
            ZOPFLI_BASELINE["results_jsonl_sha256"],
        )
        self.assertEqual(
            zopfli_measure.sha256_file(CALIBRATION_AGGREGATE_PATH),
            ZOPFLI_BASELINE["calibration_aggregate_sha256"],
        )
        requirements_path = os.path.join(HERE, "measure-requirements.txt")
        with open(requirements_path, encoding="utf-8") as handle:
            requirements = handle.read()
        self.assertIn("zopfli==0.4.3 \\\n    --hash=sha256:769875152d0625c46707bcca57d4b2233fe653482067acd55fbf6ec525cb9bdc", requirements)
        self.assertEqual(self.sample["protocol_id"], "pdf-zopfli-small-v1")
        self.assertEqual(self.sample["accepted_rows"], 1009)
        self.assertEqual(self.sample["selection"]["eligible_rows"], 878)
        self.assertEqual(self.sample["selection"]["sample_files"], 150)
        self.assertEqual(len(self.sample["selected_rows"]), 150)
        self.assertEqual(len(self.results), 150)

    def test_seeded_global_sample_order_paths_hashes_and_counts(self):
        selected = self.sample["selected_rows"]
        self.assertEqual([row["sample_order"] for row in selected], list(range(1, 151)))
        paths = [row["manifest_path"] for row in selected]
        self.assertEqual(
            zopfli_measure.path_digest(paths),
            "91425c7fecbd4e86504efa855695f8add2c5e77d47307373f2b83b97941be61a",
        )
        self.assertEqual(
            zopfli_measure.identity_digest(selected),
            "0360aa7d5178a0aa7acc9f16c30a31a6be67024802a4af88ab69e77e31318ac2",
        )
        self.assertEqual(sum(row["bytes"] for row in selected), 1345005)
        self.assertEqual(Counter(row["corpus"] for row in selected), {
            "pdfjs": 39, "qpdf": 16, "verapdf": 95,
        })
        self.assertEqual(paths, [row["manifest_path"] for row in self.results])
        for selected_row, result in zip(selected, self.results):
            self.assertEqual(result["sample_order"], selected_row["sample_order"])
            self.assertEqual(result["manifest_sha256"], selected_row["manifest_sha256"])
            self.assertEqual(result["bytes"], selected_row["bytes"])
            self.assertEqual(result["streams"], selected_row["streams"])
            self.assertEqual(result["flate_bytes"], selected_row["flate_bytes"])
            self.assertIsNone(result["error"])
            self.assertEqual(result["roundtrip_streams"], result["streams"])

    def test_calibrated_integer_totals_and_distribution_are_exact(self):
        self.assertEqual(sum(row["roundtrip_streams"] for row in self.results), 576)
        self.assertEqual(sum(row["flate_bytes"] for row in self.results), 734998)
        self.assertEqual(sum(row["zopfli_encoded_bytes"] for row in self.results), 698990)
        self.assertEqual(sum(row["zopfli_after"] for row in self.results), 698978)
        values = [row["zopfli_pct"] for row in self.results]
        self.assertEqual(
            zopfli_measure.distribution(values),
            ZOPFLI_BASELINE["expected_aggregate"]["distribution"],
        )
        self.assertEqual(sum(value > 0 for value in values), 137)
        self.assertEqual(sum(value == 0 for value in values), 13)
        self.assertEqual(sum(value < 0 for value in values), 0)

    def test_selection_sorts_globally_then_uses_local_seeded_rng(self):
        rows = [
            dict(status="accepted", manifest_path="z/last.pdf", bytes=10),
            dict(status="accepted", manifest_path="a/first.pdf", bytes=10),
            dict(status="accepted", manifest_path="m/middle.pdf", bytes=65536),
            dict(status="accepted", manifest_path="large.pdf", bytes=65537),
            dict(status="refused", manifest_path="not-accepted.pdf", bytes=10),
        ]
        accepted, eligible, sample = zopfli_measure.select_sample(
            rows, sample_size=2, seed=57
        )
        ordered = sorted((row for row in rows if row.get("status") == "accepted"
                          and row["bytes"] <= 65536), key=lambda row: row["manifest_path"])
        self.assertEqual(len(accepted), 4)
        self.assertEqual([row["manifest_path"] for row in eligible],
                         ["a/first.pdf", "m/middle.pdf", "z/last.pdf"])
        self.assertEqual(sample, random.Random(57).sample(ordered, 2))
        self.assertNotIn("large.pdf", [row["manifest_path"] for row in eligible])

    def test_sample_manifest_comparison_rejects_missing_extra_reordered_and_substituted_rows(self):
        baseline_copy = copy.deepcopy(self.sample)
        self.assertEqual(zopfli_measure._sample_manifest_differences(baseline_copy, self.sample), [])
        for mutate in (
            lambda value: value["selected_rows"].pop(),
            lambda value: value["selected_rows"].append(dict(value["selected_rows"][-1])),
            lambda value: value["selected_rows"].__setitem__(
                0, value["selected_rows"][1]),
            lambda value: value["selected_rows"][0].update(manifest_sha256="0" * 64),
        ):
            changed = copy.deepcopy(self.sample)
            mutate(changed)
            self.assertTrue(zopfli_measure._sample_manifest_differences(changed, self.sample))

    def test_result_comparator_rejects_missing_extra_reordered_substituted_changed_and_error_rows(self):
        self.assertEqual(zopfli_measure.compare_result_rows(self.results, self.results), [])
        changed = copy.deepcopy(self.results)
        cases = []
        cases.append(changed[:-1])
        cases.append(changed + [copy.deepcopy(changed[-1])])
        reordered = copy.deepcopy(changed)
        reordered[0], reordered[1] = reordered[1], reordered[0]
        cases.append(reordered)
        substituted = copy.deepcopy(changed)
        substituted[0]["manifest_path"] = "pdfjs/test/pdfs/substituted.pdf"
        cases.append(substituted)
        changed_sha = copy.deepcopy(changed)
        changed_sha[0]["manifest_sha256"] = "0" * 64
        cases.append(changed_sha)
        changed_integer = copy.deepcopy(changed)
        changed_integer[0]["zopfli_after"] += 1
        cases.append(changed_integer)
        error_row = copy.deepcopy(changed)
        error_row[0]["error"] = "zlib.error: synthetic failure"
        cases.append(error_row)
        for case in cases:
            with self.subTest(first_row=case[0] if case else None, count=len(case)):
                self.assertTrue(zopfli_measure.compare_result_rows(case, self.results))


class StreamErrorControls(unittest.TestCase):
    """Open, inflate, compress and roundtrip errors are retained and fatal."""

    def _make_pdf(self, directory, raw_stream):
        import pikepdf

        path = os.path.join(directory, "fixture.pdf")
        pdf = pikepdf.Pdf.new()
        page_object = pikepdf.Dictionary(
            Type=pikepdf.Name.Page,
            MediaBox=[0, 0, 200, 200],
        )
        pdf.pages.append(pikepdf.Page(page_object))
        stream = pdf.make_stream(raw_stream)
        stream["/Filter"] = pikepdf.Name.FlateDecode
        page_object["/Contents"] = stream
        pdf.save(path)
        return path

    def _row(self, path, directory, relative="fixture.pdf"):
        size = os.path.getsize(path)
        return dict(
            path=path,
            manifest_path=relative,
            manifest_sha256=zopfli_measure.sha256_file(path),
            corpus="pdfjs",
            bytes=size,
            streams=1,
            flate_bytes=1,
        )

    def test_missing_input_open_failure_retains_the_sample_identity(self):
        with tempfile.TemporaryDirectory(prefix="zopfli-open-") as temporary:
            row = dict(
                manifest_path="missing.pdf", manifest_sha256="0" * 64,
                corpus="pdfjs", bytes=10, streams=1, flate_bytes=1,
            )
            result = zopfli_measure.measure_file(row, temporary, 7)
            self.assertIn("FileNotFoundError", result["error"])
            self.assertEqual(result["manifest_path"], "missing.pdf")
            self.assertEqual(result["sample_order"], 7)
            self.assertEqual(result["manifest_sha256"], "0" * 64)

    def test_pdf_open_failure_is_fatal(self):
        with tempfile.TemporaryDirectory(prefix="zopfli-broken-pdf-") as temporary:
            path = os.path.join(temporary, "fixture.pdf")
            with open(path, "wb") as handle:
                handle.write(b"not a PDF")
            row = self._row(path, temporary)
            result = zopfli_measure.measure_file(row, temporary, 1)
            self.assertIsNotNone(result["error"])
            self.assertTrue(result["error"])

    def test_inflate_failure_is_fatal(self):
        with tempfile.TemporaryDirectory(prefix="zopfli-inflate-") as temporary:
            path = self._make_pdf(temporary, b"not a zlib stream")
            result = zopfli_measure.measure_file(self._row(path, temporary), temporary, 2)
            self.assertIsNotNone(result["error"])
            self.assertIn("Error -3", result["error"])

    def test_compression_failure_is_fatal(self):
        with tempfile.TemporaryDirectory(prefix="zopfli-compress-") as temporary:
            path = self._make_pdf(temporary, zlib.compress(b"a small valid payload" * 10))
            with mock.patch.object(
                zopfli_measure.zopfli_zlib, "compress", side_effect=RuntimeError("compress failed")
            ):
                result = zopfli_measure.measure_file(self._row(path, temporary), temporary, 3)
            self.assertIn("RuntimeError: compress failed", result["error"])

    def test_roundtrip_mismatch_is_fatal(self):
        with tempfile.TemporaryDirectory(prefix="zopfli-roundtrip-") as temporary:
            path = self._make_pdf(temporary, zlib.compress(b"original payload" * 10))
            with mock.patch.object(
                zopfli_measure.zopfli_zlib, "compress",
                return_value=zlib.compress(b"different payload"),
            ):
                result = zopfli_measure.measure_file(self._row(path, temporary), temporary, 4)
            self.assertIn("roundtrip differs", result["error"])


if __name__ == "__main__":
    unittest.main()
