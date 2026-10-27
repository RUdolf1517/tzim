# Manager assessment of the FITS review handoff

Date: 2026-10-26.

## Scope and provenance

The reviewer transferred commit `ddff3a91f21b7cacfb16a72517490e60c406b4a6`, whose parent is the reviewed
baseline `5dd45b38412ffb762a1fc380bd291351c9e6601e`. The manager downloaded the two files by that commit
SHA, without switching branches, and verified:

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `REVIEW-5dd45b3.md` | 78,085 | `e31d8363750b01fdcb71bbdcbbf7d908ae368a5e47b1bcdf67a8df36b3d058fd` |
| `fits-review-5dd45b3-bundle.tar.gz` | 183,056 | `257e136f6802c9f939993f094040c18df1e27882e6f3ff458b4c21c1c4f41d8a94e155551c113376579edd3528777e9b28cc66c1` |

The archive has no absolute paths, parent-directory traversal, links or other non-regular payload entries.
All ten fixture hashes match the manifest. Its index names 134 tool runs and every indexed
`.cmd`/`.stdout`/`.stderr`/`.exit` record exists. Fifteen additional experiment-wrapper exit records are all
zero; expected non-zero exits belong to the individual negative/control runs.

The manager read the report and inspected the code and selected raw records for F-01, F-03, F-05, F-09,
F-10, P-13 and the old PR #73 gate. **No reviewer script, CFITSIO build, corpus measurement or tzim binary
was executed by the manager.** Archive consistency is not independent reproduction.

## Evidence boundary

The review did not run Rust or tzim. Its candidate files and decisions come from a Python transcription of
`src/formats/fits.rs`; its external-tool runs exercise those candidates. Consequently:

- the report is useful, retained evidence about the reviewed source and transcription;
- a behavioural finding still requires reproduction with the actual tzim binary before it is called a
  confirmed shipped-binary defect;
- the report's broad statement that data are not corrupted applies only to the tested transcription
  scenarios, not to the implementation generally;
- PR #73 observations belong to snapshot `88e9041d87ec75655aa7571322c5f3eda4a6c692`. They must not be
  transferred automatically to a later PR head.

## Finding disposition

| Finding | Manager disposition |
| --- | --- |
| F-01 random groups | Supported by the source arithmetic and the recorded transcription result. The source multiplies through `NAXIS1=0` before the `GROUPS=T` refusal can run. Actual-binary regression is still required. |
| F-02 `SIMPLE=F` | Supported as a source/contract discrepancy: classification checks card presence, not its truth value. Keep the documented refusal unless the owner explicitly changes the accepted subset. |
| F-03 headers/comments | Supported by the source and recorded reconstruction diff. The writer omits `ZEXTEND` and generates replacement comments for structural cards. The current preservation contract is not waived. |
| F-04 | Retracted by the reviewer; do not implement a fix for it. |
| F-05 scaling syntax | Supported as a source/contract discrepancy: integer-only parsing can treat a non-identity floating spelling as the identity default. The manager selects **conservative refusal**, not expanded scaled-image support. |
| F-06 `1P` limit | A code-derived limit question, not an executed large-file reproduction. Resolve the exact convention bound explicitly; do not present an approximate “2.1 GB” prose value as an exact measured boundary. |
| F-07/F-08 | Source inspection supports missing negative verification coverage and unchecked generated-card properties. These are verification gaps, not demonstrated data corruption. |
| F-09 / D-4 | **Not accepted as written.** See the independence correction below. |
| F-10 | Supported: the recorded NaN-payload mutation passes `equal_nan` while byte comparison differs. Numeric equality must not be described as bitwise equality. |
| F-11 | Supported as a naming/scope issue. A policy oracle may require `GZIP_2` for float because that is what tzim writes; it must not call that a universal convention requirement. |
| F-12 / D-8 | Documentation premise needs correction after comparison with the actual fixtures and contract. Do not use it to broaden the zero-width behaviour silently. |

F-01, F-03, F-05 and the unaccepted HEASARC gate remain release-blocking as a matter of current project
policy. Other findings require explicit disposition before a stability claim; this assessment does not
close them or create an additional parallel work order.

## Method corrections to the review

### 1. P-13: retain the practical rule, narrow the causal claim

E14/E15 support the narrow observation that the recorded `fpack` invocations changed float bit patterns in
that environment. They do **not** establish that every `fpack` configuration is lossy, nor conclusively
isolate the cause to the utility rather than a particular library call path or configuration:

- the pinned CFITSIO `docs/fpackguide.md` says `-q 0` disables quantization and losslessly compresses with
  GZIP, so the observed result is unexpected and needs investigation rather than a general claim about the
  tool;
- the separate C-API control is not an equivalent configuration/control for every `fpack` operation, and its
  recorded header contains quantization/scaling-related cards;
- eight exact values in that C-API run do not prove that every library path preserves every float.

The gate rule is nevertheless clear: compare the original array bytes with the bytes recovered from the
**tzim candidate**, using the pinned external decoder and explicit endian handling. Do not use `fpack` output
as the bitwise float reference. A reference `fpack` trace may still test header-reconstruction feasibility.

### 2. F-09/D-4: independence is mixed, not simply “V1 is false”

Project §2.6 defines V1 as an independent decoder written outside this codebase. `flate2` therefore fits the
same V1 convention already used for gzip/zlib/ZIP, even though the encoder also uses that crate. FITS also
has a shared `shuffle`/`unshuffle` stage, which can hide a symmetric permutation error. The accurate
documentation is consequently **V1 external gzip inflation/checksum plus a shared V3 permutation stage**,
with external `funpack` evidence named separately. The review's proposed “V3 (own inflater)” description is
not accurate: the inflater is `flate2`.

### 3. F-05: conservative refusal without lossy numeric classification

Preserve the current refusal of non-identity `BSCALE`/`BZERO`. Accept identity only across valid equivalent
FITS numeric spellings. Do not make a binary64 (`f64`) parse the sole arbiter where rounding could turn a
large non-identity value into an apparently acceptable one; use syntax/range handling that cannot silently
misclassify the card. This is a contract decision, not permission to broaden scaled-image support.

### 4. C-10 is conditional, not a universal acceptance criterion

A legal convention file using `GZIP_1` for float need not be rejected by a general convention linter. The
project oracle, however, checks the handler's promised output, for which `GZIP_2` is required. Keep those
roles distinct; no policy change follows from C-10 alone.

## Standing gate decisions

1. Preserve original header cards, including structural-card comments, according to the current contract.
   The reconstructed-header comparison uses full 80-column cards in original order.
2. Use `funpack -C` for the literal original-versus-reconstructed header gate. Do not compare two outputs
   after passing both through a reference writer.
3. `fitsverify` supplies structural evidence; it does not inflate and validate tile payloads. `funpack` exit
   zero alone is also insufficient: retained byte comparison is required.
4. Do not normalize discrepancies to obtain a green result.
5. No FITS release is authorised while the gate is red or its required evidence is incomplete.

## Sequencing

The coder is still completing the current #71/#72 iteration. This assessment is input to its acceptance,
not a new parallel assignment. Issue #74 remains a parked review-follow-up list until the current result is
handed over. Any post-fix review must name a new SHA and use actual tzim outputs; none of the transcription
results are reusable as post-fix proof.
