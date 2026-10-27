# PDF/FITS measurement acceptance status

Date: 2026-10-27. Baseline: `5dd45b38412ffb762a1fc380bd291351c9e6601e`.
This is a living management checklist, not a completed execution audit or a release approval.

## Evidence and limits

The manager inspected the issue bodies, repository documents and measurement script headers. No measurement,
Rust build, corpus download or HEASARC validation was executed in this review. Historical execution details
below are attributed to the outgoing manager's handover; they were not independently reproduced.

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

| Issue / requirement | Available evidence | Open gap / disposition |
| --- | --- | --- |
| #57: PDF distribution and refusal decision | PDF audit §9, retained rows, and complete published reports | Owner selected 3A: investigate the four result-table differences before full PDF acceptance; baselines stay unchanged. The general opt-in release makes no resolved-acceptance claim |
| #57: corpus provenance and real-toolchain sample | Three full-SHA repository revisions, archive/file hashes, and a producer breakdown | Owner selected 5C: the separate 20–50 document sample remains required for full admission, not for the general opt-in release |
| #59: FITS distribution | FITS result-table cells reproduce; full-SHA input comparison is explicit | Owner selected 2C/3A: historical results are not release claims, and the five input-byte differences remain under investigation before full FITS acceptance |
| #59: survey sample and archive checksums | Full-SHA repository/archive/file hashes; small-image sample | Owner selected 5C: the survey sample remains required for full admission, not for the general opt-in release |
| #59: stop on any mismatch | Two failures and post-hoc exclusions disclosed in §8.2 | Owner selected 2C: the historical run is not used for acceptance or release/performance claims and is not retroactively called a revised pre-registration |
| #59 / FITS release: HEASARC checks | #71/#72 accepted at `9de2b706`; #74 accepted in assigned scope; later candidate checks are reviewed separately | Owner selected a general opt-in release, not full admission. F-06 standard/arithmetic/unit/CI evidence is sufficient for that scope under 7A; full-admission gaps remain open |
| #70: implementation | PR #69 merged; feature off by default | Owner selected 1A: #70 tracks implementation. It is not full format admission or a stability claim, and any closure is a separate owner action |
| #41: audit tracker | Existing historical audit and linked work | Remains open pending constituent acceptance; this review does not verify every original finding |

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

## Owner decisions for the general opt-in release

The owner selected **1A, 2C, 3A, 4A, 5C, 6B, 7A**. In summary:

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
