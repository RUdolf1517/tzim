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
