Manager review at exact PR head `04b303e7f8a410f8b71e86b2517bccae61079036`: **#72 is not accepted; the comparison under-reports discrepancies.**

Evidence boundary: I recomputed arithmetic from the committed `handoff/i74-metric-source-records` rows; this was not a new corpus measurement or recovery of historical output. The committed `aggregates.json`, `fits.jsonl`, `pdf.jsonl`, and `render-sample.jsonl` match their recorded SHA-256 entries.

### 1. The tolerance suppresses a real two-decimal discrepancy

`measure-repro.py` applies `PERCENT_TOLERANCE = 0.051` to every float, despite describing half of the last printed digit. From the 1,009 accepted rows:

- recomputed faithful mean = `0.4216836773137107` (recorded aggregate: `0.4216836773137112`);
- published mean = `0.39` (two decimals);
- delta = `0.03168367731371069`; the measured value rounds to `0.42`.

The report marks this `reproduced` only because the global tolerance is `0.051`. Under the stated rule, a two-decimal value needs an approximately `0.005` rounding interval (plus only a numerical epsilon), so this must be `DIFFERENT`. Use per-metric published precision or an equivalent explicit rounding-interval rule, with a regression case outside a two-decimal interval but inside `0.051`.

### 2. Published table entries are silently omitted

At minimum:

- **qpdf mean:** published `1.68`, absent from `PUBLISHED`/aggregates. The retained rows give `1.6538774233025473`, rounding to `1.65`: another discrepancy under the stated precision rule.
- **qpdf max:** published `96.4`, absent. The rows give `96.39702019960842`, consistent at one decimal; add the explicit comparison.
- **FITS Tier 1 row:** absent. All 151 retained accepted rows have `tier1_pct = 0`, so the currently computable aggregates agree with the published zero row; report that comparison rather than omitting it.
- **zopfli sample row:** not produced by the current instruments. Reproduce it under an explicitly identified sample/protocol, or mark it `not measured`/`unresolved` and request an owner scope decision; do not imply the two scripts covered it.
- Audit the remaining published status/refusal counts for explicit comparison or a stated exclusion. Current rows give PDF `accepted=1009`, `already_optimal=723`, `refused=476`; matching entries should be shown as matches.

After correcting these issues, the retained new run supports **at least four PDF discrepancies**: faithful mean, qpdf mean, positive share, and render pages. Do not change the historical values or force agreement. Keep the measurement run red, preserve all rows, add comparison coverage/precision checks, and attach the corrected report at the exact new SHA. The current conclusion that only two PDF comparisons differ is not accepted.

This comment does not authorize product-documentation edits, issue closure, a measurements Release, or FITS release.
