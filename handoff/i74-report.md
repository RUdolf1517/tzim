# #74 continuation — #71 FITS review and #72 metric evidence

Branch `arena/7ded2747-tzim`, PR **#73** remains open. Latest head **`5c2f4a2`**. No product documentation changed, no Release was created, and no issues were closed.

## 1. Full FITS diagnostics are now in CI text and the artifact

Latest review: [FITS gate run 40344219155](https://github.com/RUdolf1517/tzim/actions/runs/40344219155), [job 121646705212](https://github.com/RUdolf1517/tzim/actions/runs/40344219155/job/121646705212), [full PR report](https://github.com/RUdolf1517/tzim/pull/73#issuecomment-6419187197). It relays a dedicated Markdown report, not the last 150 lines of the whole build log. The checksummed 90-day artifact additionally retains original and tzim-candidate FITS bytes, full `fitsverify`/`funpack` output, exact header-diff JSON/text, control details, and HDU byte layouts.

The actual recovered-header diff is **not** “only `EXTNAME`.” It shows structural card reformatting/comments, `EXTEND` absent in some reconstructed primary headers, and `CHECKSUM`/`DATASUM` in reconstructed output. The full report preserves each 80-column card. Because the comparison is original-vs-`funpack` reconstruction, it does not by itself attribute a difference to `tzim`; the raw candidate is retained. No normalization and no writer change.

## 2. Writer changes are limited to confirmed causes

**No FITS writer code has been changed.** The latest evidence does not isolate a confirmed tzim writer defect:

- The multi-HDU source was itself rejected by `fitsverify` for the nonstandard `HDRLET` extension. A separate malformed table-width value in the *fixture* was corrected (`NAXIS1=8` for one 8-byte `1PB` descriptor); the harness self-check covers it. This is not a writer change or a gate exemption.
- The candidate’s independent parser layout contains five HDUs, but `fitsverify`/`funpack` stop after the compressed primary and encounter the `HDRLET` payload bytes while looking for the next header. The report includes original and candidate offsets and both original/candidate tool outputs. This remains an unresolved tool/format interaction, not yet a confirmed tzim bug.
- The literal diff includes cards written by the reconstruction path; it has not been attributed to tzim. No header differences were normalized.

Therefore there is no claimed writer “failing-before/passing-after” fix yet. If a writer change becomes justified, it will need that test pair before it is described as fixed.

## 3. Zero-width case and the exact contract interaction

Input: empty primary; `IMAGE` extension, 16-bit `32×64`, `EXTNAME='SCI'`; then `BINTABLE` with `NAXIS1=8`, two rows, `TFORM1='0A'`, `TFORM2='8A'`. It is 17,280 bytes; input SHA-256 `57eb1edaadd40271e5e4a75e07c73d38008854fd235ae789fad1b1db4b58b3cf`.

The relevant clauses interact: §4.12 permits tiling an eligible image while copying other HDUs verbatim, **but writes the whole-file candidate only when it is strictly smaller**. The observed tzim JSON is `already_optimal`, no note, bytes unchanged; `fitsverify` exits 0. The discarded candidate’s size is not exposed, so this run does not prove whether there was no whole-file gain or whether the zero-width table affects the decision. The previous `mixed` expectation demanded a rewrite without accounting for the strict-size clause. It is now explicitly **undecided**; no writer behavior is expanded and no zero-width whole-file refusal is inferred pending owner agreement.

## 4. Four metrics: definitions, source rows, and current verdict

The measurement re-run is [40344219423](https://github.com/RUdolf1517/tzim/actions/runs/40344219423), [job 121646706476](https://github.com/RUdolf1517/tzim/actions/runs/40344219423/job/121646706476), with [CI comment 6419206909](https://github.com/RUdolf1517/tzim/pull/73#issuecomment-6419206909). CI evidence bundle digest: `8d45bc4134c9c783488d290815a9051e0cfb370a73b022facbbdc47cf6a7cf5a`.

1. **FITS `astropy` median:** `pct=100*(bytes-new_size)/bytes`; `status=tier2 && exact=true`; `n=12`; conventional even median averages the two middle values `−14.285714%` and `0%` → **−7.142857%**. The earlier replay incorrectly used the upper middle (`0%`).
2. **FITS `reproject` median:** same formula and filter; `n=16`; center values `40.909091%` and `42.857143%` → **41.883117%**. The earlier upper-middle result `42.857%` used the wrong median definition.
3. **PDF share positive:** `faithful_pct=100*(F−F9)/S`, where `S` is whole-file bytes, `F` is the original encoded size of eligible Flate streams, and `F9` sums `min(raw length, zlib-level-9 recompressed length after inflate)` per stream. Positive iff `faithful_pct>0`; current count **700/1,009 = 69.375619%**, with 309 zero and 0 negative. Published value `68%` remains unresolved; the original per-file rows were not preserved.
4. **PDF render pages:** `random.seed(57)`, sample 140 accepted files in manifest order, PyMuPDF at 100 dpi, maximum six pages per file. `render_pages` is the sum of pages actually compared, not full document lengths. Current result **168**, 140 files, 0 visual differences, 0 render errors. Published `173` has no preserved sample path list, so the old sample cannot be shown to contain the same 140 files.

The source-row filenames in the CI artifact are `fits-corpus-median-source.jsonl`, `pdf-share-positive-source.jsonl`, and `render-sample.jsonl` (with per-page hashes); complete JSONL, formulas, environment, commands, manifest, and `SHA256SUMS` are retained. The first two apparent differences were a measurement-formula bug in the replay; the last two remain red. The old numbers are not presumed correct.

## 5. Control count: two negative controls plus one positive check

| Name | Expected | Actual on latest gate run |
|---|---|---|
| `corrupt_tile` — negative | Integrity comparison sees intact tile match source and flipped tile differ or fail to inflate | Detected; flipped gzip tile failed to inflate (`invalid bit length repeat`); `fitsverify` exit 0. This does not claim fitsverify detects altered payloads. |
| `broken_structure` — negative | `fitsverify` exits nonzero for `NAXIS1=16` vs 8-byte `TFORM1` | Detected; exit 1 with the exact width mismatch. |
| `toolchain` — positive fpack sanity | `fpack` output is accepted by `fitsverify` | Passed; fpack 0, fitsverify 0. |

The harness summary contains three control records, but only the first two are negative controls. The earlier phrase “three controls that must go red” was incorrect.

## 6. Reports and attachments

This report is accompanied by revised `i71-report.md`, revised `i72-report.md`, and the original corrected `i70-report.md` in the attached combined report. The metric source rows and their checksum are in the attached source-record bundle. These are copies surfaced here; the `/home/user/handoff/` path itself is not being relied on as your only access route.

**What this does not prove:** the FITS gate is still red; it does not certify the nonstandard `HDRLET` shape, attribute reconstructed-card changes to tzim, resolve the zero-width contract interaction, or establish historical source rows for the old PDF aggregates. No writer change is claimed until a cause is confirmed.
