# PDF/FITS measurement acceptance status

Historical checklist date: 2026-10-27. Baseline: `5dd45b38412ffb762a1fc380bd291351c9e6601e`.
This is a living management checklist, not a completed execution audit or a release approval.

## PR #84 execution status — 2026-10-29

The owner has fixed the #57/#59 source identities and protocol in PR comment
[6498702204](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6498702204). The exact 21-output PDF set and
14-product FITS set are now implemented as fixed-manifest workflows in
[`.github/workflows/fixed-realfiles.yml`](../.github/workflows/fixed-realfiles.yml); no input is selected, dropped,
or substituted based on size or gain. **Neither workflow has run for this source revision yet**, so this is not
measurement evidence. The existing #82 JPEG runtime artifact and #59 fixture gate are separate evidence and do
not satisfy #57 or #59. No threshold, baseline, old aggregate, #78 policy, or #82/#83 behavior was changed.

- **#57 — pending execution, not a source-manifest blocker.** The frozen set is 21 outputs: six fixed Markdown
  documents, each produced by Pandoc/pdflatex, Markdown→DOCX→LibreOffice, and native AppKit/Quartz, plus three
  scan-derived PDFs from the pinned OCRmyPDF source. The workflow must finish generation before freezing the
  expanded manifest and measuring. This sandbox lacks Pandoc, pdflatex, LibreOffice, Swift/Quartz, PyMuPDF,
  qpdf, and Rust/Cargo; the earlier local probe produced 0/15 Ubuntu PDFs because its producer/decoder tools
  were absent. The pinned scan archive and three source-image hashes were checked, but no measurement PDFs or
  qpdf/MuPDF results exist. The workflow must retain all 21 rows and their exact failures if a producer fails.
- **#59 — pending execution, not a source-manifest blocker.** The frozen set is the exact 14 TESS, GALEX and
  ROSAT products in comment `6498702204`, including full products only. No substitutes, cutouts, or repairs are
  allowed. A local HTTPS HEAD probe for TESS/GALEX/ROSAT ended in TLS EOF; this is not a completed GET and does
  not establish that a fixed source is unavailable on the pinned runner. The workflow performs exact GETs,
  records redirects/headers/hashes and retains a failed row plus one exact owner action for any failed product.
  The local sandbox lacks Astropy, NumPy, CFITSIO `fitsverify`/`funpack`, and Rust/Cargo; no fixed-set FITS
  measurements have been run.
- **#79 — implementation present; validation pending.** The FLAC handler is behind the non-default `flac`
  feature, uses flacenc plus independent Claxon PCM decoding/STREAMINFO checks, remaps seekpoints, and refuses
  distinct source seekpoints that collapse to one output frame. Reserved sample-size codes 3 and 7 have a
  regression test. No Rust compilation, test, external `flac`/`metaflac`, fuzz, corpus, or actual feature-size/
  speed/gain result is available in this sandbox. The lockfile was hand-edited because Cargo is absent here;
  CI's branch lockfile job must regenerate it with `cargo generate-lockfile`, and the all-features locked build
  and tests must validate the resulting graph.
- **#80 — implementation present; validation and #57 measurement pending independently.** The non-default
  `pdf` feature supports the narrow classic-xref and supported xref-stream paths, preserves existing `/ObjStm`
  bytes, validates type-2 xref member mappings and bounds decoded object streams at 8 MiB. It does **not** wait
  for #57 to be implemented. No Rust build/test or qpdf/MuPDF run is available locally; the frozen #57 run is
  still required for acceptance evidence.

The sandbox has no local Cargo/Rust toolchain, so none of the newly edited Rust code, lockfile graph, or Rust
regressions has been compiled or tested here. CI must provide that evidence. A green build/test result alone
will not be described as proof of #57/#59 measurements or rendered/decoded equivalence.

## Evidence and limits

The issue bodies, repository documents and measurement script headers were reviewed. The historical PDF/FITS
execution details below are attributed to the earlier handover; they were not independently reproduced in this
iteration. The CI JPEG benchmark and ordinary fixture gates are separate evidence and do not close #57 or #59.

The instruments are already linked in [PDF §9.1](pdf-audit.md#91-what-was-run) and
[FITS §8.1](fits-audit.md#81-what-was-actually-run-and-with-what):

- [PDF measurement script](../.github/scripts/pdf-measure.py), reported historical blob `1677d63`;
- [FITS measurement script](../.github/scripts/fits-measure.py), reported historical blob `2978fe4`.

The handover's statement that no document links these scripts is incorrect: both links exist at the baseline.
The headers provide invocation examples, but their shallow clones track upstream default branches; naming
historical commits in prose does not make those commands check out those commits.

According to the handover, historical per-file outputs (`pdf.jsonl`, `pdf_faithful.jsonl`, `fits.jsonl`,
`zfile.json`) and temporary corpus/environment directories were not retained. Published aggregates remain,
but are not substitutes for per-file evidence. A fresh run must retain its own identity and disclose differences.

The PDF audit records libqpdf 12.4.2 and MuPDF 1.28.5. The handover additionally reports Python 3.11.2,
pikepdf 10.17.0, zopfli 0.4.3, astropy 8.0.2 and numpy 2.4.6; these are unverified historical version claims,
not a tested environment lock. The complete historical OS/package inventory is not available here.

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
| #57: exact source set | Owner-fixed 21-output manifest in comment `6498702204`; generation and frozen-manifest scripts are in the branch | Workflow has not run for this source SHA. Retain all 21 rows, producer/tool identities, raw qpdf/Tzim output, first/middle/last render checks, and independent recomputation; do not claim measurements from CI status alone |
| #59: exact survey inputs | Owner-fixed 14-product TESS/GALEX/ROSAT list in comment `6498702204`; fetch/measurement/recompute scripts are in the branch | Workflow has not run for this source SHA. Preserve failed rows with exact URL/error/one owner action; retain raw source hashes and tool output without uploading FITS bytes; do not substitute products |
| #79: opt-in FLAC | Handler, Claxon PCM/MD5 verification, seektable mapping, and regression tests are in this branch behind `flac` | Rust build/tests, external `flac`/`metaflac`, negative controls/fuzz, full-corpus and actual feature-size/speed/gain evidence remain pending |
| #80: opt-in PDF | Narrow classic-xref/xref-stream implementation, bounded `/ObjStm` validation, and tests are in this branch behind `pdf` | Independent of #57 implementation; Rust build/tests and frozen #57 qpdf/MuPDF measurement/render evidence remain pending |
| #78 and #82/#83 | Existing accepted code and owner decisions were not changed in this iteration | Preserve them unless a concrete failure is demonstrated; no size threshold, baseline, prompt, or release default was changed |
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
4. The five unavailable Zopfli cells remain an explicit limitation. No new sample is required for this general
   release and no comparison is invented.
5. The real-toolchain PDF and representative survey FITS samples remain required for full admission only.
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
