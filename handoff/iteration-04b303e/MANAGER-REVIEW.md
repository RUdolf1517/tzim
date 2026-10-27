# Manager acceptance review — PR #73 at `04b303e7`

Date: 2026-10-26. Review target: `04b303e7f8a410f8b71e86b2517bccae61079036`.

## Evidence boundary

This review used Git source at the exact PR head, GitHub run/check status, the retained failing-before records, and the small measurement source-row bundle transferred through Git. The manager did not run the Rust project, the corpus measurement, or the HEASARC tools. External Actions artifact and job-log downloads still end in `EOF`; final artifact contents have not been inspected.

Verified run status at this SHA: PR CI `40359915275` and push CI `40359906288` pass; HEASARC gate `40359915228` passes; measurement run `40359915023` fails. A green check is evidence that its steps exited successfully, not a substitute for reviewing the comparison logic.

For the measurement findings below, the manager recomputed aggregates from the committed `handoff/i74-metric-source-records` rows. This was arithmetic verification of a retained new run, not a new corpus measurement and not recovery of historical output. The committed `aggregates.json`, `fits.jsonl`, `pdf.jsonl`, and `render-sample.jsonl` all match their recorded SHA-256 entries.

## What is supported

- The final writer diff addresses the reviewed random-groups length calculation, `SIMPLE=F` refusal, exact non-identity scaling detection, and literal structural-card/Z-card preservation. The earlier actual-binary run records four concrete failures; the exact final CI is green.
- The HEASARC gate uses actual `fitsverify`, `funpack -C`, and an `fpack` positive control. Its normal registered-image path compares literal complete 80-column cards through END and byte-exact arrays after `funpack`.
- The scoped HDRLET case is acceptable as a preservation-only case: exact non-image-HDU copying and the per-file inherited-diagnostic baseline are disclosed, while registered multi-HDU traversal separately exercises the complete `funpack` loop. No change to that scope is requested here.
- The measurement run retains its new-run identity and currently reports the positive-share and render-page discrepancies rather than editing the historical publication.

These points do not close #71, #72, or #74 and do not authorize a FITS release.

## #71 — changes requested within the existing gate criteria

1. **The corrupt-tile negative control does not exercise the candidate-acceptance path.** In `fits-heasarc.py`, the control computes `intact`/`damaged` by calling `inflate_tiles` directly. The actual rewritten-candidate path instead uses `funpack -C`, `compare_arrays`, and `literal_card_diff`; it does not call `inflate_tiles`. Thus the control can stay green even if the main content-comparison path is broken or disconnected. Exercise the corrupted candidate through the same validation routine used for acceptance and require a detected would-be failure (or an explicitly expected-red invocation). Keep the separate structural negative control.
2. **The external `double_ext` fixture omits the historical decisive card.** The historical measurement identifies `EXTNAMX` as the reason for `double_ext.fits`; the HEASARC fixture currently contains `EXTNAME` cards only. The Rust integration test covers `EXTNAMX`, but #71 requests actual-tool evidence for the historical case or a readable equivalent. Add `EXTNAMX` to the external fixture, or identify an existing external equivalent, and require its literal reconstruction.

Because the manager cannot retrieve the final Actions artifact/logs, provide the final passing per-file summary and control records as text in the issue or a small Git handoff. Keep the complete evidence artifact in Actions; do not commit a corpus.

## #72 — comparison is incomplete and one discrepancy is suppressed

### Precision bug

`measure-repro.py` applies `PERCENT_TOLERANCE = 0.051` to every floating-point published value, although the comment promises half of the last printed digit. That tolerance suits a one-decimal percentage, not a two-decimal percentage.

The retained rows contain 1,009 accepted PDFs. Recomputation gives:

- faithful mean: `0.4216836773137107` (recorded aggregate `0.4216836773137112`);
- published mean: `0.39` with two decimals;
- difference: `0.03168367731371069`; measured value rounds to `0.42`, not `0.39`.

The current comparison marks this `reproduced` because `0.03168 < 0.051`. With the stated precision rule, the two-decimal tolerance is approximately `0.005` (plus only a numerical epsilon), so this is another discrepancy. Use per-metric precision or an equivalent explicit rounding-interval rule. Add a regression case that lies outside a two-decimal rounding interval but inside `0.051`.

### Published entries silently omitted

The current `PUBLISHED` map and aggregates do not compare the complete published tables:

- PDF qpdf mean is published as `1.68` but is absent. The retained rows give `1.6538774233025473`, which rounds to `1.65`; this is another discrepancy under the stated precision rule.
- PDF qpdf maximum is published as `96.4` but is absent. The rows give `96.39702019960842`, which is consistent at one decimal.
- The published FITS Tier 1 row is absent. All 151 retained accepted rows have `tier1_pct = 0`, so the currently computable Tier 1 aggregates are consistent with the published zero row; they still need an explicit comparison rather than silence.
- The separately published zopfli sample row is not produced by either current instrument. Reproduce it under an explicitly identified sample/protocol, or mark it `not measured`/`unresolved` and request an owner scope decision. Do not silently imply that the two scripts covered it.
- Audit the remaining published status/refusal counts for explicit comparison or a stated exclusion. Current source rows give PDF `accepted=1009`, `already_optimal=723`, and `refused=476`; matching values should be shown as matches, not omitted.

Consequently, the retained new run supports **at least four** PDF discrepancies after correcting the comparison: faithful mean, qpdf mean, positive share, and render pages. This is not a request to alter historical values or force agreement. Keep the run red, preserve all rows, update the comparison/report, and attach evidence for the exact corrected SHA. The final claim that only two PDF comparisons differ is not accepted.

## Disposition

- #71: changes requested; not accepted.
- #72: changes requested; not accepted.
- #74: writer fixes are promising but final acceptance remains held with the gate/evidence review; no separate release decision.
- FITS release: still blocked. F-06 through F-12 remain outside this iteration's closed findings; F-04 remains retracted.
- No product-documentation correction, issue closure, measurements Release, or publication is authorized by this review.
