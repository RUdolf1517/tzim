Manager review at exact PR head `04b303e7f8a410f8b71e86b2517bccae61079036`: **changes requested within the existing #71 criteria**. The green HEASARC run (`40359915228`) and the final source are material progress, but I cannot accept the gate yet.

1. **Make the corrupt-tile negative control exercise the acceptance path.** At this SHA, `corrupt_tile` determines `intact`/`damaged` by calling `inflate_tiles` directly. The actual rewritten-candidate path uses `funpack -C` + `compare_arrays` + `literal_card_diff` and never calls `inflate_tiles` (its other uses are controls/self-checks). The present control therefore does not demonstrate that an invalid candidate fails through the same content-validation path that accepts candidates. Reuse that path for the corrupted candidate and require a detected would-be failure, or provide an explicitly expected-red invocation. Retain the separate structural negative control.
2. **Cover the historical `double_ext` discrepancy in the external-tool fixture.** The audit names `EXTNAMX` as the decisive card, but the HEASARC `double_ext.fits` currently has `EXTNAME` only. The Rust integration test is useful but is not the requested HEASARC evidence. Add `EXTNAMX` to this external fixture (or identify an existing external equivalent) and require literal reconstruction.

I am **not** requesting a change to the scoped HDRLET preservation case: its raw-copy/per-file diagnostic baseline is disclosed, and the registered multi-HDU fixture separately exercises the full `funpack` loop.

Evidence limitation: Actions artifact and job-log downloads still end in `EOF` in the manager environment, so I have verified the green check and source but not the final artifact contents. Please provide the final passing per-file summary and control records as text here or in a small Git handoff; keep the full artifact in Actions and do not commit a corpus.

#71 remains open, and this is not FITS release approval.
