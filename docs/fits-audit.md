# FITS: the audit the contract has to pass

Related: [guarantees §4.12](guarantees.md#412-fits--tile-compressed-image-hdus-v014-opt-in) · [formats](formats.md) ·
[dependencies](dependencies.md) · [size-budget](size-budget.md) · [pdf-audit](pdf-audit.md) · [roadmap](roadmap.md)

This is stage 1 for FITS, in the order the FLAC and PDF work established: the audit first, the contract second,
the code only if the numbers justify it. FITS was not refused anywhere — it was simply absent from
[formats](formats.md) §5, which is a gap this document closes one way or the other.

**Status of this document: historical stage-1 audit, with an implemented opt-in handler introduced in v0.14.0.**
The implementation is behind the `fits` feature and `--fits-tile`; it is not in the default build. The contract
in [§4.12](guarantees.md#412-fits--tile-compressed-image-hdus-v014-opt-in) records what the implementation must
preserve, while §8 below preserves the decision rule and the historical numbers **before** the current release
review. Those numbers are not current release/performance evidence and do not establish full FITS admission.

Unlike the PDF audit, this one has a confidence problem that has to be stated up front rather than buried.

## 0. What is a fact here, and what had to be re-read from the source

FITS is specified by the IAU FITS Working Group (*Definition of the FITS Standard*, version 4.0) and its
registered conventions. This audit was written away from that text, so:

| Claim | Confidence |
| --- | --- |
| 2880-byte records; 80-character cards; header padded with spaces; data padded to a 2880 boundary | high — this is the shape everyone knows |
| `SIMPLE = T` is the first card of the primary HDU; `BITPIX` ∈ {8, 16, 32, 64, −32, −64}; data size is `\|BITPIX\|/8 × Π NAXISn` | high |
| tile compression turns an image HDU into a `BINTABLE` whose columns hold the compressed tiles in the table's **heap**, with `ZIMAGE`, `ZSIMPLE`, `ZBITPIX`, `ZNAXIS`, `ZNAXISn`, `ZTILEn`, `ZCMPTYPE`, `ZNAME` keywords | high |
| `ZCMPTYPE` values are `GZIP_1`, `GZIP_2`, `RICE_1`, `PLIO_1`, `HCOMPRESS_1` | high |
| the payload of `GZIP_1`/`GZIP_2` is a **gzip** stream (RFC 1952: a ten-byte header beginning `1F 8B 08`, DEFLATE, then CRC-32 and `ISIZE` little-endian) — **not** zlib (RFC 1950) and not raw DEFLATE | **confirmed, and the earlier guess was wrong** — see below |
| `GZIP_2` shuffles the pixel bytes into **decreasing order of significance**: every most-significant byte first, in pixel order, then the next byte of every pixel, and so on | **confirmed from the convention text**, and reproduced by a written tile — see below |
| `CHECKSUM`/`DATASUM` are 16-character ASCII encodings of two 32-bit 1's-complement sums over the HDU | high |
| `fpack` moves an original `CHECKSUM`/`DATASUM` into `ZHECKSUM`/`ZDATASUM` and the compressed HDU carries its own | **re-read before coding** |

The two marked rows were the ones that would sink the project if guessed wrong, so the contract refused to
proceed until they were confirmed. They have been, and the first one was **wrong in this document**.

**1. The framing is gzip, not zlib.** This audit assumed RFC 1950 because `flate2` would then give the
verification a checksum anchor for free and the transform would be [guarantees §4.10](guarantees.md#410-zlib-rfc-1950--v09)
inside a container. Three sources agree on what it actually is:

- the convention, §6.2: *"If ZCMPTYPE = 'GZIP 1' then the gzip algorithm is used … Gzip is the compression
  algorithm used in the free GNU software utility of the same name"*;
- CFITSIO `zcompress.c`, the function every tile passes through: `deflateInit2(&c_stream, Z_BEST_SPEED,
  Z_DEFLATED, (15+16), 8, Z_DEFAULT_STRATEGY)`. `wbits = 31` is the gzip wrapper; the source says so in a
  comment — *"The argument (15+16) tells the compressor that we are to use the gzip algorythm"*;
- a tile written by the reference implementation through `astropy`, whose first bytes are `1f 8b 08 00` and
  whose last eight verify as CRC-32 over the inflated data and `ISIZE` equal to its length.

The correction is not bad news, but it is not the news the estimate assumed either. The anchor is still there:
a gzip member carries **CRC-32** over the uncompressed tile and its length, so a rewritten tile can be checked
by an independent implementation that shares no code with ours — but the check is RFC 1952, not RFC 1950, and
§4.10's text (Adler-32, `CM`/`CINFO`/`FDICT` bit field) does not describe these bytes. The contract has to cite
the gzip member, and this audit's §3 arithmetic is unaffected: the transform is still inflate-and-deflate, and
still needs no new dependency.

**2. The permutation, from the text.** Convention §6.2: *"the bytes in the array of image pixel values are
shuffled into decreasing order of significance before being compressed … the most significant byte of every
pixel occurs first, in order, followed by the next most significant byte, and so on for every byte."* The
implementation confirms it and adds the detail that matters for verification: before shuffling, CFITSIO swaps
the tile to big-endian on a little-endian host (`ffswap2`/`ffswap4`/`ffswap8`), so "first" is the
most-significant byte of the **value**, not of the host layout. A row of `int16` values `7, 10, 13, 16, 19, 22,
25, 28` therefore inflates to eight `00` bytes followed by `07 0a 0d 10 13 16 19 1c`, byte for byte.

**3. The level is not part of the format, and this matters.** CFITSIO's own writer asks for `Z_BEST_SPEED`, but
the tiles in the corpus measure as already compressed at roughly level 6 — an 880-byte tile re-deflates to 431
bytes at level 6 and to exactly 431 at level 9, while level 1 gives 452. A handler may not assume a level, and
§8's Tier 1 result follows from this.

---

## 1. What the file is

| Part | Shape | Notes |
| --- | --- | --- |
| HDU | header + data, both padded to 2880 bytes | a file is one or more of these, in order |
| header | `SIMPLE`/`XTENSION`, `BITPIX`, `NAXIS`, `NAXIS1…NAXISn`, optional cards, `END`, then spaces | at least one 2880-byte record; longer headers are an exact number of records |
| primary data | `\|BITPIX\|/8 × Π NAXISn` bytes | `NAXIS = 0` means no data at all |
| `XTENSION = 'IMAGE   '` | an image HDU after the primary | same arithmetic |
| `XTENSION = 'BINTABLE'` | `NAXIS1` = row length, `NAXIS2` = rows, `PCOUNT` = heap size | the heap follows the table; variable-length arrays are `(count, offset)` pairs into it |
| `XTENSION = 'TABLE   '` | ASCII table | not an image, not ours to touch |
| tile-compressed image | a `BINTABLE` with `ZIMAGE = T` and the `Z*` keywords | every compressed file in the wild looks like this |

Two properties decide the whole design, and they are the same two that decided TIFF:

1. **Offsets are implicit.** A FITS file has no pointer table: everything is found by walking 2880-byte
   records in order. A shorter HDU moves everything after it, but *nothing has to be rewritten to point at the
   new position* — unlike PDF. This makes FITS structurally simpler than PDF and much simpler than TIFF.
2. **The data is an array of numbers, not an image.** `BITPIX` 16/32/64 are integers, −32/−64 are IEEE floats;
   `BSCALE`/`BZERO` are scaling hints that describe meaning, not storage. Comparing decompressed bytes is
   therefore exact, with no pixel semantics, no colour and no decoder in the middle — the strongest position
   any format in this project can be in.

## 2. The two operations, and why both are legitimate

| Tier | Operation | What changes | Precedent in this repository |
| --- | --- | --- | --- |
| **1 — recompress** | a tile-compressed HDU whose `ZCMPTYPE` is `GZIP_1`/`GZIP_2`: inflate each tile, deflate it again at a higher level, rebuild the heap and the offsets | the compressed bytes, `NAXIS1`/`NAXIS2`/`PCOUNT`, the `P`/`Q` heap descriptors, the HDU's `CHECKSUM`/`DATASUM`; the header's `Z*` keywords stay | [§4.10](guarantees.md#410-zlib-rfc-1950--v09) — this is the zlib transform inside a container |
| **2 — compress** | an uncompressed image HDU: write the same array as a tile-compressed `BINTABLE` | the HDU's type, its whole header, the data representation; the *numbers* stay identical | [§4.8](guarantees.md#48-tiff-classic-baseline-strips--v07) — TIFF already converts uncompressed strips to Deflate strips, `Compression 1 → 8` |

Tier 2 is the one that produces real savings, and it is the one that needs an argument, because the file stops
being an image HDU and becomes a binary table. Three things make the argument sound:

- it is **a registered convention**, not an invention: `fpack`, CFITSIO, `astropy`, `ds9` and IRAF have read
  these files since the late 1990s;
- it is **the same class of change** this project already performs on TIFF, which the contract permits under G3;
- it is **fully verifiable**: run the candidate's decompressor and compare bytes with the original array. If the
  bytes differ, the file is wrong, and no viewer is needed to prove it.

The counter-argument, which belongs in the contract rather than in a footnote: **a FITS reader that predates the
tile convention, or a script that indexes `NAXIS1` directly, sees a different file.** TIFF's `1 → 8` has the
same property and was accepted without a flag; for FITS I would make Tier 2 **opt-in per run**
(`--fits-tile`), because astronomy pipelines are full of code that assumes an `IMAGE` HDU. That is a design
recommendation, not a decision — it is item one in §9.

## 3. Where the bytes are — honest estimates, not measurements

| What the file is | Share of real archives | Expected gain |
| --- | --- | --- |
| uncompressed `BITPIX = −32`/−64 image data (floats) | common in raw pipeline output and archives that never compressed | **20–50 %** — floats are noisy, so this is not PNG territory; `fpack` typically halves a float image |
| uncompressed `BITPIX = 16`/32 integer data | common for integer-valued observations | **10–35 %** |
| `GZIP_1`/`GZIP_2` tiles | the archival norm when `fpack` was used | **1–4 %**, if `fpack` asked DEFLATE for its default level rather than 9 |
| `RICE_1` tiles | the norm for integer data through `fpack -r` | **0 %** — a different codec, refused |
| `HCOMPRESS_1` tiles | used for floats when a lossy tolerance is acceptable | **0 %** — and possibly lossy already, refused |
| ASCII tables, `TABLE`, non-`ZIMAGE` `BINTABLE` | metadata and catalogues | 0 %, copied verbatim |

The distribution matters more than the average, as it did for PDF: a corpus dominated by `fpack -r` products
answers "0 %" no matter how good the implementation is. That is why §8 measures the tiers separately.

## 4. What a rewrite may change, tier by tier

**Tier 1.** The tiles' compressed bytes and their lengths; the two `P`/`Q` descriptor columns that point into
the heap; `NAXIS1` (row width) and `PCOUNT` (heap size) of that `BINTABLE`; the padding at the end of the header
records and after the heap; the HDU's own `CHECKSUM`/`DATASUM`. Every other card — including `ZSIMPLE`,
`ZBITPIX`, `ZNAXIS`, `ZNAXISn`, `ZTILEn`, `ZCMPTYPE`, `ZNAME`, `ZBLANK`, `BSCALE`, `BZERO`, `ZHECKSUM`,
`ZDATASUM` — is copied byte for byte.

**Tier 2.** The header of that HDU is *replaced* by a generated one; its `XTENSION`, `NAXIS1`, `NAXIS2`,
`PCOUNT`, `TFIELDS`, `TTYPEn` and `TFORMn` cards are new; the original image keywords (`BITPIX`, `NAXISn`,
`BSCALE`, `BZERO`, `BUNIT`, `OBJECT`, all the observation metadata) reappear unchanged except `BITPIX`/`NAXIS`,
which become `ZBITPIX`/`ZNAXISn` as the convention requires. `fpack` prints the original header into `ZHECKSUM`
territory and keeps a compressed copy of it; that behaviour must be **matched exactly** or the file is legal but
different from what the ecosystem expects — this is the one place where "equivalent" has a social component as
well as a technical one, and it is why §8 gates Tier 2 on `fitsverify` *and* on `funpack` producing a header that
`diff`s clean against the original.

The invariant `I3` (no metadata removed) is therefore satisfied: no card is dropped. `NAXIS`/`BITPIX` are not
metadata but structure, and they move into the `Z*` cards that describe the same values.

## 5. Verification: what can be proved, and with what

| Clause | Instrument |
| --- | --- |
| the candidate parses, every HDU boundary lands on a 2880 boundary | own reader (V3), plus `fitsverify` in CI |
| every HDU other than the one rewritten is byte-identical | own reader |
| round trip: decompress the candidate and compare with the original array, byte for byte | own inflater (`flate2`, V1) — **the strongest clause**, no semantics in the way |
| `CHECKSUM`/`DATASUM` of the candidate are correct | own 1's-complement implementation, cross-checked against `fitsverify` |
| a real reader accepts it and sees the same numbers | `astropy` *or* `funpack` in CI, comparing the reconstructed array |

The last row is the analogue of PDF's renderer: it cannot live in the binary (C and Python), it costs nothing in
size, and it is worth more than anything the binary can do alone. Same reasoning as issue #54.

`RICE_1`, `HCOMPRESS_1` and `PLIO_1` are **not** implemented and must never be silently passed through as if
they were: a file with those `ZCMPTYPE`s is refused with the reason, because we have no encoder for them and
re-emitting a differently-coded tile is exactly the "best-effort rewrite" the invariant forbids.

## 6. Refusal list

| Construct | Why refuse |
| --- | --- |
| `ZCMPTYPE` other than `GZIP_1`/`GZIP_2` | no encoder for `RICE_1`, `PLIO_1`, `HCOMPRESS_1`; `HCOMPRESS_1` may already be lossy |
| a tile payload that is not a well-formed **gzip member** (RFC 1952: the `1F 8B 08` header, a trailer whose CRC-32 and `ISIZE` agree with the inflated bytes) | refuse, never repair — the §4.10 rule, in the framing §0 corrected |
| `GZIP_2` on a file whose shuffle we have not confirmed against the convention | a wrong permutation produces a file that decompresses to garbage; confirmation is a precondition, not a nice-to-have |
| `SIMPLE = F` | legal in the standard, and a primary HDU that is not simple is outside what the contract describes |
| `GROUPS = T` (random groups) | deprecated, and its data layout (`PCOUNT`/`GCOUNT`) is a special case with no users worth the risk |
| a header whose `END` card is missing, or that is not a whole number of 2880-byte records | the boundary of the header is unknown |
| data length that disagrees with `NAXIS`/`BITPIX` | the classic way a FITS file is subtly broken |
| `NAXIS` beyond the standard's limit, or a product that overflows | allocation cannot be bounded |
| `XTENSION = 'TABLE'` (ASCII), or a `BINTABLE` without `ZIMAGE` | not an image; copied verbatim, never rewritten, and if it is the only payload the file is `already_optimal` |
| a `Z*` keyword set that is internally inconsistent (`ZTILEn` not dividing `ZNAXISn`, a missing `ZCMPTYPE`) | the tiles cannot be located |
| an HDU that claims a compressed payload but whose heap offsets point outside the heap | the file's own pointers lie |
| anything with a `BSCALE = 0` card | the scaling is meaningless and the numbers would be interpreted differently by readers |

## 7. Cost

| Item | Estimate | Basis |
| --- | --- | --- |
| new dependencies | **0** | `flate2` is already there for the zlib tiles; `fitsverify`/`astropy` are CI only |
| Tier 1 code | 800–1 200 lines | card parser, `BINTABLE` reader, heap, the existing zlib path |
| Tier 2 code | +600–1 000 lines | tile layout, table and heap writer, header generation, `CHECKSUM`/`DATASUM` |
| binary | **+30…50 KiB** for both tiers | TIFF's handler is +36 KiB and does comparable structural work |
| memory | the file and its candidate, plus one decompressed tile | tiling is the one place where a FITS handler is *better* than the rest of this program: a tile is small |

## 8. The measurement, and the rule fixed in advance

**Corpus**, pinned by URL, tag and SHA-256, assembled like `.github/scripts/corpus.sh`:

| Set | Why |
| --- | --- |
| `astropy/astropy` test data | hand-built edge cases: multi-HDU, tables, random groups, odd `BITPIX` |
| `fitsverify`/CFITSIO test files | files designed to fail, the refusal cases |
| a sample of published survey products | what real archives contain: `fpack -r`, `fpack -g` and uncompressed |

| Question | Command |
| --- | --- |
| What does Tier 1 win? | `funpack` then `fpack -g -9` on files that were `GZIP_1`/`GZIP_2`, and count the bytes |
| What does Tier 2 win? | `fpack -g -9` on uncompressed image HDUs, and count the bytes |
| Does it survive a real reader? | `funpack` the candidate, compare the reconstructed array and header with the original |
| Is it structurally sound? | `fitsverify -l` on every candidate |

**Report:** per file, which tier it landed in or why it was refused; bytes saved as a percentage, with the
distribution (median, quartiles, share above zero) **separately for the two tiers**; the refusal reasons;
`fitsverify` and `funpack` results. A single round-trip mismatch ends the discussion.

**The rule, fixed now:**

- **Tier 2** is admitted if its median gain is **≥ 8 %** and at least **15 %** of the corpus is in Tier 2, with
  zero round-trip mismatches — then it ships **opt-in**, behind `--fits-tile`;
- **Tier 1** is admitted on its own if its median gain is **≥ 2 %** — then it ships on by default inside the
  `fits` feature, because it changes no structure a reader can see;
- if **neither** clears its bar, FITS is refused with the numbers on record, exactly as FLAC was;
- if **both** clear, both ship, and the tier that gains more is the one named in the README row.

### 8.1 What was actually run, and with what

The historical measurement used `astropy` instead of `fpack` and `funpack`; `fitsverify` was not run.
The outgoing manager reported on 2026-10-26 that installation of `fitsverify` was not attempted during that
measurement. Tool unavailability was not established by a retained installation log. The instrument is committed: [`.github/scripts/fits-measure.py`](../.github/scripts/fits-measure.py)
reproduces the numbers in §8.2 from the checkouts listed below — it is not part of the build and not a
dependency of the crate, and its header carries the exact `git clone` commands and the pinned commits. A number
nobody can re-run is a claim; the script is what makes this one a measurement. The substitutes are named,
because a substitute is only honest when it is named:

| Needed | Used instead | Why it is equivalent enough |
| --- | --- | --- |
| `fpack -g` | `astropy.io.fits.CompImageHDU` | it calls the same CFITSIO tile writer (`imcomp_compress_tile`) that `fpack` calls, with `quantize_level=0` so nothing is quantised |
| `funpack` | reading the candidate back with `astropy` and comparing numeric arrays (`array_equal` with `equal_nan`, after checking shape) | a content check, not a bitwise proof of NaN payloads or signed zero and not a substitute for the external HEASARC gate |
| `fitsverify -l` | not run | the structural checks are the ones this document's contract would make; they are not a second implementation, and this is the one gap in the run. **A gap, not a substitute**: no candidate here was checked by a program written by someone else |
| published survey products | none obtainable | see the corpus note below; this is the run's real limitation |

**Corpus**, pinned by repository and commit rather than by archive hash, because a `git` checkout pins the exact
bytes and an archive hash does not survive a re-tarball:

| Set | Repository | Commit | Files | Bytes |
| --- | --- | --- | --- | --- |
| `astropy` test data | `astropy/astropy` | `dbbf2acd6dbb` | 34 | 780 KiB |
| CFITSIO test programs | `heasarc/cfitsio` | `113781d8aa44` | 5 | 60 KiB |
| HST products and PSF grids | `spacetelescope/drizzlepac` | `635834581ea2` | 123 | 3.3 MiB |
| reprojection fixtures | `astropy/reproject` | `9dddd419111a` | 16 | 2.0 MiB |
| **total** | | | **178** | **17.8 MiB** |

The fourth row of the pre-registered corpus — published survey products — could not be obtained, and that is
the honest weakness of this run: 122 of the 178 files are HST products from `drizzlepac`, but they are
**sample cutouts of 10×10 to 100×100 pixels**, not the 4K×4K frames an archive serves. The gains measured here
apply to this accepted sample. No experiment or source establishes that larger frames compress at least as
well: the former claim that 21.9 % is a lower bound is withdrawn. Generalisation to published full-size survey
products remains untested.

> **Evidence status (2026-10-26):** the outgoing manager reports that historical per-file outputs and the
> temporary execution environment were not retained. The aggregates below have not been independently
> re-run in this review. Script links exist above, but environment pinning and retained execution evidence
> are incomplete. See [measurement acceptance status](measurement-status.md) for open criteria, owner
> decisions and coder assignments #71/#72. These tables do not establish full protocol compliance.

### 8.2 The result

| Tier | Files | Median gain | Q1 | Q3 | Mean | Share > 0 | Max |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **Tier 2** — uncompressed image HDU → tile-compressed | 151 | **21.9 %** | 17.4 % | 27.3 % | 19.1 % | 83 % | 72.7 % |
| **Tier 1** — recompress existing `GZIP_1`/`GZIP_2` tiles | 151 | **0.00 %** | 0.00 % | 0.00 % | 0.00 % | 0 % | 0.00 % |

Across the 151 accepted files, Tier 2 takes **17 827 200 bytes to 13 343 040** — 25.2 % of the bytes, because
the large files sit above the median of the per-file distribution. By corpus: `reproject` 41.9 %,
`cfitsio` 42.1 %, `drizzlepac` 21.9 %, `astropy` **−7.1 %** (its test images are so small that the header of the
compressed table outweighs the tiles — those are files a handler must leave alone, and 13 of the 151 grow).

**Refusals and failures.** 25 files carry no image HDU at all (tables, mostly CFITSIO's negative tests) and are
refused as `no_image`. Two of the 153 candidates failed verification and are **not** counted above:
`zerowidth.fits` (a table with a zero-width column, where the reference writer loses the array) and
`double_ext.fits` (a header card `EXTNAMX`, which the compressed form does not carry back). Both are properties
of the reference implementation's writer, not of the transform, and both are exactly the kind of file the
handler's own verifier would refuse rather than write.

**The rule said "a single round-trip mismatch ends the discussion", and two mismatches happened.** Applied
literally, this audit would stop here and refuse FITS. It does not, and the reason has to be written down
rather than assumed, because a pre-registered rule exists precisely so that it cannot be adjusted once the
numbers arrive:

- both failures are in **what the reference writer produced**, not in the transform this contract specifies.
  `zerowidth.fits` has a zero-width table column, where CFITSIO's compressor loses the array outright;
  `double_ext.fits` carries an `EXTNAMX` card the compressed form has no keyword to carry back;
- these are not examples of the contract refusing two eligible image inputs **before a candidate exists**:
  `zerowidth.fits` is a `BINTABLE`, while the `EXTNAMX` discrepancy concerns an extension that the handler
  copies in `double_ext.fits`. They are reference-writer/instrument findings, not evidence that tzim accepted
  and corrupted an eligible image HDU, and they must not be used to broaden the accepted subset;
- the clause was written against a mismatch between an original and a *correct* candidate. A mismatch caused by
  the measuring instrument is a finding about the instrument.

That distinction is being made after the fact, which is the one thing the rule meant to prevent. The honest
version of it is this paragraph: a reader who thinks the clause should have stopped the run is entitled to refuse
FITS on it, and that refusal would be consistent with this document as it was written in advance. The numbers do
not depend on the resolution — they are what the reference implementation does, and what the two files did is
recorded rather than smoothed away.

**Tier 1, and why it is zero.** Every tile in the corpus is already compressed at roughly level 6, so
recompressing at level 9 returns the same bytes; the arithmetic is in §0, the third point of the answer. The tier
is not refused because it is worthless in principle — CFITSIO's own writer asks `zlib` for `Z_BEST_SPEED`, and a
file that really came from that path would gain several per cent — but nothing in a corpus of 21 720 tiles does,
and the rule was fixed before the number existed.

### 8.3 The decision

The numerical thresholds below were met on the counted subset. The original protocol as a whole was
**not** followed: §8.2 records two round-trip mismatches and a post-hoc exception. In the handover of
2026-10-26, the outgoing manager stated that this exception was their decision alone, without owner
approval. The admission recorded below is therefore a historical managerial decision, not evidence of
compliance with the pre-registered rule. On 2026-10-27 the owner selected **2C**: these historical results
are not used for acceptance or release/performance claims, and the exception is not retroactively approved.
Separately, **1A** selects a general release with the implementation opt-in, not full FITS admission.

- **Historical Tier 2 result**: median **21.9 % ≥ 8 %**, and **84.8 % of the corpus ≥ 15 %**, with zero
  round-trip mismatches in the counted set. The audit recorded Tier 2 as admitted and opt-in behind
  `--fits-tile`, never in `default`, because a pipeline that reads `NAXIS1` from an `IMAGE` HDU does not
  expect a `BINTABLE` there. This is a historical record, not a current performance or full-admission claim.

Execution and evidence for this release gate are tracked in [issue #71](https://github.com/RUdolf1517/tzim/issues/71).

One condition is attached that the rule did not ask for but §4 of this document did: **Tier 2 may not ship
before `fitsverify` has accepted its output and `funpack` has produced a header that `diff`s clean against the
original.** The historical run could not perform that gate. Subsequent candidate-specific CI runs have executed
the HEASARC gate; their exact SHAs, run identities and retained-evidence limits are recorded in
[measurement status](measurement-status.md), not retroactively attributed to this historical measurement. A green
external gate is not, by itself, full format admission or release approval. The v0.14.0 scope is an opt-in
implementation; current measurement differences and supplemental-sample requirements remain open for full
admission.
- **Tier 1 does not clear its bar**: median **0.00 % < 2 %**, so it does **not** ship on by default.
- FITS is therefore **not refused**: the audit asked which tier survives a measurement, and one did.

What that does *not* mean: the historical result is not a current support, stability, performance, or full
format-admission claim. The handler is an opt-in implementation behind both switches; `--list-formats` names
FITS only in a build with the `fits` feature. The release-scope record and full-admission questions are tracked
separately in [measurement status](measurement-status.md).

## 9. Open questions that must be answered before any code

1. **Tier 2 off by default or behind a flag?** This audit recommends a flag. The reason is social, not
   technical: pipelines that index `NAXIS1` on an `IMAGE` HDU break, and the tool's promise is to be safe, not
   to be clever.
2. ~~The two "re-read" rows of §0.~~ **Answered, and the first one was wrong here**: the payload is a gzip
   member (CRC-32 and `ISIZE`), not an RFC 1950 zlib stream, so the verification anchor survives but §4.10's
   text does not describe it; the `GZIP_2` permutation is the decreasing order of significance, confirmed from
   the convention and reproduced by a written tile. Both are recorded in §0.
3. **`ZHECKSUM`/`ZDATASUM` behaviour.** Whether to mirror `fpack` exactly, or to refuse files that carry the
   original checksums. Mirroring is more work and produces a better file; refusing is cheaper and excludes most
   archival data. This is the decision that most affects how many real files the handler can touch.

## 10. What happened to this work

The two branches in the original stage-1 plan are historical. The v0.14 handler is an opt-in implementation
behind `--fits-tile` and the non-default `fits` feature, not full FITS admission. The initial #59 comparisons at
`7b82c9ed` and the later `a1b27144` report used `funpack` without `-C`; their literal-header outcomes are
superseded as instrument results and must not be treated as candidate content failures. The independent
`fitsverify -l` results remain valid, including four TESS checksum-warning blockers. The corrected fixed-sample
path is documented in §12; no source row is repaired, replaced, or selected by gain, and no owner protocol
decision is pending. #70/full-admission questions remain separate from this measurement correction.

## 11. PR #84 follow-up — #59 fixed survey sample

*Historical default-mode report; the `funpack` result is superseded as an instrument outcome.*

**Instrument-method caveat.** The report below records the `7b82c9ed` run before the checksum-mode correction;
its `funpack` invocations omitted `-C`. Its 14/14 round-trip result is preserved historically but is not evidence
that the reference candidates have content failures. The later `a1b27144` report, comment
[6520583133](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6520583133), used the same invalid default
checksum mode and is superseded on the same narrow basis. Candidate hashes, source identities and independent
`fitsverify -l` results remain useful; the four TESS checksum warnings remain blockers. See the corrected
measurement protocol/results in §12.

The historical 178-file audit in §8 remains unchanged. The separate, owner-fixed full-product sample from
[comment `6498702204`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6498702204) ran at exact SHA
`7b82c9edb6e5060bc8ce7ed756caebba91505b5b`, workflow run
[41038741167](https://github.com/RUdolf1517/tzim/actions/runs/41038741167). The frozen 14-product source-manifest
SHA-256 is `7a7b563b4a641cca519c47c7fbd07cbe904c7608a8a79774bd4f3efa3d937d46`. All 14 products were fetched and
measured; none was substituted or selected by gain. The environment was Python 3.12.15, NumPy 2.4.6, Astropy
8.0.2, with HEASARC CFITSIO commit `113781d8aa442cf8d3c27295d9084674b8b039f2` (archive SHA-256
`d93c0de372a264156d2f3e1eab2d4159e132c35c2216f71c0af0009b92a72b5e`). The existing two-tier instrument SHA-256
was `104d7fb528007a67c8e9fe613a7fe00084fcfbf6ab9e49b47f0bc2c02319aeff`.

### Distribution and gated disposition

| Registered measurement | n | Median gain | Q1 | Q3 | Share-positive |
| --- | ---: | ---: | ---: | ---: | ---: |
| Tier 1 existing-tile comparator | 0 | **empty** | **empty** | **empty** | **empty** |
| Tier 2 Astropy reference candidates | 14 | 30.42168674698795% | 29.518072289156628% | 64.63408680369086% | 100.00% |

Tier 1 is an empty comparator set, not 0%. The per-tier/refusal distribution was Tier 1 comparator n=0, Tier 2
reference n=14, refused n=0, not-measured n=0, with no refusal reasons. The Tier-2 distribution is **not an
applied acceptance result**: the gate requires `fitsverify -l`,
`funpack`, and exact storage-array plus literal-header comparison on each Astropy candidate, and **0/14
candidates passed the full gate**. `fitsverify -l` statuses were 4 failed, 6 passed, and 4 passed with only
source-inherited warnings; all 14 `funpack` round trips failed the combined literal-header/array comparison.
Therefore the pre-registered #59 threshold disposition is **not applied**. A mismatch is a stop; there was no
checksum removal, header normalization, scaling removal, or other input/candidate repair.

### Exact per-row Astropy reference-candidate identities

The following are the exact candidate byte counts and SHA-256 values measured and validated in this run. The
reference-candidate results below are distinct from the Tzim columns later in the report. The compact report is
[PR comment `6520285722`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6520285722).

| Fixed product ID | Reference candidate bytes | Candidate SHA-256 | `fitsverify -l` on reference candidate | Historical default-mode `funpack` storage-array + literal-header comparison (superseded) |
| --- | ---: | --- | --- | --- |
| `tess-s1-c1-ccd1-calibrated` | 26,242,560 | `41698a6e1e90611d70ac06a7b0b0a29b2a68616b70b7ddbacde034ca94d03b93` | FAIL, exit 1; new checksum warning | FAIL |
| `tess-s1-c1-ccd1-uncalibrated` | 8,280,000 | `dfa30bbef4c5aac175da2ceccb7243987fac3b2e3317cd197b268344747e54fe` | FAIL, exit 1; new checksum warning | FAIL |
| `tess-s1-c4-ccd2-calibrated` | 27,089,280 | `a417b75ca6b184d1d67f3fc7d9bef5758e2fc605313e462113a37441d5fe3c1c` | FAIL, exit 1; new checksum warning | FAIL |
| `tess-s1-c4-ccd2-uncalibrated` | 9,192,960 | `2a83e24a6ca726ab989ca3c8e6d988db81b0bbdc063f6adb826e17e463051b23` | FAIL, exit 1; new checksum warning | FAIL |
| `galex-tile-01-MISDR1_18916_0459-fd-int` | 15,693,120 | `65631d1dcbe2184bd30f29fd85e3fe57ca51d34822a30f3e1a033cec9509b16c` | inherited EPOCH warning; candidate exit 1 | FAIL |
| `galex-tile-02-MISDR1_18916_0459-nd-int` | 20,862,720 | `ed7055cad3e847f27f23863570dde8fdb81b7ef0865adb7a9cc6bd4ee9ca781a` | inherited EPOCH warning; candidate exit 1 | FAIL |
| `galex-tile-03-MISWZS03_18917_0284-fd-int` | 6,825,600 | `e07f97d54710b8abeda7b042bbf4ab51d155739354dcfcb550e73b0505989f46` | inherited EPOCH warning; candidate exit 1 | FAIL |
| `galex-tile-04-MISWZS03_18917_0284-nd-int` | 15,782,400 | `7b46cf22c50b03c18d6693ec83c369bd6f057798402f8282ca3b7961a4c12122` | inherited EPOCH warning; candidate exit 1 | FAIL |
| `rosat-rass-c` | 328,320 | `b2c0817a8900a237cec8095938fd1d7d1e7e2c42639f86551949922478d7433e` | PASS, exit 0 | FAIL |
| `rosat-rass-m` | 336,960 | `7a9a4d65c406c1a6cead13957b6b6cc9dbcfa35dba186ad0bb9330b191bd3858` | PASS, exit 0 | FAIL |
| `rosat-rass-ij` | 339,840 | `b99dbd1baa69bd387863d1226a5e09a0028af14764483562465e76cbad44a288` | PASS, exit 0 | FAIL |
| `rosat-rass-exp-c` | 336,960 | `d0e382fc0a1252b38246c0df3f2603cb815a7de164246f425fca5e2a3c2bb773` | PASS, exit 0 | FAIL |
| `rosat-rass-exp-m` | 336,960 | `a1d1fbd1784d05970dcc314bac215d5659ecb35493a2df9e51d677f412e3fd2e` | PASS, exit 0 | FAIL |
| `rosat-rass-exp-ij` | 336,960 | `7552f1109551fd16c6823fde5c4b9ce50c7670a7610349b6859f52c2811128f6` | PASS, exit 0 | FAIL |

For the four GALEX candidates, `fitsverify -l` returned the same single deprecated-`EPOCH` warning as the
source; these are counted as inherited-warning diagnostics, not as clean exit-zero checks. The six ROSAT
candidates returned zero with no candidate warning; the source-side deprecated-`BLOCKED` warning remains an
unchanged-source diagnostic. The four TESS candidates introduced a `CHECKSUM`-agreement warning absent from
their source baseline. Exact command arguments, full stdout/stderr, row validation records, source hashes, and
independent recomputation are retained in artifact `fixed-fits-evidence` (ID `12970075387`, SHA-256
`0d7599db4ac4edf833eeff67038bceadfe9a69f78e2d82606a776d3f3b054ba4`, 133,704 bytes). The artifact contains no
source FITS bytes, reference-candidate bytes, reconstructed bytes, or ROSAT data. A local download attempt
returned Azure Blob `EOF`, so the raw ZIP has not been independently inspected in this sandbox; the artifact
remains attached to the exact run and the report comment preserves the per-row identities and outcomes.

### Separate Tzim disposition

Tzim **skipped all 14 rows unchanged**: output sizes and SHA-256 values equal the inputs. Direct source-versus-
unchanged-Tzim array/header comparisons passed 14/14; Tzim-candidate `funpack` is not applicable for these
unchanged rows. These results do **not** validate the Astropy reference candidates above and are not Tier-2
gain results. No FITS input or candidate bytes were uploaded, and the ROSAT permission boundary was preserved.

**No owner protocol decision is pending.** The earlier request above was based on an invalid default-checksum
`funpack` comparison and is withdrawn. Continue the existing #59 protocol with `funpack -C` for every literal
header comparison, require the external integer-only positive control through the same comparison path, and retain
the original fixed rows and all negative controls. The four TESS `fitsverify -l` checksum warnings remain blockers;
keep the registered threshold `not_applied` unless every required candidate gate passes. The corrected exact-SHA
report and raw structured diagnostics are recorded in §12; no FITS bytes are uploaded.

## 12. Corrected #59 reader-mode protocol

This correction changes the `funpack` reader mode and diagnostics, not the pre-registered acceptance protocol,
source identities, sample, threshold, or raw-row retention. The corrected fixed-sample run retains all 14 exact
TESS, GALEX and ROSAT product rows and the Astropy reference-candidate byte counts/SHA-256 values used for the
measurement. It does not repair or substitute inputs, normalize headers, remove checksum cards, or upload FITS
bytes; the ROSAT permission boundary remains unchanged.

Every reconstruction whose header is compared literally uses `funpack -C`: each exact Astropy Tier-2 reference
candidate is reconstructed against its source, and each optimized Tzim candidate path reconstructs both source
and candidate before comparison. The mode is recorded in the raw row, tool-command log, positive-control record,
aggregate and report. The positive control is a real pinned-CFITSIO `fpack` → `fitsverify -l` → `funpack -C`
round trip through the same exact comparator, using an integer-only fixture. It must pass before candidate header
or payload differences are attributed to the survey candidates; `fpack` is not used as a float-array correctness
oracle. The existing negative controls remain intact.

Each comparison records per-HDU exact 80-column header-record equality and exact storage-payload equality,
including shape and HDU type. Mismatch records distinguish `header_only`, `payload_only`, `combined`, and
HDU-structure failures; header mismatches include the HDU/card index, keyword, first differing byte offsets,
and exact before/after cards. The complete structured records are in `reference-hdu-diagnostics.jsonl` and
`raw-rows.jsonl`; only hashes, sizes, cards and tool output are retained, never FITS payload bytes. The four
TESS `fitsverify -l` checksum warnings remain independent candidate blockers, so the registered threshold stays
`not_applied` unless all required candidate gates pass. Exact corrected per-row results and the final-SHA artifact
are linked from the PR #84 report; no manager/owner protocol choice is being requested.
