# #72 — measurement reproducibility: updated formula audit and source rows

Branch `arena/7ded2747-tzim`, PR **#73** (open). Latest measurement review is on commit `5c2f4a2`: run [40344219423](https://github.com/RUdolf1517/tzim/actions/runs/40344219423), job [121646706476](https://github.com/RUdolf1517/tzim/actions/runs/40344219423/job/121646706476). CI comment: [6419206909](https://github.com/RUdolf1517/tzim/pull/73#issuecomment-6419206909). Evidence is retained for 90 days.

**Old rounded values are comparison targets, not assumed truth.** The first replay exposed four differences; investigating the definitions resolved the two FITS per-corpus medians as an even-sample median implementation error. Two PDF differences remain red and unresolved.

## Reproducible acquisition and environment

The one acquisition path remains `.github/scripts/measure-fetch.py`: seven codeload archives at full commit SHAs, SHA-256 checked, a per-file size/SHA-256 manifest for **2,386 files** (178 FITS, 2,208 PDF), and `--verify` immediately before measuring. Manifest digest: `57e18a9d97d70e4c9e8fd919546c537fea79f9d30e42437c8b4d409fd027ecc5`.

Those archive checksums are a **new baseline**, not a recovered historical checksum and not an immutability claim. The manifest records the 11 uppercase `.Pdf` exclusions rather than quietly counting them. No PDF from the exact real toolchain sample and no full-size archive survey FITS product were added. The existing 8%/15% thresholds are not re-registered. No Release was created.

The CI environment uses Python 3.12.15 and hashed wheels; package versions include NumPy 2.4.6, Astropy 8.0.2, pikepdf 10.17.0/libqpdf 12.4.2, and PyMuPDF 2.0/MuPDF 1.28.5. Commands, versions, script hashes, OS/runner and checksums are recorded in the evidence bundle.

## Four metrics: definitions, denominators, formulae, and source records

### 1–2. FITS per-corpus medians (`astropy`, `reproject`)

Per-file value: `pct = 100 * (bytes - new_size) / bytes`. The median population is only rows with `status=tier2` **and** `exact=true`; failed-verification rows remain in the source file but are marked `included_in_median=false`. For even `n`, the median is the arithmetic mean of the two central sorted values. Quartiles retain the instrument’s `int(p*n)` index rule.

The first replay used the upper middle element `sorted[int(0.5*n)]` for even `n`; that is not the conventional median and caused the apparent mismatches. A regression test now checks the average-of-middle-values rule.

| Corpus | `n` used | Central source rows | Recomputed median | Published rounded value |
|---|---:|---|---:|---:|
| `astropy` | 12 | `checksum_false.fits`: 20,160→23,040 bytes, −14.285714%; `compressed_float_bzero.fits`: 8,640→8,640 bytes, 0% | **−7.142857%** | −7.1% |
| `reproject` | 16 | `test_reproject_adaptive_2d.fits`: 63,360→37,440 bytes, 40.909091%; `test_reproject_celestial_2d_gal2equ.fits`: 40,320→23,040 bytes, 42.857143% | **41.883117%** | 41.9% |

These current-source medians reproduce the rounded values under the conventional formula. This does **not** establish that the historical run used exactly these rows: no per-file JSONL from the original audit was preserved.

### 3. PDF share with positive faithful recompression

For each accepted PDF:

`faithful_pct = 100 * (F - F9) / S`

where `S` is whole-file bytes; `F` is the sum of original encoded byte lengths of in-scope Flate streams; and `F9` is the sum per stream of `min(original encoded length, zlib level-9 length after inflating the stream)`. A file counts positive iff `faithful_pct > 0`.

Current result: **700 / 1,009 = 69.375619%**, with 309 exactly zero and 0 negative. The old publication says 68%; its original numerator and source rows are unavailable, so we cannot identify which files differ or declare either result correct. This remains a red discrepancy; no formula or threshold was adjusted to force agreement.

### 4. PDF render-page count

The run selects `random.sample(accepted_rows, min(140, n))` after `random.seed(57)`, from manifest order. Original and allowed-recompression candidates are rendered by PyMuPDF at 100 dpi; at most the first six pages per PDF are compared. The reported page metric is the **sum of pages actually compared** (`min(document pages, 6)` for each successful sample row), not the sum of full document lengths.

Current source rows: 140 selected, 140 rendered, **168 compared pages**, zero differences, zero render errors. The old report says 173 pages but preserved no sample-path list, so it cannot be asserted to be the same 140 documents. The 168-vs-173 discrepancy remains red and unresolved; it is not attributed to library version or treated as agreement.

## Machine-readable source records

The current 90-day CI artifact includes and checksums:

- `fits-corpus-median-source.jsonl` — all Tier 2 `astropy`/`reproject` rows, with bytes, new size, percentage, manifest SHA, and inclusion flag;
- `pdf-share-positive-source.jsonl` — all 1,009 accepted PDFs with the per-file numerator inputs, percentage, manifest SHA, and positive flag;
- `render-sample.jsonl` — all 140 selected relative paths and manifest SHAs, page counts, compared-page counts, per-page image SHA-256s, and equality result;
- full `fits.jsonl`, `pdf.jsonl`, `aggregates.json`, `metric-definitions.md`, environment/commands, and `SHA256SUMS`.

CI bundle digest: `8d45bc4134c9c783488d290815a9051e0cfb370a73b022facbbdc47cf6a7cf5a`. I also assembled a downloadable local source-record bundle from the same manifest baseline for this handoff; its file list and checksum are included with the attachment. The CI bundle is authoritative for the CI run’s exact render selection.

## Test and release status

The #72 historical negative controls remain in `test_measure.py`; three additional tests guard even-count medians, share numerator/denominator, and per-file render aggregation. **10 tests pass** locally. The CI measurement job is red only on `share_positive` and `render_pages`; the FITS corpus aggregates reproduce under the corrected median definition. This does not make the old publication ground truth.

No product documentation changed, no issues were closed, no criteria were weakened, and no GitHub Release was created.
