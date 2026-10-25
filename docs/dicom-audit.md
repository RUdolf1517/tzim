# DICOM: which cost is larger, the dictionary or the refusals

Related: [guarantees §4.13](guarantees.md#413-dicom--contract-written-not-implemented) · [formats](formats.md) ·
[dependencies](dependencies.md) · [size-budget](size-budget.md) · [fits-audit](fits-audit.md) ·
[pdf-audit](pdf-audit.md) · [roadmap](roadmap.md)

This is stage 1 for DICOM, in the order the FLAC, PDF and FITS work established: the audit first, the contract
second, the code only if the numbers justify it.

**Status: not implemented, not measured.** No DICOM code exists and `--list-formats` does not name the format.
[formats](formats.md) §5 has carried the same sentence since the audit of 0.10.0 — *"whether the refusal list or
the dictionary is the larger cost is unknown; an audit would answer it, nobody has written one"*. This is that
audit, and the answer is not the one the sentence expected.

**The answer, in one paragraph.** The dictionary is the larger cost, and it is avoidable — but only by avoiding
the files that matter. DICOM encodes each data element as `tag, VR, length, value`, where the VR may be present
in the file (Explicit VR) or absent and recoverable only from a registry dictionary of **5 189 entries**. A
handler that never reads the dataset needs no dictionary at all, and there is exactly one such handler: recompress
a dataset that is *already* deflated, which is opaque bytes as far as this program is concerned. That slice costs
almost nothing and touches a small share of real files. Every slice that would gain on the files people actually
have requires walking the dataset, and the largest such slice requires the dictionary. So the cheap tiers are
cheap because they do not look inside, and the expensive tiers are expensive for exactly the reason the old
sentence guessed.

## 0. What is a fact here, and what has to be re-read before coding

Unlike the FITS audit, this one was written *with* the sources open, so most rows are checked rather than
remembered. The citation says what was read, not what is believed.

| Claim | Confidence |
| --- | --- |
| A file is an optional 128-byte preamble, the four bytes `DICM`, then File Meta Information (group `0002`), then the data set | high |
| The meta group is **always** Explicit VR Little Endian and is **never** compressed, whatever the transfer syntax says | high |
| `(0002,0000)` is the group length of the meta group in bytes; `(0002,0010)` is the Transfer Syntax UID | high |
| The meta group holds 22 distinct elements in the registry dictionary | **measured** — counted in `pydicom/src/pydicom/_dicom_dict.py` (pydicom, MIT) |
| The data dictionary holds **5 189** entries in **71** groups with **34** distinct VRs, plus **71** repeater entries (`50xx`, `60xx`, `7Fxx`) | **measured** — same file |
| pydicom knows **43** transfer syntaxes: **39** carry encapsulated pixel data (JPEG family, JPEG-LS, JPEG 2000, RLE, MPEG, SMPTE video, and deflated image frames) and **4** do not (Implicit VR LE, Explicit VR LE, Deflated Explicit VR LE, and Explicit VR Big Endian, which is retired) | **measured** — counted in `pydicom/src/pydicom/uid.py` |
| Deflated Explicit VR Little Endian (`1.2.840.10008.1.2.1.99`) is the data set encoded Explicit VR LE and then compressed with **raw DEFLATE (RFC 1951)** — no zlib header, no Adler-32 | **verified in two implementations** — see below |
| A writer may append **one** `0x00` padding byte when the deflated stream has odd length | high for pydicom (it writes it), **check DCMTK before coding** |
| The deflated stream therefore carries **no checksum of any kind** — the file has no anchor for the original bytes | high, and it is the single most important consequence in this document |
| Encapsulated pixel data is `(7FE0,0010)` with undefined length, a Basic Offset Table item, then fragment items each introduced by `(FFFE,E000)` | high |
| Implicit VR LE (`1.2.840.10008.1.2`) is the classic default for what modalities emit | **medium** — this is the claim a corpus has to answer, and §8 is built around it |
| Most PACS and viewers accept a deflated data set | **low** — assume they do not, and treat Tier 2 as opt-in |

The third-to-last row deserves the source quoted, because it overturns the obvious assumption: a negative window size is zlib's way of asking for raw DEFLATE with no wrapper. pydicom's writer:

```python
compressor = zlib.compressobj(wbits=-zlib.MAX_WBITS)
deflated = bytearray(compressor.compress(buffer.getvalue()))
deflated += compressor.flush()
fp.write(deflated)
if len(deflated) % 2:
    fp.write(b"\x00")
```

and DCMTK's output filter, which is explicit about the alternative being wrong:

```c
/* create deflated ZLIB format instead of deflated bitstream format
 * (i.e. RFC 1950 instead of RFC 1951).
 * THE RESULTING BITSTREAM IS NOT DICOM COMPLIANT!
 */
```

So this is **not** [§4.10](guarantees.md#410-zlib-rfc-1950--v09) inside a container, which is what the PDF slice
and the FITS Tier 1 both are. It is the *raw DEFLATE* row that [formats](formats.md) §5 refuses — supported here
only because the meta group names the encoding, which is exactly the identification a bare DEFLATE stream cannot
provide. What it costs is the Adler-32, and §5 says what that means for verification.

## 1. What the file is

| Part | Shape | Notes |
| --- | --- | --- |
| preamble | 128 bytes, arbitrary | may be absent, and then there is no `DICM` either |
| `DICM` | 4 bytes | a file without it is a raw data set with no meta group: unreadable to this program, refused |
| File Meta Information | elements of group `0002`, Explicit VR LE, defined length | 22 possible tags; tells us the transfer syntax |
| data set | a flat sequence of elements, big or little endian, explicit or implicit VR | the bulk of the file |
| element | `(gggg,eeee)`, VR (explicit only), length, value | lengths are 2 or 4 bytes depending on the VR |
| sequence | an element with undefined length followed by items `(FFFE,E000)` and a delimiter `(FFFE,E0DD)` | nesting is arbitrary |
| encapsulated pixel data | `(7FE0,0010)` with undefined length, Basic Offset Table, fragments | each fragment is an item; the payload is JPEG, JPEG-LS, JPEG 2000 or RLE, or a private codec |

Two structural properties decide the design, and they are the same two that decided FITS:

1. **Nothing points into the data set by offset.** There is no cross-reference table, no pointer block, no
   fragment table outside the pixel data. A data set that is replaced by a shorter compressed form moves nothing,
   because nothing recorded a position — unlike PDF, where every rewrite renumbers the xref.
2. **The data set is opaque to this program.** In the tier that matters here it is a single DEFLATE stream and
   this program never learns what is inside it. That is what removes the dictionary, and it is also what removes
   any chance of a semantic check.

## 2. The three operations, and which one needs the dictionary

| Tier | Operation | Dictionary needed | What changes | Precedent |
| --- | --- | --- | --- | --- |
| **1 — recompress** | a data set already encoded as Deflated Explicit VR LE: inflate the stream, deflate it again at a higher level (optionally with Zopfli) | **no** — the data set is never parsed | the compressed bytes, their length, and the optional trailing pad | [§4.10](guarantees.md#410-zlib-rfc-1950--v09), the transform, minus the container |
| **2a — deflate** | an **Explicit** VR LE data set: walk the elements to prove the encoding is honest, then write the same bytes as a DEFLATE stream and change the transfer syntax | **no** — explicit VR carries its own VRs | the transfer syntax UID, the meta group length, and the whole data set becomes a stream; **every inflated byte is identical** | [§4.8](guarantees.md#48-tiff-classic-baseline-strips--v07) — TIFF turns `Compression 1` into `8` |
| **2b — translate and deflate** | an **Implicit** VR LE data set: recover every VR from the dictionary, re-encode as Explicit VR, then deflate | **yes — 5 189 entries and 71 repeaters** | every element header in the file | none: this is re-encoding, not compression |

What Tier 2a does need is much smaller: the **34 VRs** and, for each, whether its length field is two bytes or four. That is a 34-row constant in the source, not a generated table, and it is the difference between reading a file and knowing what every tag in the registry means.

The dictionary, measured as a table rather than as a library: 5 189 entries × 5 bytes (4-byte tag plus 1-byte VR
index) is **about 26 KiB** of packed data, sorted for binary search, plus the repeaters and the generator that
keeps it in sync with the registry. That is larger than any handler in this project except the JPEG codec, and it
exists to serve one tier.

A second transfer syntax is built on deflate — `1.2.840.10008.1.2.8.1`, Deflated Image Frame Compression — and it is **not** a second cheap tier: its frames are encapsulated fragments with a Basic Offset Table, so it falls under the encapsulated rule and is refused. It is named here only because its name invites the opposite assumption.

Tier 2b is where the old sentence in `formats` §5 was pointing, and it is now answerable: **the dictionary is
neither the only cost nor a small one, and it is required by the tier that covers the most real files**. Implicit
VR is what classic modalities emit, and it is the encoding this program would have to translate before it could
deflate. So the tiers split cleanly: cheap and rare, or expensive and common.

## 3. Where the bytes are — honest estimates, not measurements

| What the file is | Share of real archives | Expected gain |
| --- | --- | --- |
| encapsulated pixel data (JPEG baseline/lossless, JPEG-LS, JPEG 2000, RLE) | **large** in any archive that came from a PACS | **0 %** — different codecs, refused |
| uncompressed Implicit VR LE | large in archive output from classic modalities | 20–50 % available via Tier 2b, **blocked by the dictionary** |
| uncompressed Explicit VR LE | common in modern output | **20–40 %** via Tier 2a, at the cost of the reader pool |
| Deflated Explicit VR LE | **rare** — a transport choice, not a default | **1–5 %** via Tier 1, and the cheap one |
| File Meta Information, presentation states, structured reports | small in bytes | 0 % — copied verbatim |

The distribution decides this format, exactly as it decided PDF and FITS. If a corpus is mostly encapsulated
pixel data, the answer is 0 % no matter how good the implementation is; if it is mostly explicit VR, Tier 2a is the
best gain-per-line in the project; if it is mostly implicit VR, the honest answer is that the cheap tiers cannot
help and the expensive one is not worth 26 KiB.

## 4. What a rewrite may change, tier by tier

**Tier 1.** The DEFLATE stream, its length, and — if the new length is odd — the presence or absence of the single
`0x00` pad. Byte-identical: the preamble, `DICM`, and every meta element including `(0002,0000)`, whose value
counts the meta group and not the file. The invariant I3 is trivially satisfied: nothing is removed, because
nothing is read.

**Tier 2a.** The transfer syntax UID (`1.2.840.10008.1.2.1` → `1.2.840.10008.1.2.1.99`, 19 → 22 characters, so
its padding changes), `(0002,0000)` because the meta group grew by three bytes, and the data set, which becomes a
stream. Everything else in the meta group is copied byte for byte, and the data set **inflates to exactly the
original bytes**. This is a representation change, not a content change — the same class as TIFF `1 → 8` — and it
is the reason Tier 2a is opt-in: a reader without the deflated syntax sees a file it cannot open at all, which is
a worse failure than a file it opens slightly differently.

**Tier 2b.** Everything above plus every element header. Not proposed.

## 5. Verification: what can be proved, and with what

| Clause | Instrument |
| --- | --- |
| the file has `DICM` at offset 128, the meta group parses as Explicit VR LE, and `(0002,0000)` agrees with where the group ends | own reader (V3) |
| the transfer syntax is one of the two this handler claims | own reader |
| Tier 1: both deflate streams inflate, and the two inflated byte sequences are equal | `flate2` (V1) — **with the caveat below** |
| Tier 2a: the candidate inflates to exactly the original data set, byte for byte | `flate2` against raw bytes — the strongest clause in this project, no decoder and no semantics in the way |
| every meta byte is identical except the two this tier is allowed to change | own comparison |
| a real reader accepts the candidate and reports the same elements | `pydicom` **or** `dcmdump` in CI, comparing element dumps |

The caveat, which belongs in the contract and not in a footnote: **the original side has no checksum.** gzip
carries CRC-32 and ISIZE, zlib carries Adler-32, ZIP carries a CRC per entry, and TIFF's Deflate strips carry
nothing — which [§2.6](guarantees.md#26-verification-independence) already records as TIFF's one weak spot. DICOM
in Tier 1 is in TIFF's position and slightly worse: the stream does not even have a header to validate, so a
defect symmetric in `flate2`'s inflate and deflate would survive verification. Tier 2a does not have this problem
at all, because its original side is raw bytes nobody decoded.

That asymmetry is worth stating plainly: **the cheapest tier is the one whose verification is weakest.**

## 6. Refusal list

| Construct | Why refuse |
| --- | --- |
| no `DICM` at offset 128 | a raw data set with no meta group; nothing identifies its encoding |
| a meta group that does not parse as Explicit VR LE, or whose `(0002,0000)` disagrees with where the group ends | the boundary of the compressed stream is unknown |
| an undefined-length element inside the meta group | not legal there; a file that has one is broken |
| a transfer syntax other than `1.2.840.10008.1.2.1.99` (Tier 1) or `1.2.840.10008.1.2.1` (Tier 2a) | including all 39 encapsulated ones, `1.2.840.10008.1.2` (implicit), `1.2.840.10008.1.2.2` (retired big endian) and private syntaxes |
| encapsulated pixel data anywhere in a file offered to Tier 2a | the fragment structure would have to be preserved while the rest is compressed; refuse rather than special-case |
| a deflate stream that does not inflate to completion | refuse, never repair — the §4.10 rule |
| trailing bytes after the stream that are not a single `0x00`, or a `0x00` when the stream already ends on an even boundary | the file is longer than it says, or the pad is not a pad |
| Tier 2a on a data set that does not parse with explicit VR throughout (a stray implicit VR, a length that runs past the end) | re-encoding a file we misread is the failure mode this project exists to avoid |
| a data set longer than the byte budget | the inflated data set is the whole file, see §7 |

## 7. Cost

| Item | Estimate | Basis |
| --- | --- | --- |
| new dependencies | **0** | `flate2` is already used by four handlers; Zopfli is already optional |
| Tier 1 code | 250–400 lines | preamble and `DICM`, a meta-group walker with the VR length forms, one inflate and one deflate, padding |
| Tier 2a code | +400–700 lines | a full explicit-VR element walk with items, sequences and undefined lengths, and the refusal of encapsulation |
| Tier 2b code | +5 189-entry dictionary (**≈ 26 KiB**), 71 repeaters, a generator, and the same walker in implicit mode | measured, §0 |
| binary | **+4…8 KiB** for Tier 1, **+15…25 KiB** for Tiers 1 and 2a | TIFF's handler is +36 KiB for comparable walking; the table is what makes 2b expensive |
| memory | the file **and** the inflated data set, both live | a deflated data set inflates to the entire data set: this handler belongs under the [#51](https://github.com/RUdolf1517/tzim/issues/51) byte budget, not next to it |

The memory row is not a formality. A deflated data set inflates to its **uncompressed** size, which is larger than the file on disk and can be a gigabyte for one instance; that buffer exists in full before the first byte of the candidate is written, and `--jobs` multiplies it. This handler belongs under the [#51](https://github.com/RUdolf1517/tzim/issues/51) byte budget, not next to it.

## 8. The measurement, and the rule fixed in advance

**Corpus**, pinned by URL, tag and SHA-256, assembled like `.github/scripts/corpus.sh`:

| Set | Why |
| --- | --- |
| `pydicom` test files (MIT) | synthetic edge cases, both encodings, deflated files, corrupt ones |
| DCMTK test data | the reference implementation's own fixtures, including encapsulated pixel data |
| GDCM test data | a second implementation's fixtures, to catch what one vendor considers normal |
| a sample of a public archive (TCIA or similar) | the only set that answers the share question in §3 honestly |

| Question | Command | Answers |
| --- | --- | --- |
| Which tier does each file land in? | `dcmdump` for the transfer syntax, then the handler's own refusal reason | the split between §3 rows |
| What does Tier 1 win? | inflate, then `zlib -9` or the candidate, and count bytes | median gain over the files it can touch |
| What does Tier 2a win? | `dcmconv --write-xfer-deflated` as the reference, then compare sizes | median gain, and how close to the best a zlib level 9 writer is |
| Does a real reader accept the candidate? | `dcmdump` the candidate and diff the element dump against the original | the clause §5 cannot check by itself |

**The rule, fixed now:**

- **Tier 1** is admitted if its median gain is **≥ 2 %** over the files it can touch, with zero round-trip
  mismatches and no more than **8 KiB** of binary — then it ships **on** inside a `dicom` feature, because it
  changes nothing a reader can observe;
- **Tier 2a** is admitted if its median gain is **≥ 15 %** and it ships **opt-in, behind `--dicom-deflate`**, for
  the reader reason in §4 — never by default, whatever the gain;
- **Tier 2b** is admitted only if Tier 1 and Tier 2a both fail **and** the corpus shows that more than **50 %** of
  the bytes are implicit VR uncompressed — then the dictionary is worth its 26 KiB, and not otherwise;
- if **none** clears its bar, DICOM is refused with the numbers on record, exactly as FLAC was.

**Results: not measured.** No DICOM corpus, no `dcmdump` and no `dcmconv` were available where this was written.
§3 and §7 are estimates and are labelled as such; the numbers in §0 are counted, not estimated. Filling §8 is the
whole of the remaining work and does not require a line of the handler.

## 9. Open questions that must be answered before any code

1. **What share of real files is each tier?** This is the question, and everything else is secondary. If deflated
   data sets are a fraction of a percent and implicit VR is most of the corpus, the honest outcome is a refusal —
   and it will be a refusal with numbers, which is what this project asks for.
2. **The pad byte.** pydicom writes one `0x00` when the stream is odd. It is not established here that DCMTK
   does, or that readers tolerate its absence. This must be checked against both implementations before the
   handler decides whether to write it.
3. **Which readers accept `1.2.840.10008.1.2.1.99`?** Tier 2a produces a file that some viewers cannot open at
   all. The audit assumes the worst; a survey of what is actually deployed could change the flag from "opt-in"
   to "refused".
4. **Whether the meta group is as regular as claimed.** Twenty-two elements, all defined length, no sequences —
   that is the standard, and a corpus is the only way to learn whether the world agrees.

## 10. What happens to this work

- **If the measurement refuses DICOM**, this audit and [§4.13](guarantees.md#413-dicom--contract-written-not-implemented)
  stay on record, [formats](formats.md) §5 gains the numbers beside the refusal, and the next attempt starts here
  instead of from zero — the disposition FLAC got.
- **If it admits a tier**, the next issue carries the [formats §4](formats.md#4-admission-criteria-for-a-new-format)
  checklist: a synthetic DICOM writer in `tests/common/mod.rs`, the corpus gate, the measured size delta, and
  `dcmdump` wired into CI.

Either way the sentence in `formats` §5 stops being true. That is the only claim this document makes.
