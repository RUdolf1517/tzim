# Metric definitions and source rows

Old published values are comparison targets, not assumed ground truth. The original audit bundles did not preserve per-file JSONL or a render-sample list, so historical source rows cannot be reconstructed from those rounded table entries alone.

## FITS median by corpus: astropy and reproject

Per-file value: `pct = 100 * (bytes - new_size) / bytes`. Include only `status=tier2` rows with `exact=true`; the median denominator is the number of such rows in the corpus. For an even count, median is the arithmetic mean of the two middle sorted values. Quartiles use the instrument's nearest-rank indices `int(p*n)`.

The earlier re-run used the upper middle element for even counts. That is a different definition, not a change in input data. The source rows in `fits-corpus-median-source.jsonl` include all Tier 2 rows from these two corpora, including failed-verification rows (marked `included_in_median=false`).

## PDF share with a positive faithful recompression

For each accepted PDF, `faithful_pct = 100 * (F - F9) / S`, where `S` is whole-file bytes, `F` is the sum of original encoded bytes of in-scope Flate streams, and `F9` is the sum over those streams of `min(original_encoded_length, zlib_level_9_length(inflated_payload))`. A file is positive exactly when `faithful_pct > 0`. `pdf-share-positive-source.jsonl` contains all accepted rows, with the positive flag; `pdf.jsonl` retains all accepted, already-optimal and refused rows.

## PDF render-page count

Selection: `random.seed(57)` then `random.sample(accepted_rows, min(140, len(accepted_rows)))` in manifest order. Original and allowed-recompression candidate are rendered by PyMuPDF at 100 dpi; at most the first 6 pages per document are compared. `render_pages` is the sum of `compared_pages` for successfully rendered sample files, not the sum of each document's full page count. `render-sample.jsonl` records all selected paths, manifest hashes, original/candidate page totals, compared-page counts, page-image SHA-256 lists and equality result. Render failures are retained and make the run red.

## Current source counts and medians

- `astropy`: n=12; median -7.142857143% (rows 6–7 = -14.285714286% and 0.000000000%).
  - center source: `astropy/astropy/io/fits/tests/data/checksum_false.fits`, 20160 → 23040 bytes, SHA-256 `dd05a0919709f92312d1713c8614c51eb4c58c3feaa6fd3969f2e614d5598436`.
  - center source: `astropy/astropy/io/fits/tests/data/compressed_float_bzero.fits`, 8640 → 8640 bytes, SHA-256 `3c43b44b3f1a8385f70e3ef059d8a4fa70492e034dd0cd2e5733beb9a35e4275`.
- `reproject`: n=16; median 41.883116883% (rows 8–9 = 40.909090909% and 42.857142857%).
  - center source: `reproject/reproject/adaptive/tests/reference/test_reproject_adaptive_2d.fits`, 63360 → 37440 bytes, SHA-256 `f3229be53b13f1500d77d1c499aa3c7240e501624028d55f770fa130d086efca`.
  - center source: `reproject/reproject/interpolation/tests/reference/test_reproject_celestial_2d_gal2equ.fits`, 40320 → 23040 bytes, SHA-256 `0755af6e3c4e2ab40440d339b7c4c72a500eb07c908ee400953a540983c0f46d`.

## Current PDF source counts

- Positive accepted PDFs: 700 / 1009 = 69.375619425%; exactly zero: 309; negative: 0.
- Positive and zero/negative source rows are listed in `pdf-share-positive-source.jsonl` (one record per accepted input).

## Current render sample counts

- Selected: 140; rendered: 140; compared pages: 168; differences: 0; errors: 0.
- Prior aggregate 173 has no preserved old sample path list, so it cannot be asserted to refer to these same 140 documents/pages.
