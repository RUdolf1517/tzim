# PDF input order, render sample, and zlib compatibility audit

This is a targeted analysis of the saved #72 source rows, not a full FITS/PDF measurement rerun. The #72 report and source records remain in `handoff/i74-metric-source-records/`; the exact 140 render paths and page digests are in `render-sample.jsonl`.

## Accepted-row order and render sample

The original no-manifest PDF instrument traversed `sorted(glob.glob(...))` separately for each corpus, in corpus order `pdfjs`, `qpdf`, `verapdf`. The saved #72 replay's `pdf.jsonl` has the same per-corpus sorted-path order, and accepted rows are an order-preserving subset:

| Corpus | Input rows | Accepted rows | Input paths sorted | Accepted paths sorted |
|---|---:|---:|---|---|
| `pdfjs` | 1,009 | 343 | yes | yes |
| `qpdf` | 630 | 124 | yes | yes |
| `verapdf` | 569 | 542 | yes | yes |
| **Total** | **2,208** | **1,009** | | |

I applied `random.seed(57)` and `random.sample(accepted_rows, 140)` to the 1,009 accepted rows from the saved `pdf.jsonl` using local Python **3.11.2**. The resulting 140 paths match all 140 `relative_path` values, in order, from the saved render sample produced by the prior CI environment **Python 3.12.15**. The ordered path-sequence SHA-256 (NUL-delimited paths) is `c29f948d20ab97bf401c0b8dcf0e6bf64ea15bc8e27060e43c94a420443b412b`.

Regression coverage now explicitly sorts manifest entries per corpus, asserts the accepted rows remain path-sorted, and compares the seeded sample with the saved Python 3.12 sample paths. `measure-repro.py` also writes a per-run `pdf-order-verification.json` with path counts, boundaries, sequence digests, corpus order, and exact sample-order comparison. The accepted source JSONL records global JSONL order, per-corpus path order, and accepted-row order.

This establishes that Python 3.11 and 3.12 select the same 140 files for this exact ordered accepted-row list. It does **not** establish that the original publication's 173-page count used this sample: the old render path list was never preserved. The current replay's 168 compared pages and old published 173 remain a real, unresolved discrepancy; this result does not explain it away.

Source identities:

- Saved replay `pdf.jsonl` SHA-256: `a1f71a5a216e089726af58c3358059a2b1038550066b48e1a907aad3f5a4138d`.
- Saved exact render paths and per-page records: `handoff/i74-metric-source-records/render-sample.jsonl`.
- Full-SHA selected-file manifest digest: `57e18a9d97d70e4c9e8fd919546c537fea79f9d30e42437c8b4d409fd027ecc5` (same per-file digest as the earlier replay manifest).

## zlib effect on `faithful_pct > 0`

The saved measurement environment records zlib **1.2.13**. I re-opened only its 1,009 accepted PDF rows and recompressed their in-scope Flate stream payloads at level 9 with both:

- system `libz` 1.2.13, matching Python 3.11.2's `zlib.compress(payload, 9)`; and
- a separately built zlib 1.3.1 shared library, called through `compress2(level=9)`.

The comparison covered **6,344 rewritable streams**. For every stream, the two compressed output lengths were identical; consequently all per-file `F9` totals and signs were identical. There were **0** files with a changed `faithful_pct > 0` classification:

| | zlib 1.2.13 | zlib 1.3.1 |
|---|---:|---:|
| Positive accepted PDFs | 700 / 1,009 | 700 / 1,009 |
| Exactly zero | 309 | 309 |
| Negative | 0 | 0 |
| Per-stream compressed-length differences | — | 0 / 6,344 |
| Classification flips | — | 0 |

This specifically tests the borderline condition: 16 accepted PDFs have a one-byte total gain under the saved definition, and all 16 remain one-byte-positive under both zlib versions. The smallest positive gain remains 1 byte. The maximum absolute per-file gain difference between the two libraries was 0 bytes.

The JSON record `handoff/zlib-borderline-classification.json` carries all 1,009 per-file comparisons, the source JSONL hash, stream totals, zlib/Python/pikepdf/libqpdf versions, and the zlib 1.3.1 source-archive SHA-256 (`17e88863f3600672ab49182f217281b6fc4d3c762bde361935e436a95214d05c`). This targeted compatibility result does not rerun qpdf, rendering, corpus acquisition, or the full PDF measurement, and does not prove compatibility with zlib versions other than 1.2.13 and 1.3.1.
