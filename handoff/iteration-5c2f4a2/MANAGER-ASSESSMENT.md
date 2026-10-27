# Manager assessment of coder iteration `5c2f4a2`

Date: 2026-10-26.

## Evidence inspected

- PR #73 is open at `5c2f4a2899e852048b4c5278a0f8d3e422ba4ea4` (14 commits). The manager fetched that head
  without switching branches and inspected the diff against `5dd45b3`.
- [FITS run 40344219155](https://github.com/RUdolf1517/tzim/actions/runs/40344219155) and
  [measurement run 40344219423](https://github.com/RUdolf1517/tzim/actions/runs/40344219423) both completed
  with `failure` at that exact SHA.
- PR diagnostic comments `6419187197` (FITS) and `6419206909` (measurement) are readable. The two artifacts
  exist and are unexpired (`12660971756`, `12659004975`), but this environment's download failed when GitHub
  redirected to its artifact-storage host. Their contents were not inspected.
- The coder's `all-reports.md` and `i74-handoff-package.zip` attachments are not present in this workspace.
  The supplied ZIP hash cannot be checked until the file is transferred through an accessible channel.
- The PR changes ten CI/measurement files. **It does not change `src/formats/fits.rs`, `src/detect.rs` or
  `.github/scripts/fits-oracle.py`.** No manager code or CI edits were made.

## Acceptance decision

| Item | Decision | Reason |
| --- | --- | --- |
| #71 HEASARC gate | **Not accepted** | Its explicit acceptance requires a real green run. The current run is red, and the harness itself still makes a clean header result impossible. |
| #74 actual-binary review follow-up | **Not accepted** | F-01/F-03/F-05 were investigated at most; the writer and refusal logic are unchanged. No requested failing-before/passing-after writer regressions are present. F-02 also remains open from the full review. |
| #72 measurement reproducibility | **Partial implementation; acceptance held** | Runner, manifest, retained-row design, median correction and negative controls are useful. Full-SHA provenance, accessible evidence and disposition of two PDF discrepancies remain. A red comparison is a truthful measurement outcome and must not be forced green. |
| Zero-width table column | **Contract question resolved** | `already_optimal` with an unchanged original is valid: a mixed candidate may be built and then discarded because the whole file is not strictly smaller. The gate need not expose that discarded candidate's size. |
| Release / PR recommendation | **Blocked / not recommended** | No release authorisation, issue closure or merge recommendation follows from this iteration. |

## Definite FITS harness findings at this SHA

### 1. `funpack -C` is still missing

`fits-heasarc.py:647` invokes `funpack` without `-C`. The run's own diffs show the resulting added
`CHECKSUM`/`DATASUM`, and `check_fixture` turns every literal diff into a failure (`:1097-1099`). The positive
control at `:1454-1475` checks only `fpack` followed by `fitsverify`; it never performs the required
`fpack -> funpack -C -> literal original-header comparison`.

Therefore this red run does not establish whether the writer satisfies the clean-header gate. Use `-C`, keep
the full 80-column comparison, and make the reference header round trip green before attributing a header
failure to tzim. Do not compare float arrays against `fpack` output.

### 2. The multi-HDU gate mixes two different questions

The original fixture already gets `fitsverify` exit 1 for unregistered `HDRLET`; the contract requires that
HDU to be copied verbatim. A valid-candidate gate requiring zero `fitsverify` findings cannot use that input
without a separately approved inherited-diagnostic policy.

The candidate also has a **new** traversal failure relative to the original: the original tool run traverses
all four HDUs, while the candidate reads the copied HDRLET payload as a later header and `funpack` exits 207.
That is evidence of an output/tool incompatibility, but not yet an isolated writer root cause.

Split the checks:

- a standards-valid multi-HDU positive fixture with registered extensions, for which the original itself is
  clean, must exercise the release gate;
- an unknown-extension preservation case must prove byte-for-byte copying and no **new** external-tool
  failure without introducing a global warning/error allowlist.

Do not delete HDRLET coverage and do not call its inherited diagnostic a writer regression.

### 3. Zero-width expectation

Change the external-gate expectation from `undecided` to unchanged/`already_optimal`. This is not a waiver of
array/header preservation and does not assert a whole-file zero-width refusal. Internal candidate-size
instrumentation is optional unit-level evidence, not a release-gate prerequisite.

## Writer work still required by the existing findings

The following must be reproduced on the actual binary and, where confirmed, fixed with
failing-before/passing-after regressions:

1. **F-01:** legal random groups (`NAXIS1=0`) must be refused/skipped with exit 0 and an unchanged original,
   not mis-scanned as a later header. Include following-HDU layout and arithmetic bounds.
2. **F-02:** `SIMPLE=F` must follow the documented refusal rather than being classified solely by card
   presence.
3. **F-03:** preserve required original cards/comments through their `Z` forms, including `ZEXTEND`; actual
   `tzim -> funpack -C` must produce a clean full-card diff without normalisation.
4. **F-05:** non-identity `BSCALE`/`BZERO` must not bypass refusal through decimal/exponent spellings. Identity
   spellings may be accepted, but a lossy binary64-only parse must not decide large/range-sensitive values.
5. **Multi-HDU:** isolate the new traversal failure on the valid positive fixture before assigning its cause.

F-04 remains retracted. F-06…F-12 are not closed by this iteration and require later disposition; do not
claim the full independent review has been addressed.

## Measurement findings

### Full revisions are not full revisions

All seven `CORPORA` entries in `measure-fetch.py`, and `CFITSIO_COMMIT` in the FITS workflow, use 12-character
abbreviations despite the prose saying full commit SHA. GitHub resolves them to:

| Corpus | Full commit |
| --- | --- |
| astropy | `dbbf2acd6dbb177db184bff319d2a2950dd178bb` |
| cfitsio | `113781d8aa442cf8d3c27295d9084674b8b039f2` |
| drizzlepac | `635834581ea2b226a3f3028b6ac54efb5e180dfc` |
| reproject | `9dddd419111a9ca159baaf7d5e2eb5729b447b58` |
| pdf.js | `606bd6a4e138bbbc22fad10872d2ffa93365b6b1` |
| qpdf | `4eba95899886e851cc41d76886483b347612f2a8` |
| veraPDF | `bb75f4f0073d9350dfd058c0162a367e6fadf25e` |

Use these full values. If changing the codeload ref changes archive bytes, record a new archive baseline and
prove the per-file manifest unchanged; do not assume the old archive hash transfers.

### FITS medians

The first replay used the upper middle element for even counts. The corrected arithmetic median yields
`-7.142857%` for astropy and `41.883117%` for reproject on current rows, consistent with the published rounded
values. This resolves the replay-formula discrepancy, not the identity of lost historical rows.

### PDF share and render pages

Current results are `700/1009 = 69.375619%` and 168 compared pages with zero differences. They are **new-run
facts**, not proof that historical 68% or 173 was wrong. No per-file historical evidence exists to declare the
old values corrected.

The current script records compile/runtime zlib versions. CI uses Python 3.12 while the historical handover
claims 3.11.2; this must be disclosed, but Python version alone is not an established explanation. CPython
3.11/3.12 source uses the same `random.sample` algorithm for this call. Before final documentation:

- show that accepted rows are in the same per-corpus sorted-path order as the original instrument;
- compare the exact selected render paths, not just their count;
- test/report whether zlib version changes borderline `faithful_pct > 0` classifications;
- retain all current rows and discrepancies without editing the historical baseline to obtain a green run.

## One ordered follow-up iteration

1. Transfer the already-produced reports/source-row package through Git objects or another accessible
   channel; do not rerun merely to replace missing attachments.
2. Correct the harness issues (`-C`, positive reference header control, zero-width expectation, full SHAs).
3. Isolate multi-HDU on a valid fixture and implement the confirmed F-01/F-02/F-03/F-05 writer corrections
   with actual-binary regressions.
4. Complete the PDF order/zlib sensitivity analysis and preserve the old and new result identities.
5. Supply new run URLs and accessible evidence for manager review. Only then schedule the independent
   post-fix FITS review on the new SHA.

This is one sequential follow-up to the existing #71/#72/#74 work, not parallel assignments. Product
documentation remains with the manager; no Release or issue closure is authorised.
