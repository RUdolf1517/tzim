#!/usr/bin/env python3
"""Check per-file and weighted Tzim gains against independent PDF recomputation."""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MEASURE_PATH = ROOT / ".github/scripts/fixed-pdf-measure.py"
SPEC = importlib.util.spec_from_file_location("fixed_pdf_measure_tested", MEASURE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {MEASURE_PATH}")
MEASURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MEASURE)
RECOMPUTE_PATH = ROOT / ".github/scripts/recompute-fixed-pdf.py"


class FixedPdfGainTests(unittest.TestCase):
    def test_raw_faithful_rows_are_the_accepted_subset_and_include_formula_inputs(self) -> None:
        rows = [
            {
                "id": "latex/readme",
                "classification": "accepted",
                "bytes": 1234,
                "sha256": "a" * 64,
                "streams": 2,
                "flate_bytes": 300,
                "flate_after": 280,
                "faithful_pct": 100.0 * (300 - 280) / 1234,
            },
            {"id": "latex/unsafe", "classification": "refused", "bytes": 1000},
        ]
        self.assertEqual(MEASURE.accepted_faithful_rows(rows), [{
            "id": "latex/readme",
            "bytes": 1234,
            "sha256": "a" * 64,
            "streams": 2,
            "flate_bytes": 300,
            "flate_after": 280,
            "faithful_pct": 100.0 * (300 - 280) / 1234,
            "faithful_error": None,
        }])

    def test_registered_threshold_uses_faithful_pct_and_publishes_both_qpdf_distributions(self) -> None:
        rows = [
            {
                "id": f"accepted-{index:02d}",
                "classification": "accepted",
                "bytes": 1000,
                "faithful_pct": 0.25,
                "qpdf_pct": 0.4,
                "repack_pct": 2.0,
                "tzim": {"status": "optimized", "bytes": 990, "gain_pct": 1.0},
                "original_qpdf_check": {"returncode": 0},
                "qpdf_allowed_check": {"returncode": 0},
                "qpdf_repack_check": {"returncode": 0},
                "render_comparisons": [],
            }
            for index in range(18)
        ]
        rows.extend(
            {
                "id": f"refused-{index:02d}",
                "classification": "refused",
                "reason": "signed",
                "bytes": 1000,
                "tzim": {"status": "skipped", "bytes": 1000},
                "render_comparisons": [],
            }
            for index in range(3)
        )
        summary = MEASURE.aggregate(rows)
        threshold = MEASURE.threshold_disposition(rows, summary)
        self.assertEqual(threshold["status"], "applied")
        self.assertEqual(threshold["rule_result"], "refused")
        self.assertIn("faithful_pct", threshold["metric"])
        self.assertEqual(summary["qpdf_allowed_pct"]["n"], 18)
        self.assertEqual(summary["forbidden_repack_pct"]["n"], 18)
        self.assertEqual(summary["refusal_reasons"], {"signed": 3})

        mid_band_rows = [dict(row) for row in rows]
        for row in mid_band_rows:
            if row["classification"] == "accepted":
                row["faithful_pct"] = 2.0
        mid_band = MEASURE.aggregate(mid_band_rows)
        self.assertEqual(
            MEASURE.threshold_disposition(mid_band_rows, mid_band)["status"],
            "not_applied",
        )
        accepted_delta = MEASURE.threshold_disposition(mid_band_rows, mid_band, 70 * 1024)
        refused_delta = MEASURE.threshold_disposition(mid_band_rows, mid_band, 70 * 1024 + 1)
        self.assertEqual(accepted_delta["rule_result"], "admitted")
        self.assertEqual(refused_delta["rule_result"], "refused")

    def test_actual_savings_and_weighted_gain_recompute_from_all_21_rows(self) -> None:
        rows = [
            {
                "id": f"fixed-{index:02d}",
                "classification": "accepted",
                "bytes": source_bytes,
                "tzim": {
                    "status": "optimized",
                    "bytes": candidate_bytes,
                    "gain_pct": 100.0 * (source_bytes - candidate_bytes) / source_bytes,
                },
                "render_comparisons": [],
            }
            for index, source_bytes, candidate_bytes in (
                (0, 100, 90),
                (1, 200, 185),
            )
        ]
        rows.extend(
            {
                "id": f"fixed-{index:02d}",
                "classification": "refused",
                "bytes": 50,
                "tzim": {"status": "skipped", "bytes": 50},
                "render_comparisons": [],
            }
            for index in range(2, 21)
        )
        aggregate = MEASURE.aggregate(rows)
        aggregate["pre_registered_threshold_disposition"] = MEASURE.threshold_disposition(rows, aggregate)
        self.assertEqual(aggregate["tzim_optimized_n"], 2)
        self.assertEqual(aggregate["tzim_saved_bytes"], 25)
        self.assertEqual(aggregate["tzim_optimized_source_bytes"], 300)
        self.assertEqual(aggregate["tzim_optimized_candidate_bytes"], 275)
        self.assertAlmostEqual(aggregate["tzim_weighted_gain_pct"], 100 * 25 / 300)

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rows_path = root / "raw-rows.jsonl"
            aggregate_path = root / "aggregate.json"
            output_path = root / "recomputed.json"
            rows_path.write_text(
                "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows),
                encoding="utf-8",
            )
            aggregate_path.write_text(json.dumps(aggregate), encoding="utf-8")
            subprocess.run(
                [
                    sys.executable,
                    str(RECOMPUTE_PATH),
                    "--rows",
                    str(rows_path),
                    "--aggregate",
                    str(aggregate_path),
                    "--out",
                    str(output_path),
                ],
                check=True,
                cwd=ROOT,
                capture_output=True,
                text=True,
            )
            result = json.loads(output_path.read_text(encoding="utf-8"))
            self.assertTrue(result["matches_measurement_aggregate"], result["mismatches"])
            self.assertEqual(result["independent_recomputation"]["tzim_saved_bytes"], 25)


if __name__ == "__main__":
    unittest.main()
