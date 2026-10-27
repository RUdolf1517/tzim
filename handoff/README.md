# Existing handover evidence (captured 2026-10-26)

These are the reports and metric source rows already produced for #70–#74. They
are committed as Git objects so the handover is available from the PR without
relying on the runner's artifact-storage host or a local `/home/user/handoff`
path. No measurement was rerun to create this copy.

`i74-handoff-package.zip` contains the four individual reports, `all-reports.md`,
and the same `i74-metric-source-records/` bundle. Its original SHA-256 is recorded
in `i74-handoff-package.SHA256SUMS`; the report and source-record checksums are
in the source bundle's `SHA256SUMS`.

The records include both local render-sample rows and an explicit note that the
CI run's render-sample artifact is authoritative for that run. The original CI
artifact is not being represented as available in this Git copy.

## Follow-up evidence

The current PR #73 follow-up adds `fullsha-repin-comparison.md` and
`fullsha-source-manifest.json` (2,386 selected files, same per-file manifest
digest), `pdf-order-runtime-audit.md` and `pdf-order-verification-previous.json`
(the exact saved render paths, sorted accepted-row order, and Python 3.11/3.12
sample comparison), and `zlib-borderline-classification.json` (6,344 stream
comparisons across zlib 1.2.13 and 1.3.1). The #71 fail-before output and the
latest failed HEASARC diagnostics are retained as `i71-fail-before-run-40350665261.md`
and `i71-fits-heasarc-run-40350674612.md`; they are explicitly not pass-after
results. The successful follow-up run and relay package for tested code SHA
`04b303e7f8a410f8b71e86b2517bccae61079036` are indexed in
`iteration-04b303/`: per-file/control assertions, and draft issue #71/#72 relay
texts. The per-file record explicitly distinguishes run assertions from output
fields unavailable because the final Actions artifact/log endpoints returned
`EOF`.
