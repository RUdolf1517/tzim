# Relay draft for issue #71 — not posted

**Posting status:** Per the owner, GitHub rejected issue-comment publication with `Resource not accessible by integration`. This is a relay draft; it was not posted by this run.

---

## FITS HEASARC gate is green on the tested code SHA

PR #73's tested code SHA is [`04b303e7f8a410f8b71e86b2517bccae61079036`](https://github.com/RUdolf1517/tzim/commit/04b303e7f8a410f8b71e86b2517bccae61079036).

- [`ci` run 40359915275](https://github.com/RUdolf1517/tzim/actions/runs/40359915275) completed successfully, including all-feature/default tests on Linux, macOS, and Windows, FITS oracle, Clippy, and rustfmt.
- [`fits-heasarc` run 40359915228](https://github.com/RUdolf1517/tzim/actions/runs/40359915228) completed successfully on the actual `tzim` binary built with the FITS feature.

The gate's successful exit means all 12 fixtures and all three controls passed their assertions. The positive control completed `fpack → funpack -C`, `fitsverify` exited 0, its literal 80-byte-card diff count was zero, and the integer arrays matched. Arrays for `tzim` candidates were compared to the original after `funpack -C`; `fpack` is not used as a floating-point reference. The full per-file assertion table, fixture-input identities, and explicit limitations on unavailable Action output are in [`PER-FILE-SUMMARY-CONTROL-RECORD.md`](PER-FILE-SUMMARY-CONTROL-RECORD.md).

The actual-binary pass-after coverage includes legal random-groups data length and refusal, `SIMPLE=F` refusal, complete Z-form preservation including `ZEXTEND`, and exact decimal/exponent scaling checks that avoid an f64-only lossy transform. The registered positive multi-HDU traversal fixture remains distinct from HDRLET preservation. HDRLET diagnostics are compared per input file: no new candidate diagnostic/count is allowed, and its raw HDU must be preserved; there is no global warning allowlist.

`zero_width_column.fits` has the contract result **unchanged + `already_optimal`**. The gate does not use a discarded candidate size as an external criterion.

The earlier fail-before record is retained at [`handoff/i71-fail-before-run-40350665261.md`](../i71-fail-before-run-40350665261.md); it is not pass-after evidence. F-04 remains revoked; F-06 through F-12 remain open. No issue was closed and no Release was created.

---
