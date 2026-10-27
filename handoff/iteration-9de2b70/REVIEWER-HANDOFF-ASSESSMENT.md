# Manager review of the independent FITS handoff at `9de2b70`

Date: 2026-10-27. Review target: `9de2b7069a522ab5ce05e8255a3ce4c530f6190f`.

## Verdict

The reviewer handoff is **accepted as a verifiable evidence package for further review**. It is not a
manager acceptance of new product code, not actual-binary acceptance of the FITS writer, and not FITS
release approval.

The handoff was transferred as commit `eeebd350317396e72491b451efd18683fd45ae1e` on
`arena/cf9e3b68-tzim`. Its parent is the earlier review handoff `ddff3a91f21b7cacfb16a72517490e60c406b4a6`.
The commit adds only `handoff/fits-review-9de2b70/`; it does not modify PR #73.

## Integrity checks performed by the manager

The manager fetched the handoff commit and inspected the transferred files without changing the product
worktree:

- `README.md`: SHA-256 `4fa3627d602050b4a66a195b8c67288a1591865dc6b652f69a8e8dcacc349b80`.
- `REVIEW-9de2b70.md`: SHA-256 `9b607dc5b5f33d207f0a9fecc51362cd4345f3425ad56b25a1a0daf323f35a05`.
- `fits-verify-9de2b70-bundle.tar.gz`: SHA-256
  `d4287857f14cf01af0acd10069096b7accc12d677684da372e96005fb3f85853`.
- The archive has no absolute or parent-traversal paths and no link payload entries. Its `SHA256SUMS`
  verifies **429/429 entries, 0 FAILED**. The manifest has 429 unique paths and covers every payload file.
- `baseline.json` records 61 tool runs and 26 card mutants. Twenty-three mutants have a negative verdict;
  three have no verdict because the gate raised from `parse`.
- The four extracted source blobs in `05-extracted-at-sha/` match the blobs fetched independently from
  `9de2b7069a522ab5ce05e8255a3ce4c530f6190f` byte-for-byte by SHA-256.

These checks establish package integrity and internal consistency. They do **not** constitute a rerun of
CFITSIO, the oracle, the gate, or tzim by the manager.

## Finding disposition

| Item | Manager disposition |
| --- | --- |
| F-06 | `not run`: arithmetic/source evidence only; no file over 2 GB and no tzim binary. |
| F-07 | `not run`: source/test inventory only; no Rust test execution. |
| F-08 | `not run` for `tzim::check_tiled`: source review supports added checks for `ZSIMPLE`/`ZTENSION`, `ZEXTEND`, and source-present `EXTNAME`; the four original residual groups remain. The `ZVAL1` mutation is reproduced at the external gate level, not against the shipped binary. Generated-`EXTNAME` and other extra O-2 observations remain separate triage. |
| F-10 | **Confirmed at the `fits-oracle.py` script level**: NaN-payload and signed-zero mutations produce zero oracle problems while the HEASARC gate's byte comparison sees them. This is not a new writer defect finding. |
| F-11 | **Confirmed at the oracle/policy level**: the script emits the `GZIP_1` float policy diagnostic and has no self-check. This does not make legal `GZIP_1` a universal convention violation. |
| O-1 | **Reproduced at the gate level**: three malformed candidates produce an uncaught `ValueError` traceback. The gate still fails closed, but diagnostic handling is in the authorised coder scope. |
| O-2 | Triage tied to the residual F-08 work; it is not blanket approval for every additional generated-card check listed in the report. |
| O-3/O-4/O-5 | Triage notes only. No automatic defect acceptance or code change is authorised. O-4 is limited to the recorded tool invocations and must not become a universal `fpack`/`imcopy` claim. |

## Scope guards for the current coder iteration

1. The convention quotation used for F-06 says **“about 2.1 GB”**. An approximate prose value must not be
   turned into an allegedly exact implementation boundary without a contract decision. F-06 behaviour
   changes wait for that decision; arithmetic and clearly labelled options may be supplied as evidence.
2. The current F-08 scope remains the four original residual groups: `TFORM1`, `ZNAME1`/`ZVAL1`, absence
   of `ZSCALE`/`ZZERO`, and `THEAP`/`GCOUNT`. Additional generated `EXTNAME` and table-structural-card
   observations in O-2 remain triage unless separately dispositioned; they must not silently expand the
   current assignment.
3. The already authorised O-1 fix must produce a controlled, recorded rejection while preserving
   fail-closed behaviour and evidence. A traceback is not an acceptable substitute for a verdict.
4. No historical measurement, baseline, threshold, PDF/FLAC, product-documentation, issue-closure, or
   release work is added by this handoff.

## Sequencing

At the last GitHub check, PR #73 remained open at `9de2b7069a522ab5ce05e8255a3ce4c530f6190f`.
The earlier manager acceptance remains specific to that SHA. The coder may continue the authorised work in
that PR; any new HEAD requires a fresh diff review and actual-binary evidence. The reviewer should wait for
that HEAD and then review the agreed delta, retaining the distinction between binary execution, layout
transcription, source reading, and external-tool execution.

## Validation-push decision

The owner subsequently requested visible progress rather than waiting for a local Rust/HEASARC toolchain.
The manager therefore authorises a **validation commit and push to the existing PR #73 branch**. This
overrides only the earlier local-results-before-push restriction; it is not acceptance of the uncommitted
work and not authorisation to create a release.

The coder must report the new exact SHA and CI run URLs. CI must supply the full Rust validation and actual
FITS-enabled/HEASARC evidence, including the three O-1 mutants (`PCOUNT`, `BITPIX`, `GCOUNT`) or an explicit
explanation that the current three regression cases are different. A green push is not release approval;
the reviewer must still inspect the new diff and evidence. Historical measurements, baselines, thresholds,
product documentation, issues, and releases remain outside this validation push.
