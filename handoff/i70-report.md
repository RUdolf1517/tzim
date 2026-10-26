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
