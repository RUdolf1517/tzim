#!/usr/bin/env python3
"""Metric negative controls and corpus-selection guards for #72.

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
import io
import json
import os
import tarfile
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
measure_repro = load("measure-repro.py")
measure_fetch = load("measure-fetch.py")

import numpy as np  # noqa: E402
import pikepdf  # noqa: E402
from astropy.io import fits  # noqa: E402


class MeasurementDefinitions(unittest.TestCase):
    """The arithmetic and denominators behind #72's reported aggregates."""

    def test_even_count_median_averages_the_two_middle_values(self):
        rows = [dict(corpus="astropy", status="tier2", exact=True,
                     pct=value, bytes=100, new_size=100 - value)
                for value in (1.0, 2.0, 3.0, 4.0)]
        got = measure_repro.fits_aggregates(rows)["by_corpus"]["astropy"]
        self.assertEqual(got, 2.5)

    def test_pdf_positive_share_uses_accepted_file_denominator(self):
        rows = [dict(corpus="pdfjs", status="accepted", faithful_pct=value,
                     qpdf_pct=value, repack_pct=value)
                for value in (0.25, 0.0, -0.1)]
        result = measure_repro.pdf_aggregates(rows)
        self.assertEqual(result["share_positive_count"], 1)
        self.assertEqual(result["share_positive_denominator"], 3)
        self.assertEqual(result["share_zero_count"], 1)
        self.assertEqual(result["share_negative_count"], 1)
        self.assertAlmostEqual(result["share_positive"], 100.0 / 3.0)

    def test_render_totals_are_recomputed_from_every_selected_record(self):
        rows = [
            dict(rendered=True, compared_pages=2, identical=True),
            dict(rendered=True, compared_pages=4, identical=False),
            dict(rendered=False, render_error="synthetic read error"),
        ]
        self.assertEqual(
            measure_repro.render_aggregates(rows),
            dict(render_files=2, render_pages=6, render_differences=1,
                 render_sampled_files=3, render_errors=1),
        )


class FullShaCorpusSelection(unittest.TestCase):
    """Archive refs are full commit IDs and near-miss files are explicit exclusions."""

    def test_every_corpus_pin_is_a_full_commit_sha(self):
        for corpus in measure_fetch.CORPORA:
            with self.subTest(corpus=corpus["name"]):
                self.assertRegex(corpus["commit"], r"^[0-9a-f]{40}$")
                self.assertRegex(corpus["sha256"], r"^[0-9a-f]{64}$")

    def test_uppercase_suffix_is_recorded_but_not_selected_or_extracted(self):
        with tempfile.TemporaryDirectory(prefix="measure-fetch-") as temporary:
            archive = os.path.join(temporary, "qpdf.tar.gz")
            corpus_root = os.path.join(temporary, "corpus")
            root_name = "qpdf-4eba95899886e851cc41d76886483b347612f2a8"
            selected_name = root_name + "/qpdf/qtest/selected.pdf"
            excluded_name = root_name + "/qpdf/qtest/split-exp-01.Pdf"
            with tarfile.open(archive, "w:gz") as tar:
                for name, payload in (
                    (selected_name, b"selected PDF"),
                    (excluded_name, b"uppercase near miss"),
                ):
                    member = tarfile.TarInfo(name)
                    member.size = len(payload)
                    member.mode = 0o644
                    tar.addfile(member, io.BytesIO(payload))

            entry = dict(
                name="qpdf",
                repo="qpdf/qpdf",
                commit="4eba95899886e851cc41d76886483b347612f2a8",
                sha256="0" * 64,
                bytes=os.path.getsize(archive),
                format="pdf",
                selected="qpdf/qtest",
                suffixes=(".pdf",),
                files=1,
            )
            files, near = measure_fetch.extract(entry, corpus_root, archive)
            self.assertEqual(len(files), 1)
            self.assertEqual(os.path.basename(files[0]), "selected.pdf")
            self.assertEqual(len(near), 1)
            self.assertTrue(near[0].endswith("split-exp-01.Pdf"))
            self.assertFalse(os.path.exists(near[0]), "an excluded file must not enter the measured tree")

            # An existing selected tree must not make the archive's exclusions
            # disappear on a rerun.
            files_again, near_again = measure_fetch.extract(entry, corpus_root, archive)
            self.assertEqual(files_again, files)
            self.assertEqual(near_again, near)

            payload = measure_fetch.manifest(corpus_root, [(entry, files_again, near_again)])
            self.assertEqual(payload["corpora"][0]["files"], 1)
            self.assertEqual(payload["corpora"][0]["excluded"], [
                dict(path="qpdf/qpdf/qtest/split-exp-01.Pdf",
                     why="not selected: the suffix is not lowercase")
            ])
            manifest_path = os.path.join(temporary, "manifest.json")
            with open(manifest_path, "w") as handle:
                json.dump(payload, handle)
            self.assertEqual(measure_fetch.verify(corpus_root, manifest_path), [])


class PdfRowOrdering(unittest.TestCase):
    """Manifest order must reproduce sorted-glob inputs and the fixed sample."""

    def test_manifest_entries_are_sorted_per_corpus(self):
        with tempfile.TemporaryDirectory(prefix="pdf-order-") as temporary:
            manifest_path = os.path.join(temporary, "manifest.json")
            manifest = dict(corpora=[
                dict(name="pdfjs", format="pdf", entries=[
                    dict(path="pdfjs/test/pdfs/z.pdf"),
                    dict(path="pdfjs/test/pdfs/a.pdf"),
                ]),
                dict(name="astropy", format="fits", entries=[dict(path="astropy/a.fits")]),
                dict(name="qpdf", format="pdf", entries=[
                    dict(path="qpdf/qpdf/qtest/z.pdf"),
                    dict(path="qpdf/qpdf/qtest/a.pdf"),
                ]),
            ])
            with open(manifest_path, "w", encoding="utf-8") as handle:
                json.dump(manifest, handle)

            _manifest, named = pdf_measure.from_manifest(manifest_path)
            self.assertEqual(
                [(corpus, path) for corpus, path, _row in named],
                [
                    ("pdfjs", "pdfjs/test/pdfs/a.pdf"),
                    ("pdfjs", "pdfjs/test/pdfs/z.pdf"),
                    ("qpdf", "qpdf/qpdf/qtest/a.pdf"),
                    ("qpdf", "qpdf/qpdf/qtest/z.pdf"),
                ],
            )

    def test_sample_paths_match_the_saved_python_312_output(self):
        records = os.path.join(
            HERE, "..", "..", "handoff", "i74-metric-source-records"
        )
        pdf_rows = measure_repro.load_jsonl(os.path.join(records, "pdf.jsonl"))
        accepted = [row for row in pdf_rows if row.get("status") == "accepted"]
        render_rows = measure_repro.load_jsonl(os.path.join(records, "render-sample.jsonl"))

        for corpus in ("pdfjs", "qpdf", "verapdf"):
            paths = [
                row["manifest_path"]
                for row in pdf_rows
                if row["corpus"] == corpus and row["status"] == "accepted"
            ]
            self.assertEqual(paths, sorted(paths), "%s accepted rows are not path-sorted" % corpus)

        selected = pdf_measure.select_render_sample(accepted, sample_size=140, seed=57)
        self.assertEqual(
            [row["manifest_path"] for row in selected],
            [row["relative_path"] for row in render_rows],
            "the exact sample saved by the Python 3.12.15 run must match this interpreter",
        )
        audit = measure_repro.pdf_path_order_audit(
            pdf_rows, render_rows, root="/tmp/mfetch", render_sample=140
        )
        self.assertTrue(audit["ok"], json.dumps(audit, sort_keys=True))
        self.assertEqual(audit["render_sample_paths_sha256"],
                         "c29f948d20ab97bf401c0b8dcf0e6bf64ea15bc8e27060e43c94a420443b412b")

        reordered = list(pdf_rows)
        reordered[0], reordered[1] = reordered[1], reordered[0]
        bad_audit = measure_repro.pdf_path_order_audit(
            reordered, render_rows, root="/tmp/mfetch", render_sample=140
        )
        self.assertFalse(bad_audit["ok"], "an out-of-order accepted row must fail the audit")


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
