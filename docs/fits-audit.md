# FITS: the audit the contract has to pass

Related: [guarantees §4.12](guarantees.md#412-fits--contract-written-not-implemented) · [formats](formats.md) ·
[dependencies](dependencies.md) · [size-budget](size-budget.md) · [pdf-audit](pdf-audit.md) · [roadmap](roadmap.md)

This is stage 1 for FITS, in the order the FLAC and PDF work established: the audit first, the contract second,
the code only if the numbers justify it. FITS was not refused anywhere — it was simply absent from
[formats](formats.md) §5, which is a gap this document closes one way or the other.

**Status: not implemented, not measured.** No FITS code exists and `--list-formats` does not name the format.
The contract in [§4.12](guarantees.md#412-fits--contract-written-not-implemented) says what an implementation
would have to preserve; §8 below fixes the decision rule **before** the numbers exist.

Unlike the PDF audit, this one has a confidence problem that has to be stated up front rather than buried.

## 0. What is a fact here and what has to be re-read from the source

FITS is specified by the IAU FITS Working Group (*Definition of the FITS Standard*, version 4.0) and its
registered conventions. This audit was written away from that text, so:

| Claim | Confidence |
| --- | --- |
| 2880-byte records; 80-character cards; header padded with spaces; data padded to a 2880 boundary | high — this is the shape everyone knows |
| `SIMPLE = T` is the first card of the primary HDU; `BITPIX` ∈ {8, 16, 32, 64, −32, −64}; data size is `\|BITPIX\|/8 × Π NAXISn` | high |
| tile compression turns an image HDU into a `BINTABLE` whose columns hold the compressed tiles in the table's **heap**, with `ZIMAGE`, `ZSIMPLE`, `ZBITPIX`, `ZNAXIS`, `ZNAXISn`, `ZTILEn`, `ZCMPTYPE`, `ZNAME` keywords | high |
| `ZCMPTYPE` values are `GZIP_1`, `GZIP_2`, `RICE_1`, `PLIO_1`, `HCOMPRESS_1` | high |
| the payload of `GZIP_1`/`GZIP_2` is a **zlib** stream (RFC 1950: two header bytes, Adler-32 trailer) despite the name | **re-read before coding** — the whole cost estimate rests on it |
| `GZIP_2` applies a byte shuffle before deflate so that the bytes of multi-byte samples are grouped by significance | high that it exists, **the exact permutation must be taken from the convention**, never reconstructed |
| `CHECKSUM`/`DATASUM` are 16-character ASCII encodings of two 32-bit 1's-complement sums over the HDU | high |
| `fpack` moves an original `CHECKSUM`/`DATASUM` into `ZHECKSUM`/`ZDATASUM` and the compressed HDU carries its own | **re-read before coding** |

The two marked rows are the ones that would sink the project if guessed wrong, so the contract refuses to
proceed until they are confirmed. Nothing in this document is more useful than an honest "check this".

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
| `GZIP_1`/`GZIP_2` tiles | the archival norm when `fpack` was used | **1–4 %**, if `fpack` wrote zlib at its default level rather than 9 |
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
| a tile payload that is not a well-formed zlib stream | refuse, never repair — the §4.10 rule |
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

**Results: not measured.** No `fpack`, no `fitsverify` and no FITS corpus were available where this was written;
§3 and §7 are estimates and are labelled as such. Filling §8 is the whole of the remaining work and does not
require a line of the handler.

## 9. Open questions that must be answered before any code

1. **Tier 2 off by default or behind a flag?** This audit recommends a flag. The reason is social, not
   technical: pipelines that index `NAXIS1` on an `IMAGE` HDU break, and the tool's promise is to be safe, not
   to be clever.
2. **The two "re-read" rows of §0.** If `GZIP_1` payloads turn out to be raw DEFLATE rather than zlib, the
   verification story loses its checksum anchor and the cost estimate changes; if the `GZIP_2` shuffle differs
   from what is assumed, Tier 1 loses its cheapest case.
3. **`ZHECKSUM`/`ZDATASUM` behaviour.** Whether to mirror `fpack` exactly, or to refuse files that carry the
   original checksums. Mirroring is more work and produces a better file; refusing is cheaper and excludes most
   archival data. This is the decision that most affects how many real files the handler can touch.

## 10. What happens to this work

- **If the measurement refuses FITS**, this audit and §4.12 stay on record, `formats.md` §5 gains FITS with the
  numbers beside it, and the next attempt starts here rather than from zero — the same disposition FLAC got.
- **If it admits**, the next issue carries the [formats §4](formats.md#4-admission-criteria-for-a-new-format)
  checklist: a synthetic FITS writer in `tests/common/mod.rs` modelled on `build_tiff`, the corpus gate, the
  measured size delta, and `fitsverify` wired into CI.

Either way the answer stops being "nobody looked". That is the only claim this document makes.
