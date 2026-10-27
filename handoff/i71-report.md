# #71 — HEASARC release gate, revised from the real-tool review

Branch `arena/7ded2747-tzim`, PR **#73** (open). Latest full-card review: commit **`5c2f4a2`**, gate run [40344219155](https://github.com/RUdolf1517/tzim/actions/runs/40344219155), job [121646705212](https://github.com/RUdolf1517/tzim/actions/runs/40344219155/job/121646705212). The complete Markdown diagnostic is also in [PR comment 6419187197](https://github.com/RUdolf1517/tzim/pull/73#issuecomment-6419187197); evidence artifact retention is 90 days.

**Verdict: red. No writer change is made.** The run supplies concrete information, but the observed errors do not yet isolate a confirmed defect in `tzim`. The literal diff is retained, not normalized.

## Gate implementation and provenance

- `.github/scripts/fits-heasarc.py`: independent FITS fixture writer/reader and the real-tool harness. The header comparison is now a sequence diff of complete 80-byte cards, including card order, comments, padding, and `END`; it compares original HDUs with the HDUs reconstructed by `funpack`.
- `.github/workflows/fits-heasarc.yml`: builds `fitsverify`, `fpack`, and `funpack` from CFITSIO commit `113781d8aa44` at `https://codeload.github.com/heasarc/cfitsio/tar.gz/113781d8aa44`, SHA-256 `cc5f1cc219b0de8f8d8500236b080bff13d2de12b9ed253c7a417b1b7d962509`, checked with `sha256sum -c --strict`. The build installs ELF executables, not libtool wrappers. The failed earlier `-O` calls are fixed: in fpack/funpack `-O` names an output; it does not mean “overwrite.”
- No `continue-on-error`; the FITS gate remains red. No warning allowlist was introduced. Inputs, tzim outputs, tool output, literal diffs and controls are in the 90-day checksummed artifact.

The first three harness defects were: relative binary resolution from fixture directories; installing libtool wrapper scripts; and misusing `-O` as overwrite. Those were corrected and reported as red failures before the gate reached real verdicts.

## Actual header diffs — and what they do *not* attribute

The complete, card-for-card text is in the CI comment and artifact. It compares **original input to `funpack` reconstruction**; by itself it does not prove which program introduced a particular reconstructed-card difference. The raw tzim candidate is preserved separately for that reason.

The four fixtures with differences are `int16_primary`, `float32_specials`, `float64_extension`, and `double_ext`. The complete diff shows, among other exact card changes:

- `BITPIX` and `NAXIS` values are the same, but the reconstructed cards add comments (`/ array data type`, `/ number of array dimensions`) absent from the fixture cards.
- `XTENSION='IMAGE   '` is reconstructed as `XTENSION='IMAGE' / Image extension`; `PCOUNT` and `GCOUNT` also acquire comments.
- Some primary reconstructions drop `EXTEND`.
- The reconstructed headers contain `CHECKSUM`/`DATASUM` cards with CFITSIO update timestamps that were absent from the original cards. The diff does **not** establish these as changes made by `tzim`; the producer has not been isolated.

**Correction to the previous report:** the actual literal reconstructed-header diff does **not** show that `EXTNAME='COMPRESSED_IMAGE'` is the only difference. That earlier hypothesis is withdrawn. The report now names every raw 80-column card difference and does not normalize or excuse it. The `funpack` round-trip still fails the literal-header condition; no tzim writer fix is justified from these differences until their producer and contract treatment are established.

## multi_hdu — baseline and candidate diagnosis

The fixture’s malformed table width was found by running `fitsverify` on the **original**: it had `NAXIS1=16` with one `1PB` column (8-byte descriptor). That fixture-only error is corrected to `NAXIS1=8`; a self-check asserts the one-column width. The gate has not exempted the fixture or relaxed the verifier.

On the corrected original, `fitsverify` exits **1** because `XTENSION='HDRLET'` is reported as **“Unregistered XTENSION value”**. It continues through the remaining image and binary table. The unknown `HDRLET` is deliberately part of the contract’s “other HDUs copied verbatim” coverage.

On the tzim candidate:

- The harness’s independent parser sees five HDUs, with `HDRLET` at byte offset 14,400 and its 800-byte payload at offset 17,280.
- `fitsverify` sees only the empty primary and compressed primary table, then exits **1** with `Character 9 in this keyword is illegal. Hex Value = 1`; the displayed `^A^B^C^D` sequence is the exact `HDRLET` fixture payload.
- `funpack` exits **207** (`BAD_KEYCHAR`) with the same payload bytes and says it failed while processing HDU 2.

The corrected input itself is already rejected for the nonstandard `HDRLET`; the candidate failure is still recorded and the release gate remains red. The report does not yet prove whether this is a CFITSIO interaction with the copied nonstandard HDU after a compressed primary or a defect in tzim’s candidate layout. No writer change is made without that isolation. The original and candidate layouts, raw files, full `fitsverify` outputs, and full `funpack` output are retained in the artifact.

## zero-width column — explicit input and unresolved contract interaction

Exact fixture:

- HDU 0: empty primary, `BITPIX=8`, `NAXIS=0`.
- HDU 1: `IMAGE`, `BITPIX=16`, `NAXIS=2`, axes `32 × 64`, `EXTNAME='SCI'`.
- HDU 2: `BINTABLE`, `BITPIX=8`, `NAXIS1=8`, `NAXIS2=2`, `TFIELDS=2`, `TFORM1='0A'` (zero-width), `TFORM2='8A'`, `PCOUNT=0`, `GCOUNT=1`, 16 data bytes.
- Input and unchanged output are 17,280 bytes; input SHA-256 `57eb1edaadd40271e5e4a75e07c73d38008854fd235ae789fad1b1db4b58b3cf`; `fitsverify` exit 0.

The two §4.12 clauses interact: an eligible image HDU may be tiled and other HDUs copied verbatim, **but the whole-file candidate is written only if it is strictly smaller**. tzim reports `already_optimal`, no note, and leaves this file unchanged. The discarded candidate’s size is not exposed, so the gate does not assert either unconditional partial rewrite or a special whole-file refusal caused by the zero-width column. The former `expect='mixed'` test was too strong; it is now recorded as **undecided** pending owner agreement. Writer behavior is unchanged.

## Controls: two negative, one positive sanity check

There are **three entries** under `summary.controls`, but only **two negative controls**. The third is a positive toolchain sanity check; it is not expected to fail.

| Control | Role and expected result | Actual result on the latest review |
|---|---|---|
| Flip one bit in a candidate tile | Negative: intact tile inflates to the source array; corrupted tile must differ or fail to inflate | **Detected.** Intact comparison true; corrupted tile did not inflate (`invalid bit length repeat`); `fitsverify` exit **0**. This tests the harness’s content comparison, not that fitsverify rejects valid FITS structure with altered payload. |
| Set `NAXIS1=16` while `TFORM1` describes 8 bytes | Negative: `fitsverify` must exit nonzero | **Detected; exit 1.** It reports the sum of column widths is 8, not 16. |
| `fpack`-produced compressed file | Positive sanity check: `fpack` exits 0 and its output passes `fitsverify` | **Passed; fpack 0, fitsverify 0.** |

Thus the earlier phrase “three controls that must go red” was wrong. The two negative controls are supposed to be detected; the positive `fpack` check is supposed to stay green.

## Tests, scope, and open decisions

- `fits-heasarc.py --self-check`: **0 problems**, including the reader, full-card/order diff checks, fixture layout, tile reader, and structural control construction.
- `.github/scripts/test_measure.py`: 10 tests pass locally (the #72 tests and three aggregate/source-record tests).
- CI gate: red on the current SHA; not a merge requirement and not a release approval.
- No product documentation changed; no Release created; no issues closed.

Open before any writer fix or release decision:

1. Is the nonstandard `HDRLET` input in the accepted subset for the HEASARC gate, and should CFITSIO’s inability to traverse the candidate after it block release? The original and candidate failures are now separately recorded.
2. Which exact differences in the reconstructed headers are attributable to `funpack`, and what does the literal-diff release criterion require for those cards?
3. Does the zero-width-column fixture require per-HDU rewriting when the whole-file candidate is not strictly smaller?

**What this does not prove:** it does not certify FITS conformance beyond the tested shapes, establish that `funpack`-reconstructed checksum/comment changes are caused by tzim, or prove that any green run over these synthetic fixtures covers archive survey FITS. No writer patch has been made because no writer-specific cause is confirmed yet.
