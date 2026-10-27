# Relay draft for issue #72 — not posted

**Posting status:** Per the owner, GitHub rejected issue-comment publication with `Resource not accessible by integration`. This is a relay draft; it was not posted by this run.

---

## Reproducibility update on tested SHA `04b303e`

The completed [`measure-repro` run 40359915023](https://github.com/RUdolf1517/tzim/actions/runs/40359915023) fetched and verified the full-SHA corpus before rerunning the measurements. `measure-fetch.py --verify` reported `0 problem(s)`. FITS reproduced the publication; PDF did not:

- `share_positive`: measured **69.37561942517344% (700/1,009)** vs published **68%**.
- `render_pages`: measured **168** vs published **173**.

The run is red for those PDF comparisons. Neither published value was altered, and neither historical result is declared wrong. The run record and exact output are also summarized in [the PR check comment](https://github.com/RUdolf1517/tzim/pull/73#issuecomment-6421444834). Its evidence bundle digest is `2739270dcb3f6e4500032ee160e4c8c27a5ef80f5a38675308836c38d2d3b2a7` (12 files).

The seven corpora are pinned by full 40-character commit SHAs and archive SHA-256. Archive byte changes are recorded as new baselines; equality is asserted for the selected per-file manifest, not for whole archives. The 2,386 selected-file manifest digest is unchanged at `57e18a9d97d70e4c9e8fd919546c537fea79f9d30e42437c8b4d409fd027ecc5`. See [`fullsha-repin-comparison.md`](../fullsha-repin-comparison.md) and [`fullsha-source-manifest.json`](../fullsha-source-manifest.json).

The accepted PDF rows are path-sorted per corpus, and the exact saved 140-path render sample is reproduced in the saved order. The NUL-delimited ordered-path SHA-256 is `c29f948d20ab97bf401c0b8dcf0e6bf64ea15bc8e27060e43c94a420443b412b`; see [`pdf-order-runtime-audit.md`](../pdf-order-runtime-audit.md) and [`render-sample.jsonl`](../i74-metric-source-records/render-sample.jsonl).

A targeted compatibility comparison covered **6,344 rewritable Flate streams** under zlib 1.2.13 vs 1.3.1: no per-stream compressed-length differences and no `faithful_pct > 0` classification flips. Each library had 700 positive, 309 zero, and zero negative accepted PDFs. This does not explain the render-page discrepancy; the old render-path list was not preserved. Details are in [`zlib-borderline-classification.json`](../zlib-borderline-classification.json) and [`pdf-order-runtime-audit.md`](../pdf-order-runtime-audit.md).

The final Actions artifact/log downloads returned `EOF` in this environment. Existing `handoff/i74-metric-source-records/fits.jsonl` and `pdf.jsonl` are retained source rows from the earlier #72 replay, not the final run's per-file output. The new [`per-file/control record`](PER-FILE-SUMMARY-CONTROL-RECORD.md) labels that distinction and does not fabricate unavailable per-file results. The absence of a representative survey-FITS product and a PDF from the published corpus's producing toolchain remains open; old 8%/15% thresholds are not a new pre-registration.

---
