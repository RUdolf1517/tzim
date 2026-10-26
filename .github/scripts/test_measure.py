#!/usr/bin/env python3
"""Negative controls for the two bugs these instruments have already had (#72).

Both numbers in the audits are per-file gains, so both instruments can be made to
look better by *measuring less*: a FITS check that only compares the image HDUs it
happened to rebuild, and a PDF walk that stops at the first stream it cannot touch.
Neither failure produces an error — the run stays green and the numbers stay
plausible. That is why they are guarded by tests that build the bad case on
purpose, and why there is no "legacy" flag in either instrument: the guard is here,
the shipped code has no mode in which the bug is reachable.

    /tmp/v/bin/python -m unittest discover -s .github/scripts -p 'test_*.py' -v

The suites need the same packages the instruments need (`numpy`, `astropy`,
`pikepdf`); `pymupdf` is only needed by the render check, which these tests do not
exercise.
"""

import importlib.util
import os
import tempfile
import unittest
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), os.path.join(HERE, name))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


fits_measure = load("fits-measure.py")
pdf_measure = load("pdf-measure.py")

import numpy as np  # noqa: E402
import pikepdf  # noqa: E402
from astropy.io import fits  # noqa: E402


class FitsHistoricalBug(unittest.TestCase):
    """The FITS instrument once verified fewer images than the file contained.

    A file with three eligible image HDUs is rebuilt; the denominator is the
    source's own count, and the numerator must equal it. Then the mapping is cut
    down the way that bug cut it, and the check has to say so.
    """

    def setUp(self):
        self.scratch = tempfile.mkdtemp(prefix="fits-control-")
        table = fits.BinTableHDU.from_columns(
            [fits.Column(name="X", format="J", array=np.arange(3))]
        )
        self.source = os.path.join(self.scratch, "three-images.fits")
        fits.HDUList(
            [
                fits.PrimaryHDU(np.arange(16, dtype=">i2").reshape(4, 4)),
                fits.ImageHDU(np.arange(16, dtype=">f4").reshape(4, 4), name="SECOND"),
                fits.ImageHDU(np.arange(9, dtype=">i4").reshape(3, 3), name="THIRD"),
                table,
            ]
        ).writeto(self.source, overwrite=True)

    def test_denominator_is_the_source_count(self):
        self.assertEqual(fits_measure.eligible_count(self.source), 3)

    def test_correct_rebuild_reports_three_of_three(self):
        candidate = os.path.join(self.scratch, "candidate.fits")
        images, mapping = fits_measure.rebuild(self.source, candidate)
        self.assertEqual(images, 3)
        self.assertEqual(len(mapping), fits_measure.eligible_count(self.source))
        self.assertEqual(sorted(orig for orig, _ in mapping), [0, 1, 2])
        self.assertEqual(fits_measure.verify(self.source, candidate, mapping), (True, "ok"))

    def test_mapping_that_skips_an_image_is_refused(self):
        """The historical bug, stated as a call: two of three images verified."""
        candidate = os.path.join(self.scratch, "candidate.fits")
        _, mapping = fits_measure.rebuild(self.source, candidate)
        ok, why = fits_measure.verify(self.source, candidate, mapping[:-1])
        self.assertFalse(ok)
        self.assertEqual(why, "images_2_of_3")

    def test_candidate_that_drops_an_image_is_refused(self):
        candidate = os.path.join(self.scratch, "candidate.fits")
        _, mapping = fits_measure.rebuild(self.source, candidate)
        with fits.open(candidate) as hdul:
            kept = [hdu.copy() for index, hdu in enumerate(hdul) if index != 3]
        truncated = os.path.join(self.scratch, "truncated.fits")
        fits.HDUList(kept).writeto(truncated, overwrite=True)
        ok, why = fits_measure.verify(self.source, truncated, mapping)
        self.assertFalse(ok, "a candidate missing an image HDU was accepted")
        self.assertTrue(why.startswith("array_") or why == "hdu_count", why)


class PdfHistoricalBug(unittest.TestCase):
    """The PDF walk once ended at the first stream carrying `/F`.

    An external stream (a `/F` file specification) is outside §4.11's slice, so the
    file is refused — but the traversal has to keep going, because the count of
    eligible streams is what the measurement divides by. A generator that `return`s
    instead of yielding the refusal silently reports fewer streams on every file
    that has one, and never touches the rest of the document.
    """

    def setUp(self):
        self.scratch = tempfile.mkdtemp(prefix="pdf-control-")
        self.path = os.path.join(self.scratch, "external-first.pdf")
        pdf = pikepdf.Pdf.new()
        page = pikepdf.Dictionary(Type=pikepdf.Name.Page, MediaBox=[0, 0, 200, 200])
        pdf.pages.append(pikepdf.Page(page))
        plain = b"q 1 0 0 1 0 0 cm Q " * 20
        self.first = pikepdf.Stream(pdf, zlib.compress(plain))
        self.second = pikepdf.Stream(pdf, zlib.compress(plain * 2))
        external = pikepdf.Stream(pdf, b"XXXX")
        external.F = pikepdf.String("external.dat")
        page.Contents = pdf.make_indirect(pikepdf.Array([external, self.first, self.second]))
        pdf.save(self.path)

    def test_refusal_does_not_end_the_walk(self):
        with pikepdf.open(self.path) as pdf:
            walked = list(pdf_measure.rewritable_streams(pdf))
        self.assertEqual(len(walked), 3, "the walk stopped early: %r" % (walked,))
        self.assertEqual(walked[0], (None, "stream_external"))
        self.assertEqual([why for _, why in walked[1:]], [None, None])
        self.assertTrue(all(stream is not None for stream, _ in walked[1:]))

    def test_both_eligible_streams_are_counted(self):
        streams, total, after = pdf_measure.faithful(self.path)
        self.assertEqual(streams, 2, "the numerator lost a stream")
        self.assertEqual(total, 72)
        self.assertEqual(after, 72)

    def test_every_stream_in_the_file_is_seen(self):
        with pikepdf.open(self.path) as pdf:
            self.assertEqual(len(list(pdf_measure.iter_streams(pdf))), 3)


if __name__ == "__main__":
    unittest.main()
