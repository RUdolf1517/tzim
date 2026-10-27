# #70 — implementation report (corrected)

*Corrections to the previous revision are marked **[corrected]**. Nothing in this file is a claim about a
release: the handler is **implemented in `main`, not released**, and the `fitsverify` gate in
`fits-audit` §8.3 is untouched.*

## Where it lives

| | |
| --- | --- |
| Merged in | PR #69, merge commit `5dd45b3` on `main` |
| Branch head before the merge | `09553fe` |
| Release status | **implemented in `main`, not released** — [corrected] |

## CI evidence, per job **[corrected]**

The previous revision said "30/30" and then "21/21". Both were **snapshots of a moving list**, not summaries of
a commit. The `commits/<sha>/check-runs` endpoint merges the check runs of every workflow — `ci`, `lockfile`,
`corpus`, `fuzz`, `third-party`, `bench` — and those workflows do not start at the same moment: `bench` and
`third-party` are also **`schedule`-triggered**, so they attach to whatever the default branch head is when
their cron fires, which was this commit. A poll taken before those runs register returns a smaller number, and a
poll taken while one of their jobs is running returns it with `conclusion: null`.

Counted from `actions/runs/<id>/jobs` for each run whose `head_sha` is the commit, at **2026-10-26T12:52Z**:

### `5dd45b3` — `main` after the merge: 4 runs, 21 jobs, all `success`

| Run | Workflow | Event | Jobs | URL |
| --- | --- | --- | --- | --- |
| 40316986412 | `ci` | push | 18 | https://github.com/Rudolf1517/tzim/actions/runs/40316986412 |
| 40316986199 | `lockfile` | push | 1 (`generate`) | https://github.com/Rudolf1517/tzim/actions/runs/40316986199 |
| 40323158511 | `bench` | schedule | 1 (`comparisons`) | https://github.com/Rudolf1517/tzim/actions/runs/40323158511 |
| 40324343584 | `third-party` | schedule | 1 (`independent decoders`) | https://github.com/Rudolf1517/tzim/actions/runs/40324343584 |

The 18 jobs of the `ci` run, each `success`:

`audit` ([121551948194](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948194)),
`binary size` ([121551948552](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948552)),
`cargo package` ([121551948386](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948386)),
`clippy` ([121551947785](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551947785)),
`deny` ([121551948673](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948673)),
`docs (links and anchors)` ([121551948436](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948436)),
`fits oracle (astropy)` ([121551948180](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948180)),
`fmt` ([121551949297](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551949297)),
`install.sh (ubuntu-latest)` ([121551948461](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948461)),
`install.sh (macos-latest)` ([121551948666](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948666)),
`minimal (png only)` ([121551948322](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948322)),
`msrv (1.88)` ([121551948405](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948405)),
`test (ubuntu-latest, all-features)` ([121551948507](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948507)),
`test (ubuntu-latest, default)` ([121551948621](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948621)),
`test (macos-latest, all-features)` ([121551948475](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948475)),
`test (macos-latest, default)` ([121551948753](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948753)),
`test (windows-latest, all-features)` ([121551948787](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948787)),
`test (windows-latest, default)` ([121551948520](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948520)).

### `09553fe` — the branch head the merge accepted: 6 runs, 40 jobs, all `success`

| Run | Workflow | Event | Jobs |
| --- | --- | --- | --- |
| 40315854918 | `ci` | pull_request | 18 |
| 40315849821 | `ci` | push | 18 |
| 40315854857 | `fuzz` | pull_request | 1 |
| 40315854850 | `corpus` | pull_request | 1 |
| 40315854752 | `third-party` | pull_request | 1 |
| 40315850025 | `lockfile` | push | 1 |

So: **19/19** was `ci` (18) + `lockfile` (1) at a moment when the four path-filtered workflows had not yet
started; **21/21** was those nineteen plus `bench` + `third-party` scheduled runs that had appeared since, with
`bench`'s `comparisons` still in progress and therefore carrying `conclusion: null`. The settled figures are
**21 jobs on `5dd45b3`** — 18 in `ci`, one each in `lockfile`, `bench` and `third-party` — and **40 jobs on
`09553fe`**. The number that matters for the merge is that every job on the merged commit is green. "21/21" is a
statement about a commit and a moment, and it should have been written from `actions/runs`, not from
`check-runs`, which mixes workflows.

## Implemented in main, not released

**Implemented in `main`, not released.** [corrected] The handler is behind two doors, both shut by default: the
`fits` cargo feature (not in `default`) and `--fits-tile`.

An uncompressed image HDU becomes the tile-compressed `BINTABLE` `fpack` writes — `ZIMAGE = T`,
`ZCMPTYPE = GZIP_1` for integers and `GZIP_2` for floats, `ZTILE1` the whole first axis with `ZTILEn = 1`
(tile *i* is row *i*), quantisation off and readable as `ZNAME1 = 'NOISEBIT'`, `ZVAL1 = 0`. A compressed
primary array gets an empty `PrimaryHDU` in front of it. Every other HDU is copied verbatim, `NonstandardExtHDU`
and `HDRLET` included. **Tier 1 is not implemented**, per the issue: recompressing existing `GZIP_1`/`GZIP_2`
tiles measured a median of 0.00 % ([fits-audit §8.2](https://github.com/RUdolf1517/tzim/blob/5dd45b3/docs/fits-audit.md#82-the-result)).

## Checklist, item by item

| # | Item | Where it landed |
| --- | --- | --- |
| 1 | Contract clause list | [guarantees §4.12](https://github.com/RUdolf1517/tzim/blob/5dd45b3/docs/guarantees.md#412-fits--tile-compressed-image-hdus-v014-opt-in) — tiling rule, `ZNAME1`/`ZVAL1`, `CHECKSUM`/`DATASUM` refused rather than mirrored |
| 2 | Verification independent of the writer | each tile is a gzip member: `flate2` inflates, `crc32fast` recomputes the trailer's CRC-32, `ISIZE` checked — [`gunzip_member`](https://github.com/RUdolf1517/tzim/blob/5dd45b3/src/formats/fits.rs#L590); arrays compared **byte for byte** in [`check_tiled`](https://github.com/RUdolf1517/tzim/blob/5dd45b3/src/formats/fits.rs#L860) |
| 3 | Detection by content | 2880-byte records, `SIMPLE = T` at byte 0, bounded header scan — [`scan`](https://github.com/RUdolf1517/tzim/blob/5dd45b3/src/formats/fits.rs#L389) |
| 4 | Refusal list | `GROUPS`, scaled integers (`BSCALE`/`BZERO`, incl. `BSCALE = 0`), `CHECKSUM`/`DATASUM`, zero-width columns, `RICE_1`/`PLIO_1`/`HCOMPRESS_1`, unknown `ZCMPTYPE`, non-gzip tiles, heap descriptors outside the heap, any candidate not strictly smaller — [`decide_image`](https://github.com/RUdolf1517/tzim/blob/5dd45b3/src/formats/fits.rs#L474) |
| 5 | Fixture corpus | FITS writer in [`tests/common/mod.rs`](https://github.com/RUdolf1517/tzim/blob/5dd45b3/tests/common/mod.rs#L1754); edge cases built by it — see deviation 1 |
| 6 | Tests | 12 unit tests in [`fits.rs`](https://github.com/RUdolf1517/tzim/blob/5dd45b3/src/formats/fits.rs#L1133); 9 integration tests in [`integration.rs`](https://github.com/RUdolf1517/tzim/blob/5dd45b3/tests/integration.rs#L3480) |
| 7 | Measured size delta | **+36 864 B (36 KiB)** on `default` and on `--all-features`; [size-budget §1](https://github.com/Rudolf1517/tzim/blob/5dd45b3/docs/size-budget.md#1-budget), [§3](https://github.com/Rudolf1517/tzim/blob/5dd45b3/docs/size-budget.md#3-feature-matrix); `binary size` job [121551948552](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948552). The issue guessed 60–100 KiB |
| 8 | Dependency review | **zero new crates** — `flate2` already in the tree, `crc32fast` already a `jpeg-v1` dependency ([dependencies §2.11](https://github.com/RUdolf1517/tzim/blob/5dd45b3/docs/dependencies.md#211-fits-no-cfitsio-and-why-that-is-not-a-gap)) |
| 9 | Docs | `README`, `formats` §5, `--list-formats`, completions, `cli.md`, `man/tzim.1:101`, `CHANGELOG ### Added` |
| 10 | Expectations | 13 of 151 files grow; handler keeps the smaller bytes, reports `already_optimal` |

## The tests, by name

Unit (`cargo test --lib fits::tests`): `an_image_hdu_becomes_a_compressed_table_that_round_trips`,
`a_float_array_is_shuffled_and_round_trips`, `without_the_flag_nothing_is_written`,
`the_shuffle_is_the_permutation_the_convention_names`, `a_candidate_that_would_grow_is_discarded`,
`a_checksum_keeps_the_hdu_verbatim`, `a_random_groups_primary_is_refused`, `a_scaled_image_is_refused`,
`a_zero_scale_is_refused`, `a_truncated_file_is_a_parse_failure_not_a_rewrite`,
`a_header_lookup_finds_the_first_card_of_its_keyword`, `cards_parse_their_values`.

Integration (`cargo test --test integration fits_`):
`fits_an_image_hdu_becomes_a_compressed_table_that_round_trips`,
`fits_a_float_array_uses_gzip_2_and_round_trips`, `fits_every_other_hdu_is_copied_verbatim`,
`fits_does_nothing_without_the_flag`, `fits_an_unsupported_tile_codec_is_skipped`,
`fits_a_file_that_would_grow_is_left_alone`, `fits_a_checksummed_hdu_is_kept_and_the_rest_is_rewritten`,
`fits_the_two_shapes_the_reference_writer_lost_are_kept`, `fits_is_listed_and_can_be_named`.

The integration module is `#[cfg(feature = "fits")]`, so the six `test` legs run it only under `all-features`;
the `default` legs do not compile the handler at all, which is the point of #68.

## The oracle: astropy, not `fitsverify`

[`.github/scripts/fits-oracle.py`](https://github.com/RUdolf1517/tzim/blob/5dd45b3/.github/scripts/fits-oracle.py),
job `fits oracle (astropy)` ([121551948180](https://github.com/Rudolf1517/tzim/actions/runs/40316986412/job/121551948180)).
Own byte walker, so "copied verbatim" is checked on the bytes; every candidate read back through `astropy` 8.0.2
with CFITSIO underneath; the footer prints `fitsverify has not run`. Seven fixtures.

Asserted exactly: every array re-decoded (**float arrays through `equal_nan`, integer arrays as bytes** — see
the limitation below); named metadata keys survive; `ZIMAGE`, `ZCMPTYPE` against the sign of `ZBITPIX`,
`TTYPE1 = COMPRESSED_DATA`, `NAXIS1 = 8`; `ZNAXIS` against the original rank; every `ZNAXISn` against the
original shape; `ZTILE1` against the whole first axis, every `ZTILEn = 1`, `NAXIS2` against the product of the
remaining axes; non-rewritten HDUs byte-identical.

**What the float comparison does and does not prove [corrected].** `equal_nan=True` compares **values**, not
bytes: two NaNs are equal whatever their payloads, and `+0.0 == -0.0`. It therefore **cannot** show that a
NaN payload or the sign of a zero survived the round trip. Only the Rust verifier's byte comparison and the
`GZIP_2` shuffle test (`the_shuffle_is_the_permutation_the_convention_names`) say anything about those, and #71
adds explicit NaN-payload and signed-zero cases checked as bytes.

## Size, measured

| Build | Bytes | KiB | vs the row above |
| --- | --- | --- | --- |
| `default` (`…,tiff,zlib,zopfli`) | 979 808 | 956 | — unchanged, still exactly the gate |
| `default` + `fits` | 1 016 672 | 992 | **+36 864** |
| `--all-features` | 1 102 688 | 1 076 | — |
| `--all-features` + `fits` | 1 139 552 | 1 112 | **+36 864** |

Source: the `binary size` job's report on `09553fe` / `5dd45b3`. It measures the static musl release build only.

## Deviations, and what each one does not prove

1. **`zerowidth.fits` and `double_ext.fits` are generated, not committed.** *Does not prove* that those two
   actual corpus files round-trip or are refused — only that our writer's layout for the same shapes survives.
   There is nothing to diff against, so a difference between the real files and our reconstruction cannot be
   detected.
2. **`fitsverify` has still not run anywhere.** *Does not prove* conformance. astropy/CFITSIO proves a second
   implementation reads our table and the arrays re-decode. It does not prove `CHECKSUM`/`DATASUM` correctness
   (we refuse those HDUs), does not prove HEASARC's structural vetting passes, and does not satisfy the release
   gate in `fits-audit` §8.3 / `guarantees` §4.12.
3. **The 21.9 % median is a measurement of one corpus.** [corrected — the claim that it is a "floor" is
   withdrawn: a lower bound is not established by measuring a corpus that omits large frames.] 122 of the 178
   files are 10×10–100×100 cutouts and the `astropy` subset is **−7.1 %**, so the number describes files of
   that size and no other. It is not a survey measurement and it does not license re-using 8 %/15 % as a fresh
   pre-registration.
4. **Tier 1 is not implemented.** *Does not prove* no gain exists in already-compressed tiles — only that the
   median on *this* corpus, where they sit near level 6, is 0.00 %.
5. **The size figure is one target.** +36 864 B is the static musl release build; no other target is measured by
   that job.

## Known gaps

- The oracle does not assert `ZSIMPLE` / `ZTENSION`: a candidate marking a primary array as an extension (or the
  reverse) would pass today.
- The oracle's structural checks cover only fixtures listed in its `tables`/`arrays` lists.
- `tests/common` `FitsHdu.cards` records the whole 2880-byte record containing `END`, i.e. cards past `END`.
  No current assertion reads them; it is a trap for the next one.

## Not in this issue

The `fitsverify` release gate is #71; reproducible measurement provenance is #72. Neither is satisfied by this
merge, and neither #59 nor #70 is closed by it.


---

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


---

# #72 — measurement reproducibility: updated formula audit and source rows

Branch `arena/7ded2747-tzim`, PR **#73** (open). Latest measurement review is on commit `5c2f4a2`: run [40344219423](https://github.com/RUdolf1517/tzim/actions/runs/40344219423), job [121646706476](https://github.com/RUdolf1517/tzim/actions/runs/40344219423/job/121646706476). CI comment: [6419206909](https://github.com/RUdolf1517/tzim/pull/73#issuecomment-6419206909). Evidence is retained for 90 days.

**Old rounded values are comparison targets, not assumed truth.** The first replay exposed four differences; investigating the definitions resolved the two FITS per-corpus medians as an even-sample median implementation error. Two PDF differences remain red and unresolved.

## Reproducible acquisition and environment

The one acquisition path remains `.github/scripts/measure-fetch.py`: seven codeload archives at full commit SHAs, SHA-256 checked, a per-file size/SHA-256 manifest for **2,386 files** (178 FITS, 2,208 PDF), and `--verify` immediately before measuring. Manifest digest: `57e18a9d97d70e4c9e8fd919546c537fea79f9d30e42437c8b4d409fd027ecc5`.

Those archive checksums are a **new baseline**, not a recovered historical checksum and not an immutability claim. The manifest records the 11 uppercase `.Pdf` exclusions rather than quietly counting them. No PDF from the exact real toolchain sample and no full-size archive survey FITS product were added. The existing 8%/15% thresholds are not re-registered. No Release was created.

The CI environment uses Python 3.12.15 and hashed wheels; package versions include NumPy 2.4.6, Astropy 8.0.2, pikepdf 10.17.0/libqpdf 12.4.2, and PyMuPDF 2.0/MuPDF 1.28.5. Commands, versions, script hashes, OS/runner and checksums are recorded in the evidence bundle.

## Four metrics: definitions, denominators, formulae, and source records

### 1–2. FITS per-corpus medians (`astropy`, `reproject`)

Per-file value: `pct = 100 * (bytes - new_size) / bytes`. The median population is only rows with `status=tier2` **and** `exact=true`; failed-verification rows remain in the source file but are marked `included_in_median=false`. For even `n`, the median is the arithmetic mean of the two central sorted values. Quartiles retain the instrument’s `int(p*n)` index rule.

The first replay used the upper middle element `sorted[int(0.5*n)]` for even `n`; that is not the conventional median and caused the apparent mismatches. A regression test now checks the average-of-middle-values rule.

| Corpus | `n` used | Central source rows | Recomputed median | Published rounded value |
|---|---:|---|---:|---:|
| `astropy` | 12 | `checksum_false.fits`: 20,160→23,040 bytes, −14.285714%; `compressed_float_bzero.fits`: 8,640→8,640 bytes, 0% | **−7.142857%** | −7.1% |
| `reproject` | 16 | `test_reproject_adaptive_2d.fits`: 63,360→37,440 bytes, 40.909091%; `test_reproject_celestial_2d_gal2equ.fits`: 40,320→23,040 bytes, 42.857143% | **41.883117%** | 41.9% |

These current-source medians reproduce the rounded values under the conventional formula. This does **not** establish that the historical run used exactly these rows: no per-file JSONL from the original audit was preserved.

### 3. PDF share with positive faithful recompression

For each accepted PDF:

`faithful_pct = 100 * (F - F9) / S`

where `S` is whole-file bytes; `F` is the sum of original encoded byte lengths of in-scope Flate streams; and `F9` is the sum per stream of `min(original encoded length, zlib level-9 length after inflating the stream)`. A file counts positive iff `faithful_pct > 0`.

Current result: **700 / 1,009 = 69.375619%**, with 309 exactly zero and 0 negative. The old publication says 68%; its original numerator and source rows are unavailable, so we cannot identify which files differ or declare either result correct. This remains a red discrepancy; no formula or threshold was adjusted to force agreement.

### 4. PDF render-page count

The run selects `random.sample(accepted_rows, min(140, n))` after `random.seed(57)`, from manifest order. Original and allowed-recompression candidates are rendered by PyMuPDF at 100 dpi; at most the first six pages per PDF are compared. The reported page metric is the **sum of pages actually compared** (`min(document pages, 6)` for each successful sample row), not the sum of full document lengths.

Current source rows: 140 selected, 140 rendered, **168 compared pages**, zero differences, zero render errors. The old report says 173 pages but preserved no sample-path list, so it cannot be asserted to be the same 140 documents. The 168-vs-173 discrepancy remains red and unresolved; it is not attributed to library version or treated as agreement.

## Machine-readable source records

The current 90-day CI artifact includes and checksums:

- `fits-corpus-median-source.jsonl` — all Tier 2 `astropy`/`reproject` rows, with bytes, new size, percentage, manifest SHA, and inclusion flag;
- `pdf-share-positive-source.jsonl` — all 1,009 accepted PDFs with the per-file numerator inputs, percentage, manifest SHA, and positive flag;
- `render-sample.jsonl` — all 140 selected relative paths and manifest SHAs, page counts, compared-page counts, per-page image SHA-256s, and equality result;
- full `fits.jsonl`, `pdf.jsonl`, `aggregates.json`, `metric-definitions.md`, environment/commands, and `SHA256SUMS`.

CI bundle digest: `8d45bc4134c9c783488d290815a9051e0cfb370a73b022facbbdc47cf6a7cf5a`. I also assembled a downloadable local source-record bundle from the same manifest baseline for this handoff; its file list and checksum are included with the attachment. The CI bundle is authoritative for the CI run’s exact render selection.

## Test and release status

The #72 historical negative controls remain in `test_measure.py`; three additional tests guard even-count medians, share numerator/denominator, and per-file render aggregation. **10 tests pass** locally. The CI measurement job is red only on `share_positive` and `render_pages`; the FITS corpus aggregates reproduce under the corrected median definition. This does not make the old publication ground truth.

No product documentation changed, no issues were closed, no criteria were weakened, and no GitHub Release was created.


---

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
