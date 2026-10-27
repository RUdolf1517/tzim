# Measurement re-run

Run at 2026-10-26T14:30:18Z, new measurement, not the published one.

Manifest: `/tmp/mfetch/manifest.json`, 2386 files, digest `57e18a9d97d70e4c9e8fd919546c537fea79f9d30e42437c8b4d409fd027ecc5`.

## FITS — published in docs/fits-audit.md §8.2

| metric | published | measured | verdict |
| --- | --- | --- | --- |
| `files` | 178 | 178 | reproduced |
| `tier2` | 153 | 153 | reproduced |
| `accepted` | 151 | 151 | reproduced |
| `median` | 21.9 | 21.875 | reproduced |
| `q1` | 17.4 | 17.3913 | reproduced |
| `q3` | 27.3 | 27.2727 | reproduced |
| `mean` | 19.1 | 19.0911 | reproduced |
| `share_positive` | 83 | 82.7815 | reproduced |
| `maximum` | 72.7 | 72.7273 | reproduced |
| `bytes_before` | 17827200 | 17827200 | reproduced |
| `bytes_after` | 13343040 | 13343040 | reproduced |
| `median[reproject]` | 41.9 | 41.8831 | reproduced |
| `median[cfitsio]` | 42.1 | 42.1053 | reproduced |
| `median[drizzlepac]` | 21.9 | 21.875 | reproduced |
| `median[astropy]` | -7.1 | -7.1429 | reproduced |

Refusals, by reason: `no_image` 25.

Files the instrument rewrote and could not verify:

* `double_ext.fits` (`astropy`, `keyword_EXTNAMX`)
* `zerowidth.fits` (`astropy`, `array_0`)

## PDF — published in docs/pdf-audit.md §9.2

| metric | published | measured | verdict |
| --- | --- | --- | --- |
| `files` | 2208 | 2208 | reproduced |
| `accepted` | 1009 | 1009 | reproduced |
| `median` | 0.08 | 0.082 | reproduced |
| `q1` | 0.0 | 0.0 | reproduced |
| `q3` | 0.09 | 0.0856 | reproduced |
| `mean` | 0.39 | 0.4217 | reproduced |
| `share_positive` | 68 | 69.3756 | DIFFERENT |
| `maximum` | 29.7 | 29.6864 | reproduced |
| `qpdf_median` | 0.59 | 0.591 | reproduced |
| `qpdf_q1` | -0.07 | -0.0681 | reproduced |
| `qpdf_q3` | 1.94 | 1.9369 | reproduced |
| `qpdf_share_positive` | 60 | 60.2577 | reproduced |
| `repack_median` | 20.1 | 20.1409 | reproduced |
| `repack_mean` | 17.0 | 16.9857 | reproduced |
| `qpdf_check_passed` | 1009 | 1009 | reproduced |
| `render_files` | 140 | 140 | reproduced |
| `render_pages` | 173 | 168 | DIFFERENT |
| `render_differences` | 0 | 0 | reproduced |

Disagreements, kept rather than smoothed:

* share_positive: published 68, measured 69.37561942517344
* render_pages: published 173, measured 168

Refusals, by reason: `incremental_update` 332, `damaged` 40, `unreadable` 40, `encrypted` 21, `filter_chain` 15, `no_header` 15, `not_inflatable` 4, `signed` 3, `truncated` 2, `stream_too_short` 2, `not_deflate` 2.

## Source-row definitions for the disputed metrics

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

## What this does not prove

The archive hashes above are the first recorded baseline for those codeload URLs; the run verifies that the bytes match *that* baseline and nothing about what the original audits downloaded. The corpus is still the four FITS test-data sets and three PDF corpora — no archive survey FITS product and no PDF from the toolchain that produced the published PDFs — so the open gaps in §8.1 and §9.1 are unchanged, and the eight-per-cent and fifteen-per-cent figures are the old thresholds reported, not a new pre-registration.
