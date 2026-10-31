# PDF/FITS measurement acceptance status

Historical checklist date: 2026-10-27. Baseline: `5dd45b38412ffb762a1fc380bd291351c9e6601e`.
This is a living management checklist, not a completed execution audit or a release approval.

## PR #91 current exact-SHA status — 2026-10-31

The current fixed-sample result is bound to source SHA
`c69d25e377215031dee2f2245afc967604423ae9`. Fixed workflow run
[41215985536](https://github.com/RUdolf1517/tzim/actions/runs/41215985536) and its [exact report comment](https://github.com/RUdolf1517/tzim/pull/91#issuecomment-6545587417)
record the 14 frozen TESS/GALEX/ROSAT inputs (source-manifest SHA-256
`500fdbe6763da40b5db856f9ea7218e18b244956f41f4505e4a838836ebb79d0`). The run used the pinned CFITSIO archive
and terminal-blank patch; instrument SHA-256 `be27888e60b2bfab9731d3eebadbe9ac2b5b9ac1d2f96d2ebcf1bacd9de634a2`.
The evidence artifact is `fixed-fits-evidence` (ID `13048795912`); it retains hashes and tool output, not FITS
payload bytes.

All 14 reference candidates passed `funpack -C`; 20/20 HDUs matched exact literal cards through `END` and exact
storage payload, with zero inserted, dropped, or changed cards and zero payload mismatches. `fitsverify -l`
reported 10 clean passes and 4 passes with inherited source warnings. The external positive and negative controls
passed. The unchanged pre-registered #59 rule is **applied**: Tier 2 clears for opt-in `--fits-tile`; its 14-row
gain distribution was median 30.42168674698795%, Q1 29.518072289156628%, Q3 64.63408680369086%, and 100%
positive. Tier 1 is empty/not evaluated. Tzim skipped all 14 products unchanged and direct Tzim comparisons passed
14/14. This is not full FITS admission; the ROSAT permission and no-payload-publication boundaries remain. The
separate #57 rule remains applied and **refused after measurement**.

### Required `measure-repro` investigation

The required comparison still fails at the same exact source SHA in run
[41215985505](https://github.com/RUdolf1517/tzim/actions/runs/41215985505), report
[comment `6545555319`](https://github.com/RUdolf1517/tzim/pull/91#issuecomment-6545555319). All three examined
runs used the same 2,386-file manifest digest
`57e18a9d97d70e4c9e8fd919546c537fea79f9d30e42437c8b4d409fd027ecc5`; the result at `c69d25e` is identical to
`385e0cb`, while `e18d84d` predates the common FITS instrument change:

| Exact source SHA / run | FITS candidates | Exact-verified files | `cannot_compress` | Exact failures |
| --- | ---: | ---: | ---: | ---: |
| `e18d84df4aa5d63a4a0ae6e6eb9891620fd2d95c` / [41190144163](https://github.com/RUdolf1517/tzim/actions/runs/41190144163) | 140 | 140 | 13 | 0 |
| `385e0cb1a5bd113b93fbf402976bf580a6025ffb` / [41190823523](https://github.com/RUdolf1517/tzim/actions/runs/41190823523) | 148 | 147 | 5 | 1 (`zerowidth.fits/array_0`) |
| `c69d25e377215031dee2f2245afc967604423ae9` / [41215985505](https://github.com/RUdolf1517/tzim/actions/runs/41215985505) | 148 | 147 | 5 | 1 (`zerowidth.fits/array_0`) |

The `e18d84d` → `385e0cb` diff changes `.github/scripts/fits-measure.py`'s compressed-header repair: it restores
omitted literal source cards and rebuilds/pads the candidate header, while retaining hard errors for ambiguous or
same-key/different-value cards. Across that instrument change, the report moves by +8 candidates, +7 exact-verified
files, −8 `cannot_compress` refusals, and exposes one exact failure (`zerowidth.fits/array_0`). The published
failure set also includes `double_ext.fits/keyword_EXTNAMX`, which the current result no longer reports. The
`385e0cb` → `c69d25e` change is the pinned CFITSIO terminal-blank `funpack -C` patch, not a
`fits-measure.py` change; it has no effect on these `measure-repro` results.

The current c69 report's remaining FITS cells that differ from the published table are:

| Cell | Published | Measured at c69 |
| --- | ---: | ---: |
| `tier2_candidates` | 153 | 148 |
| `tier2_files` | 151 | 147 |
| `tier1_files` | 151 | 147 |
| `tier2_q1` | 17.4 | 18.2 |
| `tier2_mean` | 19.1 | 20.0 |
| `tier2_share_positive` | 83 | 85 |
| `bytes_before` | 17,827,200 | 17,648,640 |
| `bytes_after` | 13,343,040 | 13,121,280 |
| `bytes_pct` | 25.2 | 25.7 |
| `exact_failure_count` | 2 | 1 |
| `median[astropy]` | −7.1 | −21.4 |
| `by_status[refused]` | 25 | 30 |
| `by_status[tier2]` | 153 | 148 |
| `refusals[cannot_compress]` | None | 5 |
| `exact_failure_set` | `double_ext.fits/keyword_EXTNAMX`; `zerowidth.fits/array_0` | `zerowidth.fits/array_0` |

The changed count/verification cells and their derived statistics are tool-version-sensitive: the e18d→385e
instrument change accounts for the observed candidate/refusal delta, while the reports do not establish a
per-file cause for each of the five remaining refusals. The c69 report explicitly identifies
`zerowidth.fits/array_0` as rewritten but not verified; its public summary does not enumerate the five
`cannot_compress` filenames or error strings. The existing `measure-repro-evidence` artifact (ID `13048345852`,
416,909 bytes, not expired) is the raw-row evidence source; prior artifact downloads ended in `EOF`, so those five
rows remain uninspected. No tool or baseline change is made on an incomplete per-row attribution.

The following published-versus-measured differences were already present in the `e18d84d` report, before that
instrument change, and remain identical at `385e0cb` and `c69d25e`; they are not caused by the current common
`.github/scripts/fits-measure.py` change:

- FITS selected-file byte totals (counts and repository commits reproduce): Astropy 780→675 KiB; CFITSIO
  60→678 KiB; Drizzlepac 3.3→14.8 MiB; Reproject 2.0→1.8 MiB; total 17.8→17.9 MiB.
- PDF: mean 0.39→0.42168367731371126; positive share 68→69.37561942517344; qpdf mean
  1.68→1.6538774233025468; render pages 173→168. The historical 173-page sample path list/per-file rows were
  not retained, so the old selection cannot be reconstructed.
- Five Zopfli cells remain unavailable, not mismatched: `sample_files` (published 150), median (0.15), Q1 (0.14),
  Q3 (2.81), and positive share (91). The historical 150-file path list/per-file rows were not retained; no new
  sample is substituted.

The reports remain red evidence, not permission to alter published baselines, corpus, thresholds, or required
status. Exact reports: `e18d84d` [comment `6541884360`](https://github.com/RUdolf1517/tzim/pull/91#issuecomment-6541884360),
`385e0cb` [comment `6541976402`](https://github.com/RUdolf1517/tzim/pull/91#issuecomment-6541976402), and `c69d25e`
[comment `6545555319`](https://github.com/RUdolf1517/tzim/pull/91#issuecomment-6545555319).

## Historical PR #84 execution status — 2026-10-30

The owner-fixed #57/#59 source identities and protocol are in PR comment
[6498702204](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6498702204). The fixed-manifest workflows
retain every row and do not select, drop, repair, or substitute inputs based on size or gain. Runs at `7b82c9ed`,
`5721867`, `5967e31`, and `f05f6d2` are historical snapshots. The 572/5967/f05 #59 header mismatch counts and
0/14 verdicts came from a padded, positional comparator; they are **superseded instrument results, not final
candidate-content verdicts**. The four independent TESS `fitsverify -l` warnings are retained as findings for
those exact runs; they are not current blockers after the later c69 run documented above.

The comparator correction was introduced in PR #84 and reran all 14 fixed #59 products with `funpack -C` for every
source/candidate pair (including unchanged rows), an external integer-only positive control, and exact
storage-payload checks. Each report is authoritative only for its recorded source SHA. The PR #91 c69 report above
is the current disposition and includes non-truncated per-product/per-HDU diagnostics. The #57 report also
publishes its accepted-subset `faithful_pct` rows and recomputation inputs. This document is itself a
Markdown-derived #57 PDF input, so historical workflow comments must not be attributed to a later SHA. No baseline,
threshold, old aggregate, #78 policy, or #82/#83 behavior was changed.

- **#57 — fixed sample measured; original threshold applied and refused after measurement (previous 5967e31 snapshot).** At exact source
  SHA `5967e318951f9c3945f0c256c3095b347483e234`, all 21 pre-registered outputs were retained: 18 accepted and
  3 refused as `incremental_update`. The registered accepted-subset `faithful_pct` distribution (flate2 level 9)
  was n=18, median 0.00034127460376596526%, Q1 0.0%, Q3 0.20532715969914445%, share-positive 72.22%; the
  registered refusal bar remains 1%. The original rule was **applied and returned refused after measurement**.
  The exact-SHA #57 comment publishes every accepted `faithful_pct` row with its raw formula inputs; that
  comment's recorded source SHA governs the snapshot. Tzim optimized 12/21 rows and saved 3,723 bytes from
  76,993,480 optimized-input bytes; this separate
  optimized-only metric was not substituted. Qpdf candidate-check failures and registered render differences were
  both zero. Manifest SHA-256 `a5ecf26b32bda38353f743e1cd450b7a7b44d3ccff2a35827858e4406092858c`; report
  [comment `6521774706`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6521774706).
- **#59 — prior 5967 exact-SHA fixed-survey snapshot; padded-comparator result superseded.** At exact SHA
  `5967e318951f9c3945f0c256c3095b347483e234`, all 14 frozen TESS/GALEX/ROSAT products downloaded and measured;
  Tier 2 remained n=14 with median 30.42168674698795%, Q1 29.518072289156628%, Q3 64.63408680369086%, and
  100% positive share. Tier 1 is explicitly empty, not 0%. The real integer-only CFITSIO `fpack` →
  `fitsverify -l` → `funpack -C` positive control passed through the same literal-header/storage-array comparator;
  attribution was enabled, and no fpack float-array oracle was used. That run reported 20 header-only HDU
  mismatches and 2,412 positional card differences, with no storage-payload mismatches. Because the comparator
  included record padding and was position-based, its header counts and 0/14 gate outcome are superseded
  instrument results—not final candidate-content verdicts. The independent `fitsverify -l` findings remain valid
  for this 5967e31 snapshot: 4 failed TESS candidates, 4 inherited-warning GALEX candidates, and 6 passed ROSAT
  candidates. The four TESS checksum warnings blocked that historical snapshot; the later c69 result above
  supersedes its current disposition with 14/14 reference candidates passed and #59 applied. The PR #84 exact-SHA
  report published its corrected card-through-END and payload diagnostics.
  Tzim independently skipped all 14 rows unchanged; no input repair or FITS-byte upload occurred and the ROSAT
  boundary was preserved. The a1 default-checksum outcome remains historical and is superseded only as an
  instrument result. Source-manifest SHA-256 `e96d7c913754a4c1931d307110509a49f78c909685b63cf8ce92875c18bf02de`;
  report [comment `6521814144`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6521814144), artifact
  `fixed-fits-evidence` ID `12973643986`, SHA-256
  `fc84ed264206712a35bc2b9d01327248d1ab70f2536d6f93fd2da805af177fe0` (no FITS bytes). See [corrected FITS
  audit §13](fits-audit.md#13-corrected-59-exact-head-rerun-at-5967e31), [protocol §12](fits-audit.md#12-corrected-59-reader-mode-protocol), and [historical a1 comment
  `6520583133`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6520583133).
- **#79 — fixed FLAC corpus evidence recorded.** The exact-SHA run at
  `5967e318951f9c3945f0c256c3095b347483e234` retained all 43 rows; the run-specific expanded-manifest SHA-256
  was `effc9ceb27f12be56dfb52709500f29307b78a435da838aa505b113a54b16193`. Counts were 30 valid inputs
  (23 optimized, 7 already optimal), 7 documented refusals and 6 invalid controls. All 23 candidates passed
  external `flac --test`, `metaflac`, exact PCM comparison, and STREAMINFO-MD5 checks. Whole-corpus bytes were
  109,472,390 → 108,727,820; optimized-only bytes were 91,284,082 → 90,539,512; saved 744,570. First-pass
  time was 6.371 seconds total (median 102.685 ms/file). The `flac`-only feature build was 836,384 bytes.
  Exact report: [comment `6521735772`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6521735772).
- **#80 — implementation remains separately authorized and non-default.** The PDF handler supports the narrow
  classic-xref/xref-stream paths, preserves existing `/ObjStm` bytes, validates type-2 members and bounds
  decoded object streams at 8 MiB. It does not wait for #57. The #57 sample's refused result does not alter
  the owner's separate opt-in implementation decision. All-feature and feature-specific CI tests passed; no
  full PDF admission or release-default claim is made.

**Checks and remaining limits for the prior 5967e31 snapshot.** PR CI run `41049496986` passed the platform
matrix, all-feature Rust tests, Clippy, MSRV, package, size, and documentation-link checks. Fixed run
`41049496855` completed generation, measurement, and independent recomputation; its padded-comparator #59
header verdict is superseded as an instrument result. The four independent TESS checksum warnings were blockers
for that historical SHA-specific run; the current c69 result is recorded above. Fixed #57 applied the original
rule and returned refused after measurement. Fixed FLAC run `41049496908`, fuzz `41049496757`, corpus
`41049496752`, third-party `41049496948`, lockfile `41049489944`, and HEASARC gate `41049496840` completed
successfully. The `measure-repro` run `41049496753` failed on published-versus-measured differences; no baseline
or threshold was changed. #78 remains open; no result here closes it.

The unrelated historical `measure-repro` check
[41047304810](https://github.com/RUdolf1517/tzim/actions/runs/41047304810) failed on the published-versus-measured
differences, not on the fixed #57/#59 gates: FITS totals were Astropy 780→675 KiB, CFITSIO 60→678 KiB,
Drizzlepac 3.3→14.8 MiB, Reproject 2.0→1.8 MiB, and total 17.8→17.9 MiB (counts/commits reproduced). Historical
PDF median/Q1/Q3 and repack median reproduced, but mean was 0.4216836773% vs 0.39%, share-positive 69.3756% vs
68%, qpdf mean 1.65% vs 1.68%, and render pages 168 vs 173; five old Zopfli cells remain unavailable because
their per-file sample was not retained. No baseline or threshold was changed. The local sandbox lacks Rust/Cargo
and HEASARC tools; pinned CI performed those measurements. A green CI status alone is not treated as proof of
acceptance.

## Evidence and limits

The historical 2,208-file PDF and 178-file FITS execution details below remain attributed to the earlier
handover; their historical per-file artifacts were not retained. The #57/#59 reports at `7b82c9ed`, `5721867`,
`5967e31`, `f05f6d2` and a1 remain historical snapshots. The a1 default-checksum result and the later padded,
position-based #59 header outcomes are superseded as instrument results, not candidate-content verdicts. Their
independent `fitsverify -l` findings, including the four TESS checksum warnings, remain part of those historical
reports only. The current c69 #59 report above carries the exact source SHA, raw per-product/per-HDU
card-through-END and storage-payload results, and current threshold disposition; no corpus or candidate payload
bytes are published. The #57 report comments carry their own exact SHA and accepted-subset PDF rows. This document
is itself an input to the #57 PDF sample, so later SHA-specific snapshots must not be conflated. The #78
final-evidence/review limitations and ordinary fixture gates remain separate; neither turns a failed required
candidate gate into a pass.

The current instruments are linked in [PDF §9.1](pdf-audit.md#91-what-was-run) and
[FITS §8.1](fits-audit.md#81-what-was-actually-run-and-with-what):

- [PDF measurement script](../.github/scripts/pdf-measure.py) and fixed-sample wrapper/recomputation scripts;
  the exact source SHA and manifest are recorded in the #57 report/artifact;
- [FITS measurement script](../.github/scripts/fits-measure.py) and fixed-sample wrapper/recomputation scripts;
  the exact c69 two-tier instrument SHA-256 is `be27888e60b2bfab9731d3eebadbe9ac2b5b9ac1d2f96d2ebcf1bacd9de634a2`.

The old blob labels `1677d63` and `2978fe4` above the historical aggregates refer to earlier script versions;
they are not the current fixed-sample instrument identities. The c69 #59 report above records Python 3.12.15,
NumPy 2.4.6, Astropy 8.0.2 and pinned CFITSIO 4.7.0 source; the 5967e31 #57 snapshot records pinned qpdf
12.4.2 and its MuPDF render tool. Exact-SHA PR comments publish their own source-specific raw values; the c69
workflow artifact retains commands, stdout/stderr, identities, recomputation, and complete structured per-HDU
diagnostics without FITS bytes. The 5967e31 #59 artifact `fixed-fits-evidence` (ID `12973643986`, 992,196
bytes, SHA-256 `fc84ed264206712a35bc2b9d01327248d1ab70f2536d6f93fd2da805af177fe0`) is historical
padded-comparator evidence, not a substitute for the c69 report. A separate older 5721867 artifact download
returned Azure Blob `EOF`; that download is not retried.

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
| #57: exact source set | Owner-fixed 21-output set; exact 5967e31 snapshot `41049496855` at source SHA `5967e318951f9c3945f0c256c3095b347483e234`, manifest SHA `a5ecf26b…` | Previous snapshot: 18 accepted, 3 refused; registered `faithful_pct` median 0.00034127460376596526%; original threshold applied and returned refusal. The current-head report posts every accepted `faithful_pct` value with its raw formula inputs; its recorded SHA governs the document snapshot. The rule/disposition are unchanged; never replace the registered metric with Tzim-only gain |
| #59: exact survey inputs | Owner-fixed 14-product TESS/GALEX/ROSAT set; exact c69 source SHA `c69d25e377215031dee2f2245afc967604423ae9`, fixed run [41215985536](https://github.com/RUdolf1517/tzim/actions/runs/41215985536), report [6545587417](https://github.com/RUdolf1517/tzim/pull/91#issuecomment-6545587417) | All 14 reference candidates passed `funpack -C`; 20/20 HDUs matched exact cards through `END` and storage payload, with zero card/payload mismatches. `fitsverify -l`: 10 clean and 4 inherited-warning passes; positive and negative controls passed. The unchanged threshold is **applied** for opt-in `--fits-tile`; Tier 1 is empty, and Tzim skipped all 14 rows unchanged. The 572/5967/f05 padded-comparator 0/14 results and four TESS warnings remain historical SHA-specific findings, not current candidate-content verdicts. Preserve the ROSAT permission boundary, open #78 and final-evidence limitations |
| #79: opt-in FLAC | Handler, Claxon PCM/MD5 verification, seekpoint mapping, and regression tests behind `flac`; fixed 43-row run `41049496908` at exact SHA `5967e318951f9c3945f0c256c3095b347483e234` | 23 optimized, 7 already optimal, 7 documented refusals, 6 invalid controls; all 23 optimized rows passed external tools, exact PCM and STREAMINFO-MD5. Saved 744,570 bytes; whole-corpus and optimized-only labels are both recorded. Fuzz, feature-size and timing evidence are linked; broader admission/review remains separate |
| #80: opt-in PDF | Narrow classic-xref/xref-stream implementation, bounded `/ObjStm` validation, tests, behind `pdf` | Implementation is independent of #57 and remains owner-authorized/non-default. #57's original rule refused the fixed sample; qpdf/MuPDF evidence and all-feature tests passed. No default enablement or full PDF-admission claim |
| #78 and #82/#83 | #78 HEASARC run `41049496840` passed; its external Astropy EXTNAMX fixture and final evidence/repro jobs remain skipped. #82/#83 code and owner decisions were unchanged | Keep #78 open pending its missing evidence/manager review; preserve #82/#83. No size threshold, baseline, prompt, or release default was changed |
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
5. For PR #84, the exact fixed real-toolchain PDF and survey FITS samples were required and run. At that
   historical snapshot, #57 applied its rule and refused after measurement; #59 was recorded as unapplied after
   the padded-comparator reference-candidate mismatch. The later exact c69 #59 disposition is applied as recorded
   above; neither result by itself claims full format admission.
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
