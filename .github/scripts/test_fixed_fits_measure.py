#!/usr/bin/env python3
"""Guard FITS primary-image round trips against Astropy's virtual CompImageHDU header."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

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


if __name__ == "__main__":
    unittest.main()
