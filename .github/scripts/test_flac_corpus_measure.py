#!/usr/bin/env python3
"""Offline unit tests for FLAC corpus PCM and STREAMINFO-MD5 evidence."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / ".github/scripts/flac-corpus-measure.py"
SPEC = importlib.util.spec_from_file_location("flac_corpus_measure_tested", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT}")
MEASURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MEASURE)
RECOMPUTE_SPEC = importlib.util.spec_from_file_location(
    "recompute_flac_corpus_tested", ROOT / ".github/scripts/recompute_flac_corpus.py"
)
if RECOMPUTE_SPEC is None or RECOMPUTE_SPEC.loader is None:
    raise RuntimeError("cannot load recompute_flac_corpus.py")
RECOMPUTE = importlib.util.module_from_spec(RECOMPUTE_SPEC)
RECOMPUTE_SPEC.loader.exec_module(RECOMPUTE)


class FlacCorpusMeasurementTests(unittest.TestCase):
    def test_streaming_pcm_digest_records_sha256_and_flac_md5(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "pcm.bin"
            path.write_bytes(b"abc")
            self.assertEqual(
                MEASURE.digest_pcm(path),
                {
                    "bytes": 3,
                    "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                    "md5": "900150983cd24fb0d6963f7d28e17f72",
                },
            )

    def test_parse_md5_accepts_only_32_hex_digits(self) -> None:
        self.assertEqual(MEASURE.parse_md5("ABCDEF0123456789abcdef0123456789"), "abcdef0123456789abcdef0123456789")
        for value in (None, "", "0" * 31, "0" * 31 + "g"):
            with self.subTest(value=value):
                self.assertIsNone(MEASURE.parse_md5(value))

    def test_nonzero_streaminfo_md5_must_match_input_candidate_and_decoded_pcm(self) -> None:
        valid = "0123456789abcdef0123456789abcdef"
        helpers = (MEASURE.streaminfo_md5_check, RECOMPUTE.streaminfo_md5_check)
        for check in helpers:
            with self.subTest(check=check.__module__):
                self.assertTrue(check(valid, valid, valid)["ok"])
                self.assertFalse(check(valid, "f" * 32, valid)["ok"])
                self.assertFalse(check(valid, valid, "f" * 32)["ok"])

    def test_zero_input_sentinel_allows_candidate_md5_only_when_pcm_matches(self) -> None:
        zero = "0" * 32
        actual = "1234567890abcdef1234567890abcdef"
        helpers = (MEASURE.streaminfo_md5_check, RECOMPUTE.streaminfo_md5_check)
        for check in helpers:
            with self.subTest(check=check.__module__):
                result = check(zero, actual, actual)
                self.assertTrue(result["ok"])
                self.assertTrue(result["input_zero_md5_sentinel"])
                self.assertFalse(check(zero, zero, actual)["ok"])
                self.assertFalse(check(zero, actual, "f" * 32)["ok"])

    def test_aggregate_handles_nonoptimized_rows_without_candidate_checks(self) -> None:
        rows = [
            {
                "id": "pre-registered-skip",
                "status": "skipped",
                "before_bytes": 100,
                "after_bytes": 100,
                "saved_bytes": 0,
                "tzim_duration_ms": 1.0,
                "reference_flac_test": {"return_code": 0},
                "reference_metaflac": {"return_code": 0},
                "candidate_checks": None,
                "blockers": [],
            }
        ]
        expanded = {
            "inputs": [{"id": "pre-registered-skip", "fetch_status": "fetched"}],
            "derived_inputs": [],
        }
        aggregate = MEASURE.aggregate_rows(rows, expanded)
        self.assertEqual(aggregate, RECOMPUTE.recompute(rows, expanded))

        row = {
            **rows[0],
            "handler_expectation": "valid-input",
            "before_sha256": "a" * 64,
            "after_sha256": "a" * 64,
            "tzim_file_row": {
                "status": "skipped",
                "note": "independent decoder rejected a frame sequence gap; source bytes were preserved without modification",
            },
            "reference_flac_test": {"return_code": 0},
            "reference_metaflac": {"return_code": 0},
        }
        with tempfile.TemporaryDirectory() as temporary:
            summary = Path(temporary) / "summary.md"
            MEASURE.write_summary(
                summary,
                {
                    "corpus": {"repository": "example/flac", "commit": "abc123"},
                    "inputs": [{"size": 100}],
                    "derived_inputs": [],
                },
                {
                    "expanded_manifest_sha256": "b" * 64,
                    "inputs": [{"id": row["id"], "fetch_status": "fetched"}],
                    "derived_inputs": [],
                },
                [row],
                MEASURE.aggregate_rows([row], expanded),
                {
                    "flac": {"stdout": "flac 1.4.3"},
                    "metaflac": {"stdout": "metaflac 1.4.3"},
                },
            )
            report = summary.read_text(encoding="utf-8")
            self.assertIn("Per-row fixed input identities", report)
            self.assertIn("pre-registered-skip", report)
            self.assertIn("source flac pass / metaflac pass; candidate not run", report)
            self.assertIn("not run (no optimized candidate)", report)
            self.assertIn(
                "independent decoder rejected a frame sequence gap; source bytes were preserved without modification",
                report,
            )


if __name__ == "__main__":
    unittest.main()
