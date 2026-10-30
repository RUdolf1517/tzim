# PDF/FITS measurement acceptance status

Historical checklist date: 2026-10-27. Baseline: `5dd45b38412ffb762a1fc380bd291351c9e6601e`.
This is a living management checklist, not a completed execution audit or a release approval.

## PR #84 execution status — 2026-10-30

The owner-fixed #57/#59 source identities and protocol are in PR comment
[6498702204](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6498702204). The fixed-manifest workflows
retain every row and do not select, drop, repair, or substitute inputs based on size or gain. The first complete
measurement pass ran at exact source SHA `7b82c9edb6e5060bc8ce7ed756caebba91505b5b` in
[workflow `41038741167`](https://github.com/RUdolf1517/tzim/actions/runs/41038741167). These results are exact
for that SHA. The documentation commit below changes Markdown-derived PDF inputs; the required post-documentation
run is reported separately on the PR and must not be conflated with this immutable snapshot. No baseline,
threshold, old aggregate, #78 policy, or #82/#83 behavior was changed.

- **#57 — fixed sample measured; original threshold applied.** All 21 registered outputs were generated and
  retained (six documents through all three named producers plus three pinned scan-derived PDFs): 18 accepted,
  3 refused as `incremental_update`. The accepted-subset `faithful_pct` distribution was n=18, median
  0.00038990779980227345%, Q1 0.0%, Q3 0.19344060494152818%, share-positive 72.22%. Permitted-qpdf and
  forbidden-repack distributions are in [PDF audit §11](pdf-audit.md#11-pr-84-follow-up--57-fixed-21-output-sample).
  The pre-registered rule was **applied and returned refused after measurement** because the faithful median is
  below 1%. Tzim's separate result was 12 optimized rows and 3,565 bytes saved from 72,982,531 optimized-input
  bytes; this optimized-only median was not substituted for the registered metric. There were zero qpdf
  candidate-check failures and zero registered render differences. The exact report is
  [comment `6520274047`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6520274047).
- **#59 — fixed survey measured; threshold not applied.** All 14 exact TESS, GALEX and ROSAT products were
  fetched and measured. Tier 2 Astropy candidates: n=14, median 30.42168674698795%, Q1 29.518072289156628%,
  Q3 64.63408680369086%, share-positive 100%. The per-tier/refusal distribution is Tier 1 comparator n=0,
  Tier 2 reference n=14, refused n=0, not-measured n=0, with no refusal reasons; Tier 1 remains explicitly
  empty, not 0%. Every Astropy candidate was retained by its exact size/SHA, then checked with HEASARC
  `fitsverify -l` and `funpack`. The full round-trip gate failed 14/14 on literal-header/storage-array
  comparison, so the registered threshold was **not applied**. Tzim independently skipped all 14 rows
  unchanged; its 14 direct comparisons concern source-versus-unchanged-copy, not the reference candidates.
  No input repair or FITS-byte upload occurred; the ROSAT boundary was preserved. See [FITS audit §11](fits-audit.md#11-pr-84-follow-up--59-fixed-survey-sample) and [comment `6520285722`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6520285722).
- **#79 — fixed FLAC corpus evidence recorded.** The 43-row manifest SHA-256 remains
  `fe3e15d22d098744bee1955a026c537cc077331b89765a698ddfb6c169de71be`; the run-specific expanded-manifest
  SHA-256 is `225cb1b592da64008f9c0e5679bdb898aa943751579c9b03c93257fcdc58615c`. Counts were 30 valid inputs
  (23 optimized, 7 already optimal), 7 documented refusals and 6 invalid controls. All 23 candidates passed
  external `flac --test`, `metaflac`, exact PCM comparison, and STREAMINFO-MD5 checks. Whole-corpus bytes were
  109,472,390 → 108,727,820; optimized-only bytes were 91,284,082 → 90,539,512; saved 744,570. First-pass
  time was 6.873 seconds total (median 109.207 ms/file). The `flac`-only feature build was 836,384 bytes.
  Exact report: [comment `6520214774`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6520214774).
- **#80 — implementation remains separately authorized and non-default.** The PDF handler supports the narrow
  classic-xref/xref-stream paths, preserves existing `/ObjStm` bytes, validates type-2 members and bounds
  decoded object streams at 8 MiB. It does not wait for #57. The #57 sample's refused result does not alter
  the owner's separate opt-in implementation decision. All-feature and feature-specific CI tests passed; no
  full PDF admission or release-default claim is made.

**Checks and remaining limits at this SHA.** PR CI run `41038741139` passed the platform matrix, all-feature
Rust tests, Clippy, MSRV, package, size, and documentation-link checks; its Astropy FITS oracle passed. Fixed
run `41038741167` passed PDF generation, qpdf/MuPDF measurement and independent recomputation, and all-feature
Rust tests, but the #59 reference-candidate gate correctly made the aggregate run fail. The fixed FLAC run
`41038741487`, fuzz `41038741155`, corpus `41038741154`, third-party `41038741314`, lockfile `41038735328`, and
HEASARC `41038741101` completed successfully. #78's external Astropy EXTNAMX fixture and final evidence-bundle /
repro jobs were skipped; #78 remains open. `measure-repro` run
[41038741066](https://github.com/RUdolf1517/tzim/actions/runs/41038741066) failed at this SHA: the historical
FITS totals were Astropy 780→675 KiB, CFITSIO 60→678 KiB, Drizzlepac 3.3→14.8 MiB, Reproject 2.0→1.8 MiB,
and total 17.8→17.9 MiB (counts/commits reproduced). Historical PDF median/Q1/Q3 and repack median reproduced,
but the mean was 0.4216836773% vs 0.39%, share-positive 69.3756% vs 68%, qpdf mean 1.65% vs 1.68%, and render
pages 168 vs 173; five old Zopfli cells remain unavailable because their per-file sample was not retained.
These are historical-reproduction differences, not the fixed #57/#59 run results; no baseline or threshold was
changed. The local sandbox still lacks Rust/Cargo and HEASARC tools; pinned CI performed the measurements. A
green CI status alone is not treated as proof of acceptance.

## Evidence and limits

The historical 2,208-file PDF and 178-file FITS execution details below remain attributed to the earlier
handover; their historical per-file artifacts were not retained. This is distinct from the actual fixed #57/#59
runs at exact SHA `7b82c9edb6e5060bc8ce7ed756caebba91505b5b`, whose raw rows, reports, commands, environments and
independent aggregates are attached to the workflow and linked above. The ordinary fixture gates and JPEG
benchmark remain separate evidence; they do not turn the failed #59 reference-candidate gate into a pass.

The current instruments are linked in [PDF §9.1](pdf-audit.md#91-what-was-run) and
[FITS §8.1](fits-audit.md#81-what-was-actually-run-and-with-what):

- [PDF measurement script](../.github/scripts/pdf-measure.py) and fixed-sample wrapper/recomputation scripts;
  the exact source SHA and manifest are recorded in the #57 report/artifact;
- [FITS measurement script](../.github/scripts/fits-measure.py) and fixed-sample wrapper/recomputation scripts;
  the existing two-tier instrument SHA-256 is `104d7fb528007a67c8e9fe613a7fe00084fcfbf6ab9e49b47f0bc2c02319aeff`.

The old blob labels `1677d63` and `2978fe4` above the historical aggregates refer to earlier script versions;
they are not the current fixed-sample instrument identities. The current #59 report records Python 3.12.15,
NumPy 2.4.6, Astropy 8.0.2 and pinned CFITSIO 4.7.0 source; #57 records pinned qpdf 12.4.2 and its MuPDF
render tool. The exact raw command arguments and stdout/stderr are retained in the workflow artifacts. The
133,704-byte #59 evidence artifact has SHA-256 `0d7599db4ac4edf833eeff67038bceadfe9a69f78e2d82606a776d3f3b054ba4`; a local download attempt ended in Azure Blob `EOF`, so its ZIP contents remain uninspected in this sandbox. The report comment itself retains the complete 14-row source/candidate identities and summarized command outcomes.

## Independent FITS review handoff

The reviewer supplied a [675-line report](../handoff/fits-review/REVIEW-5dd45b3.md) and evidence archive at
transfer commit `ddff3a91f21b7cacfb16a72517490e60c406b4a6`. The manager verified both file hashes, all ten
fixture hashes and the presence of the 134 indexed tool-run records. The reviewer did not run tzim: its
behavioural results come from a source transcription. These are retained review leads, not actual-binary
acceptance or proof that every implementation path preserves data.

The [manager assessment](../handoff/fits-review/MANAGER-ASSESSMENT.md) records the supported findings and
two methodological corrections: the `fpack` float result does not justify a universal causal claim about the
tool/library, and FITS verification independence is a mix of external gzip decoding and shared permutation
code rather than simply “V1” or “V3”. The conservative `BSCALE`/`BZERO` refusal remains the selected policy.
No finding is closed by this handoff, and no additional parallel coder assignment is authorised by it.

The coder then delivered PR #73 at `5c2f4a2899e852048b4c5278a0f8d3e422ba4ea4`. Both new workflows were red
at that SHA. The [iteration assessment](../handoff/iteration-5c2f4a2/MANAGER-ASSESSMENT.md) rejected #71/#74,
held #72 for evidence/provenance follow-up and resolved the zero-width expectation in favour of
`already_optimal`. In particular, that harness omitted `funpack -C`, so its red header result was not a valid
verdict on the writer.

The latest completed baseline/handoff review target was PR head `9de2b7069a522ab5ce05e8255a3ce4c530f6190f`.
Earlier reviews remain at [`04b303e7`](../handoff/iteration-04b303e/MANAGER-REVIEW.md), [the handoff-only
commit](../handoff/iteration-04b303e/HANDOFF-C0F629F-REVIEW.md), and
[`4a90ade`](../handoff/iteration-4a90ade/MANAGER-REVIEW.md). #71 and #74 are accepted in their assigned scopes
and closed by the owner. The [final #72 acceptance](../handoff/iteration-9de2b70/MANAGER-ACCEPTANCE.md) accepts the
reproducible runner, retained evidence, and complete publication comparison at `9de2b706`. The current targeted
candidate review is `a4a0fdfd3fdc0bb712542176e2657fc2b9d180a8`, documented below; it does not replace the
earlier evidence or constitute release approval.

The corrected PR run keeps the measurement red for real discrepancies while successfully publishing the full
report and uploading evidence. The report contains four PDF result-table differences, five FITS input-byte
differences, and five explicitly unavailable Zopfli cells, with unchanged baselines and thresholds. Its PR
merge-SHA identity was checked against the exact reviewed HEAD. This acceptance does not declare historical
reproduction or resolve those differences. External Actions ZIP contents remain uninspected; the complete PR
comment and successful job steps are the report-relay evidence. FITS release approval has not been issued.

A subsequent [independent reviewer handoff](../handoff/iteration-9de2b70/REVIEWER-HANDOFF-ASSESSMENT.md)
was integrity-checked at transfer commit `eeebd350317396e72491b451efd18683fd45ae1e`: the bundle verifies
429/429 entries and its extracted source blobs match `9de2b706`. It supplies script/gate-level evidence for
F-10/F-11 and O-1, but no tzim binary or Rust test run; F-06/F-07/F-08 therefore remain `not run` for the
shipped binary. O-3/O-4/O-5 remain triage notes. This handoff does not change the acceptance above or approve
FITS release.

## Targeted candidate review — `a4a0fdf`

The [targeted manager review](../handoff/iteration-a4a0fdf/MANAGER-REVIEW.md) inspected only the delta from
`fba375915` to `a4a0fdfd3fdc0bb712542176e2657fc2b9d180a8`: one oracle file, with no writer or `compare_arrays`
change. The narrow F-11 correction is accepted for that scope. Fresh push/pull-request CI, HEASARC, fuzz,
corpus, third-party and lockfile runs are green; the measurement run remains red for the documented comparison
rows. The PR rollup was 41 successful checks plus that one expected measurement failure.

The [candidate evidence inventory](../handoff/iteration-a4a0fdf/EVIDENCE-INVENTORY.md) records exact source,
merge, run, report and artifact identities and hashes. Artifact metadata is present, but both artifact downloads
ended in `EOF`; raw ZIP contents and job logs remain uninspected. At the review stage this was not writer
acceptance, full/stable FITS admission, or permission to merge, tag, publish a Release, or close issues.

## Merge update — `a404837b`

The owner subsequently authorized the merge. PR #73 is now merged at
`a404837b391337dbfbcda86e425e08960a65c782`; its parents are the baseline and the reviewed `a4a0fdf` head, and
its tree is identical to that reviewed head. The [post-merge record](../handoff/iteration-a4a0fdf/POST-MERGE.md)
contains the action and evidence. Main CI run `40524876729` and lockfile run `40524876718` both completed
successfully at the merge SHA. No post-merge failure or unexpected new commit was observed.

This merge does not close #41/#57/#59/#70, resolve the measurement differences, approve a tag/Release, or
publish the durable evidence asset. PR #73 did not include the manager documentation; the release line adds it
separately, and the final release inventory must name the actual release SHA.

## Acceptance matrix

| Issue / requirement | Available evidence in this iteration | Open gap / disposition |
| --- | --- | --- |
| #57: exact source set | Owner-fixed 21-output set; exact run `41038741167` at source SHA `7b82c9edb6e5060bc8ce7ed756caebba91505b5b`, manifest SHA `8b4705e6…` | 18 accepted, 3 refused; `faithful_pct` median 0.00038990779980227345%; original threshold applied and returned refusal. Permitted-qpdf/repack distributions, raw commands, qpdf checks, renders and independent recomputation are retained. The post-documentation rerun is separately reported; do not replace the registered metric with Tzim-only gain |
| #59: exact survey inputs | Owner-fixed 14-product TESS/GALEX/ROSAT set; exact run `41038741167` at source SHA `7b82c9edb6e5060bc8ce7ed756caebba91505b5b`, manifest SHA `7a7b563b…` | 14/14 products downloaded; Tier 2 n=14 (median 30.42168674698795%, Q1 29.518072289156628%, Q3 64.63408680369086%, positive share 100%); Tier 1 explicitly empty. All 14 reference-candidate `funpack` array/literal-header checks failed; threshold **not applied**. Full per-row candidate hashes and external results retained without FITS bytes; all unchanged Tzim skips remain distinct |
| #79: opt-in FLAC | Handler, Claxon PCM/MD5 verification, seekpoint mapping, and regression tests behind `flac`; fixed 43-row run `41038741487` at exact SHA `7b82c9edb6e5060bc8ce7ed756caebba91505b5b` | 23 optimized, 7 already optimal, 7 documented refusals, 6 invalid controls; all 23 optimized rows passed external tools, exact PCM and STREAMINFO-MD5. Saved 744,570 bytes; whole-corpus and optimized-only labels are both recorded. Fuzz, feature-size and timing evidence are linked; broader admission/review remains separate |
| #80: opt-in PDF | Narrow classic-xref/xref-stream implementation, bounded `/ObjStm` validation, tests, behind `pdf` | Implementation is independent of #57 and remains owner-authorized/non-default. #57's original rule refused the fixed sample; qpdf/MuPDF evidence and all-feature tests passed. No default enablement or full PDF-admission claim |
| #78 and #82/#83 | #78 HEASARC run `41038741101` passed; its external Astropy EXTNAMX fixture and final evidence/repro jobs were skipped. #82/#83 code and owner decisions were unchanged | Keep #78 open pending its missing evidence/manager review; preserve #82/#83. No size threshold, baseline, prompt, or release default was changed |
| #70: FITS implementation | PR #69 implementation and existing `fits` feature remain opt-in | This #59 measurement work does not close #70 or make a full FITS-admission claim |
| #41: audit tracker | Historical audit and linked work remain available | This iteration does not claim every original finding is closed |

The owner's exact decisions are retained in
[Owner decisions for the next release scope](../handoff/iteration-fba3759/OWNER-DECISIONS.md). They do not close
#57/#59/#70, accept a new PR head, create a Release, or authorise publication.

## Coder assignments

- [#71: HEASARC release gate](https://github.com/RUdolf1517/tzim/issues/71): actual tools, explicit corpus,
  tzim-produced candidates, retained per-file validation evidence and a negative control.
- [#72: reproducible measurements](https://github.com/RUdolf1517/tzim/issues/72): pinned inputs/environment,
  retained outputs, fresh runs and checks for historical measurement bugs.
- [#74: FITS review follow-up](https://github.com/RUdolf1517/tzim/issues/74): actual-binary reproduction,
  writer corrections where confirmed, and a gate capable of distinguishing harness faults from writer faults.

The owner has relayed assignments to a coder and an iteration has been delivered; no deadline is assumed.
The manager owns product documentation and the acceptance report, not code or CI implementation.

## Historical owner decisions for the general opt-in release — superseded where noted for PR #84

The earlier general-release decision selected **1A, 2C, 3A, 4A, 5C, 6B, 7A**. It remains part of the
historical #72 record, but the owner's later PR #84 instructions supersede its #57/#59 sample-blocker and
#80-waits-for-#57 procedural choices. For this PR, execute the fixed manifests in comment `6498702204`,
implement #79 and #80 independently of the measurements, and continue unrelated work. Preserve #78 and #82/#83;
no baseline/threshold changes, corpus commits, self-merge, self-close, or Release are authorized.

1. This is a general release with FITS behind `--fits-tile`, not full/stable FITS admission. #70 remains an
   implementation tracker and is not closed automatically.
2. Historical FITS results are not used for acceptance or release/performance claims; the violated mismatch
   stop rule is not retroactively relabelled as an approved revised protocol.
3. The five FITS input-byte and four PDF result-table differences remain under investigation before the
   corresponding full format acceptance. Baselines and thresholds are not edited to obtain green CI.
4. The five unavailable Zopfli cells remain an explicit limitation; the earlier general-release decision did
   not request a replacement sample. This does not override PR #84's separate fixed #57/#59 measurement scope.
5. For PR #84, the exact fixed real-toolchain PDF and survey FITS samples were required now and have been run.
   The #57 threshold was applied; the #59 threshold remains unapplied after the reference-candidate mismatch.
   Neither result by itself claims full format admission.
6. A durable measurements/evidence Release artifact is required before publication. Selecting that retention
   policy does not authorise creating or publishing the Release; separate final confirmation is still needed.
7. Signed-P standard/source evidence, arithmetic boundary tests and ordinary CI are sufficient for opt-in F-06
   scope. A >2 GB integration run and full large-file stability claim are not implied.

The complete decision record and remaining release path are in
[OWNER-DECISIONS.md](../handoff/iteration-fba3759/OWNER-DECISIONS.md). These decisions do not close
#57/#59/#70/#41 or constitute merge, tag, Release-upload, or publication approval.

## Additional review items

- The outgoing manager reports that `--fits-tile` is accepted even without the `fits` feature. This review did
  not execute that case. It is a CLI/documentation question, not demonstrated data loss; no behaviour change
  is requested without a policy decision.
- The historical measurement used numeric `equal_nan` comparison, which is not proof of identical NaN payloads
  or signed-zero bits. The current oracle uses FITS storage-byte comparison and has negative controls, but its
  self-check is not a real-file corpus for every registered codec; neither fact is a full-admission claim.
