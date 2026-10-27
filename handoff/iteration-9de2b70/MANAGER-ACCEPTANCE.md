# Final acceptance of #72 — PR #73 at `9de2b706`

Date: 2026-10-26. Review target: `9de2b7069a522ab5ce05e8255a3ce4c530f6190f`.

## Verdict

**#72 is accepted as the assigned reproducible-runner, retained-evidence, and publication-comparison task.** This is not a declaration that the historical measurements reproduced, not owner resolution of the exposed discrepancies, and not FITS release approval.

#71 and #74 remain accepted in their assigned scopes and are closed by the owner. F-04 remains retracted; F-06 through F-12 remain open.

## Exact evidence checked

- The diff from `4a90ade` to `9de2b706` changes only `.github/workflows/measure-repro.yml`: the `ci-report.sh` invocation is now one physical shell line. Product code, Rust tests, comparison scripts, product documentation, README, and changelog trees are unchanged.
- Measurement run `40392951153` has the expected red comparison step. `Seal the evidence`, `Summary`, **`Publish failure log`**, and artifact upload all succeed. CI `40392950853` and HEASARC `40392951490` pass at the same HEAD.
- CI comment `6426183078` was authored by `github-actions[bot]` and contains the complete generated report. Its marker names PR merge SHA `6eb962e55f79e7473359d1ffd0a9c3592d650502`; that merge commit's parents are baseline `5dd45b38412ffb762a1fc380bd291351c9e6601e` and the reviewed HEAD `9de2b7069a522ab5ce05e8255a3ce4c530f6190f`, so the report is not stale or from another head.
- Artifact `12685261129` (`measure-repro-evidence`, 413,197 bytes) exists and is unexpired. Its ZIP contents were not downloaded; acceptance of the report relay is based on the complete PR comment and successful job steps, not a claim of artifact inspection.

## Published report content independently counted

The complete report contains exactly 14 non-`reproduced` table rows:

1. Five FITS input-byte `DIFFERENT` rows: Astropy `780 → 675 KiB`, CFITSIO `60 → 678 KiB`, DrizzlePac `3.3 → 14.8 MiB`, reproject `2.0 → 1.8 MiB`, total `17.8 → 17.9 MiB`.
2. Four PDF result-table `DIFFERENT` rows: faithful mean `0.39 → 0.42`, positive share `68 → 69`, qpdf mean `1.68 → 1.65`, render pages `173 → 168`.
3. Five Zopfli `unavailable` rows: sample size, median, Q1, Q3, and positive share. The report explicitly says the historical 150-file path list and per-file rows were not retained.

The report also contains manifest digest `57e18a9d97d70e4c9e8fd919546c537fea79f9d30e42437c8b4d409fd027ecc5`, render path-sequence digest `c29f948d20ab97bf401c0b8dcf0e6bf64ea15bc8e27060e43c94a420443b412b`, 140 selected/rendered files, 168 compared pages, zero differences/errors, and the strict `faithful_pct > 0` boundary. Full commit identities display as reproduced against their published prefixes. No baseline or threshold was changed.

## Remaining decisions, not hidden by acceptance

- Review the four PDF result-table differences and five FITS input-byte differences; do not rewrite history merely to make the job green.
- Decide whether the unavailable historical Zopfli sample may remain an explicit limitation or requires a separately identified new measurement.
- The missing representative survey-FITS and source-toolchain-PDF samples remain open protocol gaps.
- F-06…F-12 and other owner decisions remain open. **FITS release remains blocked** despite acceptance of #71, #72, and #74.
