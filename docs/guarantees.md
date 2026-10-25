# Guarantees

This document is normative. It defines what Tzim promises for every file it touches, and how each promise
is verified before a file is replaced. If the code and this document disagree, that is a bug and the run
must be treated as a verification failure.

Related: [formats](formats.md) · [safety](safety.md) · [testing](testing.md)

---

## 1. Vocabulary

| Term | Meaning |
| --- | --- |
| **Information** | All bytes of the input file, interpreted according to the format specification. |
| **Content** | The part of the information a consumer of the format actually uses: pixels, samples, entry payloads, text. |
| **Significant property** | A format-level attribute that changes how the content is interpreted or presented: dimensions, color type, bit depth, frame delays, entry names, timestamps, compression method, checksums. |
| **Contract** | An explicit, per-format statement of what is preserved, what is allowed to change, and how the result is checked. |
| **Verification** | A mechanical check performed on the candidate file. It never uses the encoder that produced it; how independent its *reader* is differs per format and is stated in [§2.6](#26-verification-independence). |
| **Candidate** | The temporary output file produced by a handler. It never replaces the original until it passes verification. |

A transformation is acceptable only if a contract exists, the contract is implemented as a verification
routine, and the routine passes.

---

## 2. Guarantee classes

### G1 — byte-identical

The output file is byte-for-byte identical to the input. Tzim produces G1 outcomes only when it does not
write anything:

- `already_optimal` — no candidate was strictly smaller;
- `unsupported` — no handler for the detected format;
- `skipped` — policy refusal (symlink, hard link, size limit, filter, opt-in format not enabled);
- every failure path;
- every `--dry-run` run.

G1 holds because nothing is written on these paths: a candidate, when one exists, lives in a temporary file
beside the destination and the guard removes it on every exit. The only comparison the program makes is the
re-check of `(device, inode, length, mtime)` immediately before the replacement, which is what aborts a run
whose input changed underneath it (I5). Content hashing happens in the test suite ([testing](testing.md)),
not in the program, and this document does not claim otherwise.

### G2 — exactly reversible

The transformation is reversible: the original file can be reconstructed bit-for-bit from the output. This
requires a container that stores the reconstruction parameters (the Precomp `.pcf` approach), which means
the output is no longer a normal file of the same type.

**Tzim does not use G2.** It is listed here so that the three classes are never mixed. Any future use would
require an explicit, separate document and an explicit opt-in.

### G3 — format-equivalent

Bytes differ; all content and all significant properties are preserved according to a per-format contract
(§4). The output remains a valid, ordinary file of the same format, readable by any conforming consumer.

G3 is the only class used for real optimizations. It is always verified by:

1. re-parsing the candidate strictly, including all checksums;
2. decoding both input and candidate and comparing content — with an independent library where one exists,
   and with this repository's own reader where it does not ([§2.6](#26-verification-independence));
3. comparing the inventory of significant properties and preserved byte ranges;
4. accepting the candidate only when it is not larger than the input — one function, `formats::size_ok`
   in `src/formats/mod.rs`, called from `formats::verify` and again from `src/pipeline.rs` before the
   temporary file is read back, and by both fuzz targets (strictly smaller unless `--strip-metadata`
   removed something, see [§2.7](#27-g4--metadata-removed-on-request); never larger, under any flag).
   The rule lives here because it is a property of the candidate, not of a format, and a second copy of it
   in a caller is how the equal-size case became an internal error in 0.11.0.

### G4 — metadata removed on request

Used **only** when `--strip-metadata` is given. Content and every significant property are still preserved —
the pixels, the coefficients, the payload, the names, the order — but a fixed list of metadata is removed on
purpose. The complete list per format, and the rules that bound it, are in §2.7.

G4 exists so that the other three classes never have to be quietly stretched: without the flag, nothing in
this document changes.

### 2.6 Verification independence

Three levels exist, and this project uses all of them. Naming the level is the point: "verified" on its own
would suggest more than any of these delivers.

| Level | Meaning | Used for |
| --- | --- | --- |
| **V1 — independent decoder** | A decoder written by someone else, from the specification, with no knowledge of this codebase. | PNG and ICO pixels (`png` crate), gzip and ZIP payloads (`flate2`, `miniz_oxide`), **JPEG pixels** (`jpeg-decoder`) when this build has the `jpeg-v1` feature, and the whole corpus through Pillow in the `third-party` job |
| **V2 — separate reader** | Code in this repository, used only for reading, that the writer's output never passes through. | *Not used anywhere today.* |
| **V3 — shared implementation** | The candidate is parsed by the same reader the handler used to build it. | ZIP structure, ICO directory, JPEG markers and JPEG coefficients |

| Format | Structure | Content | What a shared bug could hide |
| --- | --- | --- | --- |
| PNG | V3 (own chunk walker) | **V1** (`png` crate decodes both files, pixels compared) | A framing error that both sides accept; the IDAT payload is compared through an independent inflater, so it cannot hide a content change |
| gzip | V3 (own header walker) | **V1** (`flate2` decodes both bodies; CRC-32 and ISIZE recomputed) | A header misreading; the payload comparison would still be independent |
| zlib | V3 (own header reader) | **V1** (`flate2` decodes both streams; `Adler-32` recomputed here, not by the encoder) | A header misreading; the payload comparison and the checksum would still be independent |
| ZIP | V3 (own central-directory reader) | **V1** per entry (`flate2` + recomputed CRC-32) | A structural field both sides misread; content is checked by an independent inflater and a checksum the writer had to compute from the bytes |
| ICO | V3 (own directory reader) | **V1** for embedded PNGs, byte comparison for bitmaps | A directory field both sides misread |
| TIFF | V3 (own IFD walker; every tag value compared field by field) | **V1** for strips that were uncompressed — the original side is raw bytes and needs no decoder; **shared `flate2`** for strips that were already Deflate | A structural field both sides misread would pass. Unlike gzip, zlib and ZIP there is **no external checksum** anchoring the original side: those three carry a CRC-32, an Adler-32 or an ISIZE written by the original archiver, while TIFF strips carry none, so one `flate2` decode is compared with another `flate2` decode |
| JPEG | V3 | **V3 for coefficients; V1 for pixels with the `jpeg-v1` feature**, and V1 for the whole corpus in CI (Pillow) | A published build (no `jpeg-v1`) can still miss a defect symmetric in our encoder and decoder; the CI comparison is what covers it there, and it does not run inside `verify` |

**The honest consequence for JPEG, and what changed in v0.13.** Until v0.13 the JPEG comparison was the
weakest of the seven: both sides of the coefficient comparison were this repository's codec, so a defect
*symmetric* in the encoder and the decoder would have passed every check here. v0.13 adds two independent
readers, and — this is the part that matters — **neither is in the published binary**, for the reason given
below:

- **`jpeg-decoder`**, inside `verify`, when this build has the `jpeg-v1` feature. Both files are decoded by a
  library that shares nothing with this codebase and the decoded pixels must be identical; a disagreement is
  `failed`, exit 1, and the original is untouched. Since identical coefficients imply identical pixels, this
  adds a check the coefficient comparison cannot make: that our decoder reads the coefficients the way the
  rest of the world reads them. If the independent decoder cannot read the *original*, the file is `skipped`
  with that reason — an oracle that cannot read a file is not a verdict about it. The feature is off in the
  published builds because it does not fit the size condition it was approved under
  ([size budget §3](size-budget.md#3-feature-matrix)); `cargo build --features jpeg-v1` turns it on.
- **Pillow**, outside the binary, in the `third-party` CI job — the same idea for the whole corpus and for
  the other formats (PNG, TIFF, ICO pixels; `zlib` for gzip and zlib streams; `zipfile` for archives). It
  compares at the strongest level each library offers and proves on every run that it can fail, by mutating
  a candidate and requiring the comparison to reject it. Zero bytes of it reach the binary; see
  [testing §3.3](testing.md#the-third-party-decoder-job).

What remains true, and is stated here rather than implied: **a published build's JPEG verification is still
V3 plus whatever CI can prove.** The in-`verify` oracle is a feature, the corpus comparison is a job, and a
user's own file gets neither unless they build with `jpeg-v1`. None of this is a proof: a defect shared by
several independently written decoders would survive. That is why JPEG stays opt-in and why its gain is not
oversold.

**For TIFF** the content comparison is independent only while the strips are uncompressed: `optimize` turns
compression `1` into Deflate, and for that path the original is raw bytes read without a decoder. A TIFF whose
strips were *already* Deflate is re-compressed and compared `flate2` against `flate2`, and TIFF has no
per-strip checksum of its own — so a symmetric defect in the inflater would survive. The structure is compared
field by field (V3), which is where a layout mistake would show up, and the fuzz target covers the parser.

**For ZIP** the risk is smaller than it sounds: the structure is copied, not synthesized, so most bytes are
compared literally; the payload of every entry is inflated independently and its CRC-32 recomputed from the
decompressed bytes, which the writer cannot fake without also changing the content.

### 2.7 G4 — metadata removed on request

`--strip-metadata` is the only path in Tzim that deliberately removes information, and it exists because
every serious optimizer has it and users otherwise reach for a tool that removes it silently.

| Format | Removed | Always kept |
| --- | --- | --- |
| PNG / APNG | `tEXt`, `zTXt`, `iTXt`, `tIME`, `eXIf` | `iCCP`, `sRGB`, `gAMA`, `cHRM`, `pHYs`, `sBIT`, `bKGD`, `tRNS`, `hIST`, `sPLT`, every `acTL`/`fcTL`/`fdAT`, every critical chunk, every unknown chunk |
| gzip | `FNAME`, `FCOMMENT`, `FEXTRA`, `FHCRC`, `MTIME` (zeroed) | the `XFL` and `OS` bytes, `CM`, the payload, CRC-32 and ISIZE |
| ZIP family | archive comment, entry comments, DOS timestamps | names, extra fields (some are structure: ZIP64, the Unicode path), attributes, order, method, CRCs |
| JPEG | `APP1` (Exif, XMP), `APP13` (Photoshop/IPTC), `COM` | `APP0` (JFIF density), `APP2` (ICC), `APP14` (Adobe colour transform), every other `APPn`, all quantization and Huffman tables, every DCT coefficient |
| ICO / CUR | nothing of its own; embedded PNGs are stripped under the PNG rule | the directory, bitmap entries, every PNG chunk in the keep list above |
| TIFF | the descriptive tags listed in [§4.8](#48-tiff-classic-baseline-strips--v07): `DocumentName` (269), `ImageDescription` (270), `Make` (271), `Model` (272), `Software` (305), `DateTime` (306), `Artist` (315), `HostComputer` (316), `InkNames` (333), `Copyright` (33432), IPTC (33723), Photoshop (34377), `UserComment` (37510), `ImageSourceData` (37724), the Windows XP tags (40091–40095), GDAL metadata (42112) | colour, geometry, `Orientation`, ICC, strips, GDAL nodata (42113), unknown tags |
| FLAC (if ever implemented) | `VORBIS_COMMENT`, `PICTURE`, `PADDING` | `STREAMINFO`, `SEEKTABLE`, `CUESHEET`, `APPLICATION` |
| zlib | nothing: the header has no name, comment or timestamp, and `FLEVEL` describes the encoder | every byte of the header and the trailer |

Rules that hold in every format:

- the flag is off unless typed; no configuration file, no environment variable can turn it on;
- content is never touched: pixels, coefficients, payload bytes, entry names and order;
- anything this build does not understand is kept — removing what cannot be checked is not a decision the
  tool makes, even when asked;
- the run says so: every rewritten line carries `(metadata stripped)` and the JSON grows
  `"metadata_stripped":true`;
- `--dry-run` reports what would be removed and writes nothing.

---

## 3. Global invariants

These hold for every format, in addition to the per-format contract.

| ID | Invariant |
| --- | --- |
| **I1** | The original file is never modified unless a candidate has passed every check. |
| **I2** | A candidate is only used if `candidate.len() < original.len()`. A larger or equal candidate is discarded, with one exception: under `--strip-metadata`, an equal-size candidate is accepted when the only change is a removed metadata field ([§2.7](#27-g4--metadata-removed-on-request)). A larger candidate — or an empty one — is never accepted, under any flag. Implemented once, as `formats::size_ok`, so the pipeline, the verifier and the fuzz targets cannot disagree. |
| **I3** | No metadata, comment, attachment, vendor chunk, private field or unknown structure is removed or normalized. The single exception is the explicit `--strip-metadata` request, which removes the fixed list in §2.7 and nothing beyond it. |
| **I4** | No lossy transformation, no format conversion, no quality, resolution, rate or bit-depth change. |
| **I5** | If the input changes on disk between the initial stat and the replacement, the operation is aborted. |
| **I6** | Verification never reuses the encoder. Where the checker necessarily shares a parser with the writer — the JPEG codec and the ZIP structure — the sharing, and what it cannot catch, are stated in §2.6 instead of being called independence. |
| **I7** | The candidate is written to a temporary file in the same directory and replaces the original only by an atomic rename where the platform supports it (see [safety](safety.md)). |
| **I8** | Anything that cannot be parsed with full confidence is refused with a clear message, never "repaired". Tzim does not attempt to fix broken files, and never writes an output for an input it could not fully validate. |

The filesystem-level invariants — **I1**, **I5**, **I7** — are pinned by the tests that carry `T-FS-*` ids in
[testing §2](testing.md#2-traceability). The ones without such an id have no test yet, and
[testing](testing.md#2-traceability) lists them as such rather than implying coverage.

### 3.1 What "verified" actually compares — and what it does not

`verify` runs on every candidate before the rename, and the word appears throughout this document, so it is
worth stating exactly what it is a comparison *of*. It is a byte, structure and pixel comparison. It is not a
semantic model of an image, an archive or a document, and nothing here claims that it is.

| Format | What `verify` compares |
| --- | --- |
| PNG | The `IHDR` payload; every chunk that is neither `IDAT` nor `fdAT` byte for byte and in order; every frame decoded with the `png` crate and compared per index, together with its control data. Under `--strip-metadata`, no chunk from the list in §2.7 may survive. |
| gzip | The header — or, under the flag, exactly the stripped header and nothing else; the eight trailer bytes; payload length and CRC-32; and the inflated payload, in 64 KiB blocks. |
| zlib | The two header bytes, the four `Adler-32` bytes, and the inflated payload. The checksum is recomputed here, not taken from the encoder. |
| ZIP | Entry count and order, names, methods, flags, version fields, extra fields, entry and archive comments, each entry's CRC-32 and uncompressed size, and each entry's inflated content. A nested child is re-verified against its own contract. |
| ICO / CUR | The type, the entry count and order, every directory field except the size/offset pair, bitmaps byte for byte, and embedded PNGs through the PNG check above. |
| TIFF | The surviving entries' tag, type, count and value; the strip count and geometry; and every strip decoded and compared. |
| JPEG | The marker sequence outside the entropy-coded data, `DQT` and `SOF` payloads byte for byte, and every DCT coefficient, decoded and re-encoded. |

What it therefore does **not** do:

- **It does not compare sizes.** Whether a candidate is small enough to be worth writing is decided in the
  pipeline, before `verify` is called, and the rule is in I2.
- **It has no notion of meaning.** Two byte-identical APNG frames swapped with each other are not detected,
  because the decoded pixel sequence is unchanged; such a swap changes nothing that any consumer can observe,
  so it is not a defect. Two *different* frames swapped are detected, because the pixels at that index differ.
  The comparison is deliberately strict rather than semantic: a comment re-encoded to different bytes with the
  same characters is rejected, exactly as a removed one would be.
- **It is only as good as the decoder it uses.** Where the comparison is decoded content, a defect symmetric
  in the encoder and the decoder passes every check here; §2.6 names that case for each format instead of
  calling the result independent.
- **It says nothing about the filesystem.** Permissions, ownership, mtime, `fsync` and the atomicity of the
  rename belong to [safety](safety.md) and are checked by `T-FS-*` tests, not by `verify`.

So "verified" in this document means: *the accepted candidate compares equal to its original under the rules
in the table above*. It does not mean "proved to be the same picture", and it does not mean "semantically
identical". Anything stronger is not claimed anywhere.

Refusal is always preferable to a guess. There is no `--force` flag that bypasses a contract check.

---

## 4. Contracts

### 4.1. PNG — v0.1

**Transformation.** Recompress the image data stream with a better DEFLATE encoder, optionally re-selecting
the per-scanline filters (the pixel data itself is never altered).

**Preserved, byte-identically and in order:**

- the 8-byte PNG signature;
- the IHDR payload: width, height, bit depth, color type, compression method, filter method, interlace
  method;
- **every chunk that is not IDAT**, with its type, payload and CRC — this includes `PLTE`, `tRNS`, `gAMA`,
  `cHRM`, `sRGB`, `iCCP`, `cICP`, `mDCv`, `cLLi`, `sBIT`, `bKGD`, `hIST`, `pHYs`, `sPLT`, `eXIf`, `tIME`,
  `tEXt`, `zTXt`, `iTXt`, APNG control chunks, and any unknown or private chunk;
- chunk order for all of the above;
- `IEND`.

**Allowed to change:**

- the IDAT payload (the zlib stream), the number and boundaries of IDAT chunks, and the IDAT CRC values;
- the total file length.

**Verification:**

1. Candidate re-parsed strictly: signature, chunk framing, every chunk CRC, IHDR first, IEND last, at least
   one IDAT, all IDAT chunks contiguous.
2. Inventory comparison: the ordered sequence of `(type, payload bytes)` for all non-IDAT chunks equals the
   input's sequence. IHDR payload compared byte-for-byte.
3. Independent decode of both files with the `png` crate in identity mode (no transformations); compare
   width, height, bit depth, color type, interlace method and every decoded image byte, streamed row by row.
4. `candidate.len() < input.len()`.

**Refused:**

- source truncated, bad chunk CRC, missing IHDR/IEND/IDAT, or not a PNG;
- interlace method other than 0 or 1;
- images larger than `--max-pixels` (default 100 MPix) or files larger than `--max-size`;
- image data that does not decompress to exactly the expected number of scanline bytes;
- files with data after `IEND`.

**Notes and limits:**

- In v0.1 the filters are re-selected only for non-interlaced images; for interlaced and APNG inputs only
  recompression is applied.
- In v0.1 APNG frame data (`fdAT`) is preserved untouched; only the default image's IDAT is recompressed.
- Color-type or bit-depth reduction (for example palette compaction, RGB→grayscale) is **not** implemented
  and is not part of this contract. If it is ever added, it requires its own contract and its own
  verification, and it changes significant properties.

**Verification IDs:** `T-PNG-01` … `T-PNG-09` (see [testing](testing.md#2-traceability)).

---

### 4.2. gzip — v0.1

**Transformation.** Recompress the DEFLATE body. The header is copied verbatim.

**Preserved, byte-identically:**

- the complete member header exactly as it appears in the input: `ID1`, `ID2`, `CM`, `FLG`, `MTIME`, `XFL`,
  `OS`, and, when present, `FEXTRA`, `FNAME`, `FCOMMENT` and `FHCRC`;
- the decompressed payload, byte for byte and in length;
- the trailing `CRC32` and `ISIZE` values (recomputed, and required to equal the input values);
- the number of members (v0.1: exactly one).

**Allowed to change:**

- the DEFLATE body bytes and length;
- the total file length.

Because the header is copied verbatim, a present `FHCRC` stays valid and a stored original filename stays
exactly as it was, including non-UTF-8 bytes.

**Verification:**

1. Header bytes of the candidate equal the header bytes of the input.
2. Decompressing both files yields identical byte sequences and lengths.
3. Candidate trailer equals the input trailer (`CRC32`, `ISIZE`).
4. `candidate.len() < input.len()`.

**Refused:**

- `CM` other than 8, reserved `FLG` bits set, header checksum mismatch;
- more than one member, or any trailing bytes after the member trailer (multi-member `.gz` files are
  common; v0.1 refuses them rather than silently merging members);
- truncated input, or input whose trailer does not match the decompressed data;
- files larger than `--max-size`.

**Note.** `.tgz` and `.svgz` are gzip files and are covered by this contract automatically: the tar or SVG
payload is decompressed and recompressed as opaque bytes; nothing inside is interpreted in v0.1.

**Verification IDs:** `T-GZ-01` … `T-GZ-06`.

---

### 4.3. ZIP — v0.2

**Transformation.** Recompress entries that use DEFLATE. The archive is rewritten surgically: every
structure is copied and only the fields that must change are patched.

**Preserved, byte-identically:**

- entry count, entry order, and each entry's raw name bytes (no re-encoding, no normalization);
- local header and central directory **extra fields**, verbatim, in order;
- per-entry comments and the archive comment;
- general purpose flags, including the data-descriptor flag (bit 3) and the UTF-8 flag (bit 11);
- version made by / version needed, internal and external attributes (including Unix modes), disk number
  start;
- DOS timestamps and any timestamp extra fields;
- compression method (**v0.2 never changes the method**; stored entries stay stored);
- uncompressed size and CRC-32 of every entry;
- the central directory order.

**Allowed to change:**

- the compressed bytes and compressed length of DEFLATE entries;
- compressed-size fields in the local header, the data descriptor and the central directory;
- local-header offsets stored in the central directory, and the central directory offset/size in the
  end-of-central-directory record;
- the total file length.

**Verification:**

1. Structural snapshot of both archives compared for equality on every preserved field listed above.
2. Every entry decompressed from both archives; length, bytes and CRC-32 compared, and the CRC-32 compared
   with the value recorded in the headers.
3. Every local-header offset resolves to a `PK\x03\x04` signature; every local header name matches the
   central directory name; sizes agree when the descriptor flag is clear.
4. End-of-central-directory consistency: entry count, central directory size and offset, comment length.
5. `candidate.len() < input.len()`.

**Refused (no modification, clear message):**

- ZIP64 archives, multi-disk archives, encrypted entries;
- compression methods other than 0 (store) and 8 (deflate);
- truncated or structurally inconsistent archives, overlapping entries, or a local header whose name
  disagrees with the central directory;
- **signed or signature-carrying archives** — presence of `META-INF/*.SF`, `*.RSA`, `*.DSA`, `*.EC`, an APK
  signing block immediately before the central directory, an entry under `_xmlsignatures/` (signed OOXML),
  or `package/services/digital-signature/` (VSIX). Recompression would invalidate the signature: the bytes
  would be "losslessly" changed while the file became unusable. Tzim refuses these.
- archives with data prepended before the first local header (self-extracting stubs, some installers) —
  v0.2 refuses rather than risk inconsistent offsets;
- archives whose total uncompressed size exceeds `--max-uncompressed` (default 8 GiB) or whose ratio
  exceeds `--max-ratio` (default 1000:1).

**Notes:**

- ODF and EPUB archives (`mimetype` stored as the first entry) are accepted, because v0.2 preserves the
  compression method of every entry and does not reorder anything.
- A refused archive is reported as `skipped` with the reason, the file is left byte-identical, and the run
  still exits `0`: the input is not broken, policy simply declined it. A genuinely corrupt archive is
  reported as `failed` with exit `1`.
- Detection is by content, so an archive with a prepended stub (`MZ...`, `.exe` installers) is not recognised
  as a ZIP at all and is reported as `unsupported`; it is never rewritten either way.
- Changing a stored entry to DEFLATE is a much larger win on some archives but can break formats that
  require stored entries. It is deliberately **not implemented**; if it is added, it will be an explicit
  opt-in with an ODF/EPUB/installer deny-list and its own contract clause.

**Verification IDs:** `T-ZIP-01` … `T-ZIP-12`.

---

### 4.4. ICO / CUR — v0.3

**Transformation.** Optimize PNG images embedded in the icon directory.

**Preserved:** the ICONDIR header, entry count and every directory-entry field except the byte size and
offset of recompressed entries; every non-PNG entry verbatim; entry order.

**Preserved:** the 6-byte directory header, the image count, the entry order, and every directory-entry field
except the byte size and offset of images that moved; every non-PNG (bitmap) entry byte for byte; the icon
type (icon vs cursor, which also carries hotspot coordinates in two entry fields).

**Verification:** the candidate is re-parsed strictly; the first eight bytes of every directory entry
(everything except size and offset) are compared; each embedded PNG is verified under the PNG contract; each
bitmap payload is compared byte for byte.

**Refused:** images that overlap, point outside the file, start inside the directory, or leave unreferenced
bytes at the end of the file. An ICO with no PNG entry produces no candidate and is reported as
`already_optimal`.

**Refused:** malformed directories, entries pointing outside the file, embedded PNGs failing the PNG
contract.

**Verification IDs:** `T-ICO-01` … `T-ICO-04`.

---

### 4.5. APNG — v0.3

**Transformation.** Recompress the default image and every animation frame.

**Preserved:** `acTL` and all `fcTL` payloads verbatim (frame count, loop count, delays, blend and dispose
operations, dimensions, offsets); chunk order; every non-image chunk; **the number of `fdAT` chunks and their
sequence numbers**.

**Mechanism.** A frame's data is the concatenation of its `fdAT` payloads (minus the 4-byte sequence
number). That stream is recompressed and split back into exactly the same number of chunks, each keeping the
sequence number of the chunk it replaces, so `fcTL` payloads and the animation's sequencing stay
byte-identical. If the recompressed stream is too short to fill the original chunks, that frame is left
unchanged.

**Verification:** decode every frame of both files independently and compare frame metadata and pixel bytes;
compare all control chunks byte for byte.

**Corrupt (failed, exit 1):** malformed frame sequences — `fcTL` and `fdAT` number themselves 0, 1, 2, … in
file order with no gaps, and the number of `fcTL` chunks must be the frame count `acTL` declares — and a frame
whose data does not inflate. The sequence is checked structurally on the input before anything is written
(#49); the `png` crate refuses such a file while decoding too, but a decode that stops on both sides of a
comparison is indistinguishable from the end of the frame loop, so the crate is not something this program may
rely on for that verdict. "Corrupt", not "refused": a broken animation is invalid input and exits `1`, while
this document reserves `refused` for a valid file the run declines to rewrite.

**Verification:** the candidate is re-parsed, every non-`IDAT`/`fdAT` chunk is compared byte for byte, the
`fdAT` count and sequence numbers are compared, and every frame is decoded from both files and compared for
pixels and for its control data (size, offsets, delay, dispose and blend operations).

**Verification IDs:** `T-APNG-01` … `T-APNG-05`.

---

### 4.6. Nested containers (v0.3, opt-in)

**Transformation.** Optimize supported files stored inside an archive, then optimize the archive itself.

**Contract:** the parent contract must hold **and** every optimized child must independently satisfy its own
contract. A child that fails verification is stored unmodified in the parent's output; the parent is still
accepted only if it passes its own verification.

**Consequence for the parent contract.** When a child really is rewritten, the entry's CRC-32 and
uncompressed size necessarily change. This is the one documented exception to "uncompressed size and CRC-32
of every entry are preserved" (§4.3). The parent verification therefore accepts a changed checksum **only**
when all of the following hold:

1. the entry's decompressed content is a format Tzim supports (detected by content);
2. the new content is strictly smaller than the old content;
3. the new content passes that format's own contract verification;
4. every other field of the entry is unchanged.

For all other entries the original rule applies unchanged, and an archive with no optimized child must come
out with identical CRCs.

**Limits:** `--max-depth` (default 3) bounds how many containers are opened inside each other. Memory is
bounded by `--max-uncompressed`, which is a limit on **the archive**, not a running total shared by its
children: `check_limits` refuses the whole archive when the sum of the declared uncompressed sizes exceeds
it, before anything is decompressed, and an entry whose data disagrees with its own declared size is refused as
corrupt. There is therefore no "remaining budget" per child, and the per-entry check in `nested_child` is
defence in depth that cannot fire from the normal path.

**Verification IDs:** `T-NEST-01` … `T-NEST-03`.

---

### 4.7. JPEG — v0.4, opt-in

**Transformation.** Re-encode only the entropy-coded data with optimal Huffman tables. Every DCT
coefficient, and therefore every decoded pixel, is bit-identical to the input; this is the `jpegtran
-optimize` transformation, implemented here in safe Rust with no external codec. Typical saving is 1–3 %,
sometimes nothing.

**Scope.** Baseline sequential (SOF0, 8-bit) with exactly one scan. Progressive, arithmetic-coded,
lossless, hierarchical, 12-bit, extended-sequential (SOF1) and multi-scan files are refused, not guessed
at. Refusal is `skipped` and exit code 0.

**Preserved:** every marker except the ones below, as a raw segment with its payload bytes and its
position relative to the scan — `APPn` (JFIF, Exif including the embedded thumbnail, ICC, Adobe), `COM`,
`DQT`, `SOF0`, `DRI`, and anything else present; frame dimensions and component geometry; component ids,
sampling factors and quantization table selectors; the `SOS` header itself (component list, table
selectors, `Ss`/`Se`, `Ah`/`Al`); the restart interval; bytes after `EOI`, byte for byte.

**Allowed to change:** the `DHT` segments (the tables actually used are replaced by optimal ones; tables
defined but unused are carried over verbatim; the segments are merged into one at the position of the
first, so the segment count may drop), the entropy-coded bytes of the scan, and the length fields that
follow from those.

**Refused:** any `SOF` other than SOF0; precision other than 8; more than one `SOS`; `Ss`/`Se`/`Ah`/`Al`
not equal to 0/63/0/0; `DAC` (arithmetic conditioning); `DNL` (height deferred to after the scan); zero
dimensions; more components than 4; sampling factors outside 1–4. A file that parses but contains one of
these is valid, so it is reported as skipped with a precise reason and left untouched.

**Corrupt input:** truncated data, a marker length that runs past the end of the file, a Huffman table
that is overfull or contains the forbidden all-ones code, an entropy stream that ends in the middle of a
block, a restart marker where none belongs — all `failed`, exit code 1, original untouched.

**Verification.** The candidate is re-parsed from bytes and decoded again. Three conditions must hold:

1. **Coefficient identity.** Every DCT coefficient of every component is compared with the input's. This
   is strictly stronger than comparing decoded pixels: identical coefficients imply identical pixels,
   while identical pixels do not imply identical coefficients.
2. **Structural identity.** The ordered sequence of non-`DHT` markers and their payload bytes must match
   exactly, the frame header must match field by field, every `DHT` table that the scan does not use must
   be carried over byte for byte, and the trailing bytes after `EOI` must be identical.
3. **Pixel identity through an independent decoder** (`jpeg-v1`, **not** in the default build). The original
   and the candidate are both decoded by `jpeg-decoder` — a library that shares no line with this
   repository — and the decoded pixels must be identical. The two sides are decoded one at a time and
   hashed with CRC-32, so verification never holds two extra images in memory (`7 × pixels` per side, as
   [safety §7.1](safety.md#71-memory--an-enforced-budget) charges). A disagreement is `failed`, exit 1;
   an original the library cannot read is `skipped` with that reason, because the oracle cannot be a
   verdict on a file it cannot decode. The feature is off by default because it costs more than the size
   condition of #39 allowed: 118 784 bytes at the release profile and 102 400 at `opt-level = "s"`, against
   68 KiB of headroom. `verify` says nothing less when it is absent, and §2.6 says which level a published
   build reaches.

If any condition fails the candidate is discarded and the original is not replaced. Condition 3 exists
because conditions 1 and 2 share a codec with the writer, and a defect symmetric in both directions — a
wrong symbol mapping used to encode and again to decode — round-trips "successfully" while the pixels
change. With it, the residual risk is a defect shared by two independently written decoders, plus the
`--no-default-features` builds that drop the feature on purpose. The corpus is also compared against
Pillow in CI ([testing §3.3](testing.md#the-third-party-decoder-job)), which is the same check on files
nobody on this project wrote.

**Default:** disabled. JPEG is processed only when `--formats` names it, because the gain is small and
the code path is the most intricate in the project.

**Verification IDs:** `T-JPG-01` … `T-JPG-08`.

---

### 4.8. TIFF (classic, baseline strips) — v0.7

**Transformation.** Rewrite the strip data and the offsets that point at it. Uncompressed strips
(`Compression = 1`) are compressed with zlib and the `Compression` entry becomes `8`; `Deflate` strips
(`Compression = 8` or `32946`) are recompressed, keeping whichever of the two encodings is smaller per
strip. The decoded bytes of every strip are identical to the input, which for `PhotometricInterpretation`
without predictor means identical pixels.

**Accepted subset** — the file is rewritten only when all of these hold, and refused with a reason
otherwise:

- classic TIFF only: byte order `II` or `MM`, magic `42`. BigTIFF (`43`) is refused;
- exactly **one** IFD. A non-zero next-IFD offset is refused (multi-page);
- strips, not tiles: tags `322`–`325` are refused;
- `PlanarConfiguration = 1` (interleaved), `Predictor` absent or `1` (none). Anything else is refused,
  because re-predicting a strip would change what the bytes mean;
- `Compression` ∈ {`1`, `8`, `32946`}. LZW (`5`), JPEG (`6`, `7`), PackBits (`32773`), CCITT, and every
  other compression are refused: rewriting LZW needs an encoder this build does not have, and pretending
  otherwise would mean writing a file whose strips do not decode;
- `FillOrder` absent or `1` (most significant bit first). `FillOrder = 2` — the bits of every sample
  reversed, which is what the `R` in Pillow's `hopper2R.tif` means — is refused, because libtiff reverses
  the bits of a *compressed* strip before it inflates it: a Deflate strip with `FillOrder 2` is a file the
  reference decoder rejects with `incorrect header check`. Compressing such a file would shrink it by
  breaking it. This one was found, not assumed: the third-party job of issue
  [#54](https://github.com/RUdolf1517/tzim/issues/54) ran Pillow over the corpus and five files —
  `pillow-tiff_gray_2_4_bpp_hopper{2,4,2I,4I}R.tif` and `pillow-16bit.r.tif` — came back unreadable
  *after* tzim had rewritten them, every one of them `FillOrder 2` and uncompressed;
- every out-of-line value belongs to a tag this build knows to be **plain data** (the list below). An
  unknown out-of-line tag is refused, because a value we cannot classify may be an absolute offset that
  the rewrite would invalidate. An unknown tag whose value fits in the entry's four value bytes is copied
  verbatim: it is not moved, so it cannot break;
- the only non-zero bytes outside the structures that are re-placed are the strips, the IFD, and the
  out-of-line values above. Any other non-zero byte would be data we cannot place, and the file is
  refused;
- entries are in ascending tag order, `StripOffsets` and `StripByteCounts` have the same count, and every
  strip lies inside the file.

**Tags this build treats as plain data.** An out-of-line value is moved only when its tag is on this list;
every other out-of-line tag is refused, because a value we cannot classify may be an absolute offset that the
rewrite would invalidate.

| Tags | What they are |
| --- | --- |
| 254, 255, 256, 257, 258, 259, 262, 263, 266 | baseline geometry, sample, compression and photometric fields |
| 269, 270, 271, 272 | `DocumentName`, `ImageDescription`, `Make`, `Model` |
| 273, 274, 277, 278, 279 | `StripOffsets`, `Orientation`, `SamplesPerPixel`, `RowsPerStrip`, `StripByteCounts` |
| 280, 281, 282, 283, 284, 286, 287 | sample value ranges, X/Y resolution, `PlanarConfiguration`, X/Y position |
| 292, 293, 296, 297 | T4/T6 options, `ResolutionUnit`, `PageNumber` |
| 305, 306, 315, 316, 317, 318, 319, 320 | `Software`, `DateTime`, `Artist`, `HostComputer`, `Predictor`, white point, primaries, `ColorMap` |
| 338, 339, 340, 341, 342, 343, 344, 347 | extra samples, sample format, transfer ranges, clip paths, JPEG tables |
| 530, 531, 532 | subsampling |
| 700 | XMP packet |
| 33432 | `Copyright` |
| 33550, 33922, 34735, 34736, 34737 | GeoTIFF data |
| 33723, 34377, 34675, 37510 | IPTC, Photoshop, ICC profile, `UserComment` |
| 37724 | `ImageSourceData` (Photoshop layers) |
| 40091, 40092, 40093, 40094, 40095 | Windows XP tags |
| 42112, 42113 | GDAL metadata and GDAL nodata |

**`--strip-metadata` removes exactly these tags and nothing else.** The list is fixed; it is the same list
this section promised above, and [§2.7](#27-g4--metadata-removed-on-request) repeats it for readers who
never reach this subsection.

| Tag | Name |
| --- | --- |
| 269 | `DocumentName` |
| 270 | `ImageDescription` |
| 271 | `Make` |
| 272 | `Model` |
| 305 | `Software` |
| 306 | `DateTime` |
| 315 | `Artist` |
| 316 | `HostComputer` |
| 333 | `InkNames` |
| 33432 | `Copyright` |
| 33723 | IPTC / NAA |
| 34377 | Photoshop image resources |
| 37510 | `UserComment` |
| 37724 | `ImageSourceData` |
| 40091–40095 | Windows XP tags (`XPTitle`, `XPComment`, `XPAuthor`, `XPKeywords`, `XPSubject`) |
| 42112 | GDAL metadata |

GDAL nodata (`42113`) is **kept**: it is a data tag, not a description of the data, and a tile without it can
render differently. Every other tag, including every unknown one, is preserved.

**Preserved:** byte order, the tag set, entry order and every entry's type and count; every value except
the three entries below, as resolved bytes rather than as bytes at a fixed position; `ImageWidth`,
`ImageLength`, `BitsPerSample`, `PhotometricInterpretation`, `SamplesPerPixel`, `RowsPerStrip`,
`ResolutionUnit`, the X/Y resolution rationals, `ColorMap`, `ExtraSamples`, `SampleFormat`, ICC profile,
XMP, EXIF-in-TIFF, ASCII tags and every private tag in the plain-data list above.

**Allowed to change:** `Compression` (only `1` → `8`; a Deflate file keeps `8`), `StripOffsets` and
`StripByteCounts` (recomputed for the new layout), the strip payloads, the offsets of the out-of-line
values, and therefore the length of the file.

**Verification.** Re-parse the candidate with the same strict reader (V3, [§2.6](#26-verification-independence)),
then:

1. entry count, order, tags, types and counts are identical;
2. every value is identical except `Compression`, `StripOffsets` and `StripByteCounts`;
3. each strip of both files is decoded — with `flate2` for Deflate, verbatim for uncompressed — and the
   decoded bytes are compared exactly (V1 for the content);
4. the candidate is accepted only when it is smaller than the input — enforced in the handler, not by the
   re-parse, and independent of `verify()`.

What the shared reader cannot catch is a structural field both sides misread; the strip bytes themselves
are compared through an independent inflater, so a content change cannot hide behind it.

**Removed by `--strip-metadata`.** Only the tags in this table, and only the entry itself: the tag
disappears from the IFD and its out-of-line value stops being written. The list is a constant in the code
(`tiff::STRIPPED_TAGS`) and a unit test compares the two, so a tag cannot be added to one without the other.

| Tag | Name | Why it is metadata |
| --- | --- | --- |
| 269 | `DocumentName` | the name of the scanned document |
| 270 | `ImageDescription` | free text about the image |
| 271 | `Make` | camera or scanner manufacturer |
| 272 | `Model` | camera or scanner model |
| 305 | `Software` | the program that wrote the file |
| 306 | `DateTime` | when the file was written |
| 315 | `Artist` | the person who made the image |
| 316 | `HostComputer` | the machine that produced the file |
| 333 | `InkNames` | the ink set of a separated file |
| 33432 | `Copyright` | the copyright notice |
| 33723 | `IPTC/NAA` | IPTC caption, keywords, credit |
| 34377 | `Photoshop` | Photoshop image resources (also carries IPTC) |
| 37510 | `UserComment` | free text |
| 37724 | `ImageSourceData` | Photoshop layer data |
| 40091 | `XPTitle` | Windows title |
| 40092 | `XPComment` | Windows comment |
| 40093 | `XPAuthor` | Windows author |
| 40094 | `XPKeywords` | Windows keywords |
| 40095 | `XPSubject` | Windows subject |
| 42112 | `GDAL_NODATA` | the nodata marker of a geospatial raster |

Everything else is kept, in particular `Orientation` (274), `XResolution`/`YResolution` (282/283),
`ResolutionUnit` (296), `ColorMap` (320), `ICCProfile` (34675), the XMP packet (700), `GeoKeyDirectory`
(34735) and the private tags. `42113` (`GDAL_METADATA`) is **kept** on purpose: it is not in the list.

**Refused (skipped, exit 0):** BigTIFF, tiles, more than one IFD, planar `2`, predictor other than `1`/
absent, `FillOrder 2`, LZW/JPEG/PackBits/CCITT compression, an unknown out-of-line tag, unplaceable
non-zero bytes, `StripOffsets`/`StripByteCounts` with different counts. A file of this kind is valid and
untouched.

**Why `FillOrder 2` is refused rather than honoured.** The tag says the bits inside every byte are stored
least significant first. Uncompressed, a decoder reverses them after reading and the file is fine.
Compressed, libtiff reverses the bits of the *zlib stream itself* before inflating — the stream no longer
starts with `78 9c`, and decoding stops at `incorrect header check`. That was checked by hand against
Pillow 13/libtiff on `hopper2R.tif`: deflating it and keeping `FillOrder 2` reproduces `decoder error -2`,
while the same file with `FillOrder 1`, or without the tag, reads back pixel-identical. Honouring the tag
would mean reversing the sample bits and rewriting `FillOrder` to `1` — a legal, lossless transformation,
but a change of representation this build has no test for. Refusing costs the compression of a handful of
Pillow test images; guessing would cost the contract.

**A file that is already damaged gets a different sentence, and the difference matters.** `FillOrder 2` with
`Compression` 8 or 32946 is a combination libtiff cannot read at all, and **tzim 0.7 through 0.12.1 wrote
exactly that** when it compressed an uncompressed reversed-bit strip: the bits of the stream it produced were
reversed along with the samples, so the result is not a zlib stream and never was. Those files are not
repairable, by this program or by any other — the compressed bytes are not recoverable as data, and
re-compressing a stream that is not a stream cannot recover the pixels. When a run meets one, it is refused
with a note that says so and points at the only way out, a backup or the original source; the alternative,
telling its owner that Deflate "would make the file unreadable", describes a future that has already happened
and sends them round the same loop again. Nothing is written either way, and the file is left byte-identical.

The honest summary of the incident: from 0.7 to 0.12.1 a rewrite could turn a readable TIFF into an unreadable
one, nothing inside the repository noticed, and what found it was the third-party job of v0.13 on its first
run — an implementation that shares no code with this one. A file already damaged by those versions cannot be
detected as damaged from its own bytes: it looks like a legitimate `FillOrder 2` Deflate file, which is why the
note describes the cause instead of promising a repair.

**Corrupt (failed, exit 1):** bad byte order or magic, an IFD outside the file, an entry count that runs
past the end, a strip outside the file, a zlib stream that does not decode, a decoded strip whose length
does not match `RowsPerStrip` × row bytes for the last strip — all leave the original untouched.

**Expected gain:** 5–30 % on uncompressed strips, 0–5 % on Deflate strips already written at level 9,
nothing on LZW (refused). This is the honest range: an uncompressed 8-bit RGB scan shrinks by roughly the
entropy of its content, while a TIFF from a modern scanner is often already Deflate- or LZW-compressed.

### 4.9. FLAC — refused after measurement

**Status: refused, contract kept.** The audit behind it is [flac-audit.md](flac-audit.md). It was measured
before it was decided: a stronger encoder buys **0.4 %** on musical material and **0 %** on files already
written at `flac -8`, while the encoder and the independent decoder together cost roughly **+150…300 KiB** of
binary — a quarter of it — for a format that is already entropy-coded to its limit. The numbers are in
[flac-audit §7](flac-audit.md#7-decision-refused-and-the-measurement-behind-it).

Nothing in this section is a claim that the binary supports FLAC: the handler does not exist,
`--list-formats` does not name the format, and `docs/formats.md` records it as refused. The contract is kept
because it is correct and free, and because a future encoder with a materially better search would restart
from here rather than from nothing. The clauses below are what such an implementation would have to satisfy.

**Transformation.** Re-encode the audio frames with a better encoder. The decoded samples are identical;
the frames, their lengths and the byte offsets that point at them are not, because a FLAC encoder is free
to choose the block size, the prediction order, the Rice partitioning and the stereo mode, and a different
encoder chooses differently.

**Accepted subset** — a file is rewritten only when all of these hold, and is skipped with a reason
otherwise:

- the stream starts with `fLaC` at offset 0. A leading ID3v2 tag is refused, not skipped over: it is
  outside the format, and a rewriter that walks past it is one offset bug away from damaging it;
- `STREAMINFO` is present, first, and exactly 34 bytes long, with a sample rate of at least 1 Hz, 1–8
  channels, a bit depth of 8, 16 or 24, and a non-zero `total samples`. A zero `total samples` means "unknown
  length" (a stream, not a file) and is refused;
- every metadata block type is one of the seven defined types (`0`–`6`). A reserved type (`7`–`126`) or the
  forbidden `127` is refused: an unknown block cannot be classified as metadata or as structure;
- block lengths add up to a position that lies inside the file, and the frames that follow decode;
- the audio is not encrypted or otherwise transformed outside the format.

LZW has a counterpart here: there is no "refuse the compression method" case, because FLAC has exactly one
entropy coding — but there is an equivalent, and it is the encoder. If the encoder cannot express a
combination the input uses (a block size outside its range, a bit depth it does not implement), that is a
refusal with the reason, never a conversion.

**Preserved:** the marker; every metadata block, byte for byte, in its original order — `PADDING`
(including its length), `APPLICATION`, `VORBIS_COMMENT`, `CUESHEET` and `PICTURE`; the `STREAMINFO` fields
that describe the audio: sample rate, channel count, bit depth, total samples, and the MD5 of the unencoded
audio.

**Allowed to change:** every frame; the frame headers' coded numbers and CRCs; `STREAMINFO`'s min/max block
size and min/max frame size, which describe the frames and must be recomputed correctly; each `SEEKTABLE`
point's byte offset, and only that — its sample number and its frame length are recomputed from the same
sample position, and a point that cannot be resolved makes the file a refusal; the file length.

**Verification.** The candidate is decoded with a decoder that is not the encoder (`claxon`, against
`flacenc`; see [§2.6](#26-verification-independence)), and:

1. both files decode to the same sample rate, channel count and bit depth;
2. the decoded sample streams are compared exactly — byte equality, no tolerance, no resampling. The input's
   frames are decoded through the same path, so this also proves the *input* still decodes identically;
3. the MD5 of the candidate's decoded audio equals the MD5 in the candidate's `STREAMINFO` and the MD5 in
   the input's `STREAMINFO`. When the input stores a zero MD5 ("not computed", which is legal), clause 2
   carries the proof alone and the candidate's own MD5 must still be correct;
4. every metadata block other than `STREAMINFO` and `SEEKTABLE` is byte-identical, and appears in the same
   order and at the same length;
5. `STREAMINFO`'s sample rate, channel count, bit depth, total samples and MD5 are unchanged; its min/max
   block size and min/max frame size match the frames actually written;
6. every `SEEKTABLE` point resolves to the frame that contains its sample number, and its frame length is
   that frame's;
7. the coded frame numbers cover the whole stream: no gap, no overlap, no frame beyond `total samples`;
8. the candidate is strictly smaller than the input.

Clause 2 is what makes FLAC the strongest contract in this document: the comparison is between integers,
and the only way to pass it with damaged audio is to damage the reference the same way.

**Refused (skipped, exit 0):** a leading ID3v2 tag, a reserved or forbidden metadata block type, a zero
sample rate or zero total samples, a bit depth the encoder does not implement, more than 8 channels, a
`SEEKTABLE` point that does not resolve, a `STREAMINFO` that disagrees with the frames it precedes. The file
is valid and is left untouched.

**Corrupt (failed, exit 1):** a truncated stream, a frame CRC that does not match, a metadata length that
runs past the end of the file, a decoder error in the input. All leave the original untouched.

**`--strip-metadata`** removes `VORBIS_COMMENT`, `PICTURE` and `PADDING` and keeps `STREAMINFO`,
`SEEKTABLE`, `CUESHEET` and `APPLICATION`. With the flag the acceptance rule becomes "never larger"
([§2.7](#27-g4--metadata-removed-on-request)); without it, `PADDING` is preserved byte for byte and the
rule is "strictly smaller".

**Expected gain:** 0–3 %, and 0 % is the common answer. A file from `flac -8` is already what the format
allows; files from older settings, hardware recorders or low-effort rips have 1–3 % left. This contract does
not promise more than the format can give, and the tool's refusal to write a larger file is what protects
the user from the rest.

### 4.10. zlib (RFC 1950) — v0.9

**Transformation.** Recompress the DEFLATE stream. The two header bytes and the four trailer bytes are
copied verbatim; only the bytes between them change.

**Preserved, byte-identically:**

- the two header bytes exactly as they appear in the input: `CM`, `CINFO`, `FLEVEL` and `FCHECK`. `CINFO`
  is a promise about the encoder's window, so preserving it is not cosmetic; `FLEVEL` is a hint about the
  level that produced the stream, and a stale hint is still the input's own value;
- the decompressed payload, byte for byte and in length;
- the trailing `Adler-32`, big-endian (recomputed from the decompressed bytes, and required to equal the
  input's value);
- the number of streams (exactly one).

**Allowed to change:**

- the DEFLATE body bytes and length;
- the total file length.

**Accepted subset** — the file is rewritten only when all of these hold, and is skipped with a reason
otherwise:

- `CM = 8` (DEFLATE). Every other method is refused;
- `CINFO ≤ 7`, and `CINFO = 7` (a 32 KiB window) in particular. A stream that declares a smaller window is
  refused: this build's encoder always uses a 32 KiB window and is free to emit distances up to it, and a
  decoder that honours the declared window would then reject or mis-decode the result. Refusing is the only
  answer that cannot produce a broken file;
- `FDICT = 0`. A preset dictionary means the payload cannot be decoded without bytes that are not in the
  file, so there is nothing to verify against;
- the two header bytes, read big-endian, are a multiple of 31 (RFC 1950 §2.2), and neither the header nor
  the DEFLATE stream is truncated;
- the DEFLATE stream ends exactly where the trailer begins. A second stream or any trailing byte is refused
  rather than concatenated or dropped;
- the trailer's `Adler-32` matches the decompressed payload. A mismatch is *corruption*, not a variation.

**Verification** — structure **V3** (this repository's header reader), content **V1** (`flate2`/`miniz_oxide`
decodes both files, and the `Adler-32` is recomputed by code in this repository, not by the encoder):

1. both files parse under the accepted subset;
2. the candidate's two header bytes equal the input's;
3. the candidate's four trailer bytes equal the input's;
4. decompressing both files yields identical byte sequences and identical lengths;
5. `candidate.len() < input.len()`.

**Refused (skipped, exit 0):** `CM ≠ 8`, `CINFO < 7`, `FDICT = 1`, trailing data after the stream. The
file is valid and is left untouched.

**Nothing decodes behind the header (unsupported, exit 0):** the two header bytes pass the container's
check and the data decodes to nothing at all. There are two possible explanations — the header was a
coincidence (see "A note on detection" below), or the stream is a real zlib stream cut too short to produce
a byte — and the decoder cannot tell them apart (#62). Neither is corruption, so the run keeps its exit code;
the note names both possibilities and the file is untouched. This is the one case where a handler overrules
the detector, and only a handler that can prove the difference is allowed to do it.

**Corrupt (failed, exit 1):** a header that fails the check value, a truncated stream, an `Adler-32` that
does not match, and a stream that decoded at least one byte before it broke. All leave the original
untouched. The distinction from the row above is the whole rule: **nothing decoded — unsupported; something
decoded and then failed — corrupt.** A truncated `.zlib` that produced bytes must not hide behind exit 0.

The rule has one cost, and it is the price of making it decidable: a stream cut so short that **nothing at all**
decodes — a large file truncated to a few dozen bytes — is reported `unsupported` rather than corrupt, because
at that length it is byte-for-byte indistinguishable from a coincidental header. Nothing is lost but the
label and the exit code: the file is untouched either way.

**`--strip-metadata`** does nothing here, and says nothing: a zlib header carries no name, no comment and no
timestamp, and `FLEVEL` describes the encoder rather than the file. The flag is accepted and the output is
the same as without it.

**A note on detection.** A zlib header is two bytes with internal redundancy — `CM = 8`, `CINFO ≤ 7`,
`FDICT = 0`, a multiple of 31 — which is weaker than a magic number: exactly 1 of the 1984 two-byte
sequences that can start a file satisfies it. Detection is therefore a *hint*, and the handler validates
everything again before writing anything. The bytes are never at risk; what a false positive used to cost was
a `failed` line, which is why the split above exists. A tree of 2000 unrelated files has a **~63 %** chance
(1 − (1983/1984)^2000) of containing one, and until v0.12.1 that file turned a whole `tzim -r` run red.

Detection is not made stronger to fix this: `BTYPE = 3` in the first block would divide the probability by
roughly 1.33 and change nothing that matters, and a trial decode at detection time would cost a pass over
every file the tool does not recognise.

**Expected gain:** 0–10 %, and it depends entirely on who wrote the file. Streams from a default `zlib`
level 6 — the level every library, HTTP client and tool uses by accident — have something left, but on small
inputs it is little: the measured set in [benchmarks §3.2](benchmarks.md#32-against-the-established-tools) is
twelve real source files at level 6 and gains **0.88 %**. Streams already written at level 9 have nothing,
and a stream from an old or lazy encoder has far more than a percent. `--zopfli` adds the usual 1–5 % at
10–100× the CPU cost.

**Raw DEFLATE is not supported, deliberately.** A bare DEFLATE stream has no header, no trailer and no
signature: there is no byte sequence that identifies one, so a file cannot be recognised without trying to
decompress every file passed to the tool and accepting whatever happens to parse. That guesses, and guessing
is the one thing this project does not do. It is refused in [formats §5](formats.md#5-refused-formats) with
the same reasoning.

**Verification IDs:** `T-ZL-01` … `T-ZL-09`.

---

---

### 4.11. PDF — contract written, not implemented

**Status: not implemented, and not yet justified.** No PDF code exists, `--list-formats` does not name the
format, and `docs/formats.md` §5 records PDF as refused. This contract is written because it is the only way to
make the question decidable: [pdf-audit](pdf-audit.md) pre-registers the measurement that would admit or refuse
the format, and these are the clauses an implementation would have to satisfy if it did. Nothing here is a claim
of support.

The shape of the opportunity is different from FLAC's and worth stating: a `FlateDecode` payload **is** an
RFC 1950 stream, so the transformation is the one this document already specifies in
[§4.10](#410-zlib-rfc-1950--v09), applied inside a container. There is no PDF codec to write and no new
dependency.

**Transformation.** Inflate the payload of each `FlateDecode` stream, deflate it again at a higher level, and put
a smaller stream back. Nothing else is re-encoded, renumbered or reordered.

**Accepted subset** — a file is rewritten only when all of these hold, and is skipped with a reason otherwise:

- the header is found within the first 1024 bytes, and everything before it is copied verbatim;
- the cross-reference section parses: a classic `xref` table or a `/Type /XRef` stream, with every entry
  resolving to an object header at the offset it names. A file that needed "repair" to be read is refused, not
  repaired;
- exactly one `%%EOF`, and no trailer `/Prev`: an incrementally updated file is refused, because flattening the
  revisions is repacking;
- no `/Encrypt` in the trailer;
- no `/ByteRange` signature anywhere in the document;
- no `/Linearized` dictionary and no hint streams;
- every stream that is rewritten has `/Filter` equal to the single name `FlateDecode`, with no `/F`, and its
  `/Length` agrees with `endstream`;
- its payload is a well-formed RFC 1950 stream — `CM = 8`, `CINFO ≤ 7`, `FDICT = 0`, the check value valid and
  the Adler-32 equal to the inflated bytes. Anything else is refused rather than repaired, as in §4.10;
- objects held in an `ObjStm` are left alone: the container is copied verbatim. A file whose only compressed
  payloads are inside object streams is therefore `already_optimal`, not an error.

**Preserved, byte-identically:**

- the header, the junk before it, and every byte between objects;
- every object number, generation number, and the order in which objects appear;
- every dictionary, including `/Info`, `/Metadata`, `/ID`, `/PieceInfo` and `/DecodeParms`, with one exception:
  the value of `/Length` for a stream that was rewritten, when that length is a direct value;
- every stream that was not rewritten;
- the inflated content of every stream that was rewritten.

**Allowed to change:**

- the bytes of each rewritten `FlateDecode` stream, and their length;
- the `/Length` value of those streams (direct, or the object holding it when it is an indirect reference);
- the cross-reference section and `startxref`, because offsets move;
- the total length of the file.

`/ID` is **not** updated. The specification recommends changing its second element on an update; preserving it is
the choice here, because a changed identifier is a changed document identity, and this project does not normalise
metadata it was not asked to remove.

**Verification** — structure **V3** (this repository's own reader, the same level as the ZIP structure), content
**V1** (`flate2` inflates both sides, and the Adler-32 is recomputed by code in this repository):

1. both files parse, and every xref entry points at an object header at the offset it names;
2. the set of `(object number, generation)` pairs is identical, in the same order;
3. every dictionary compares equal, except `/Length` of the rewritten streams;
4. every rewritten stream inflates to bytes identical to the original's, and its recomputed Adler-32 equals the
   original stream's;
5. every other stream is byte-identical;
6. `startxref` resolves to the xref section, and `/Root` and `/Info` name the same objects as before;
7. `candidate.len() < input.len()`.

Clause 4 is the one that carries the trust, and it is the same instrument §4.10 uses. What this cannot prove is
stated in the audit rather than here: no object-level comparison proves that a document *renders* the same, and
the rendering check lives in CI with a real renderer, not in the binary.

**Refused (skipped, exit 0):** encryption, signatures, linearization, incremental updates, a broken xref, object
streams as a rewrite target, any filter other than a single `FlateDecode`, a payload that is not a valid zlib
stream, a `/Length` that disagrees with `endstream`, external stream data (`/F`), duplicate object numbers.

**Corrupt (failed, exit 1):** a header that cannot be found within its bound, an object that runs past the end of
the file, a trailer without `startxref`, a document whose `/Root` does not resolve. All leave the original
untouched.

**`--strip-metadata`** is not part of this contract. `/Info`, `/Metadata` and `/PieceInfo` are objects, not
streams; removing them is [G4](#27-g4--metadata-removed-on-request) work with its own arguments and is not
claimed here.

**Expected gain:** unmeasured, and the audit says why (§3, §9). The honest expectation is that most heavy PDFs
are scans whose streams are already entropy-coded, that a `FlateDecode` payload written at level 6 has a few
percent in it, and that 0 % will be a common answer. The measurement in [pdf-audit §9](pdf-audit.md#9-the-measurement-method-corpus-and-the-rule-that-is-fixed-in-advance)
fixes the threshold in advance: median ≥ 3 % with at least 10 % of the corpus accepted admits the format;
median < 1 % refuses it with the numbers on record.

---

### 4.12. FITS — contract written, not implemented

**Status: not implemented, and not yet justified.** No FITS code exists and `--list-formats` does not name the
format. FITS was never refused; it was absent, and it is now specified so that the question becomes measurable
([fits-audit](fits-audit.md)). Nothing in this section is a claim of support.

Two preconditions, from [fits-audit §0](fits-audit.md#0-what-is-a-fact-here-and-what-has-to-be-re-read-from-the-source),
must be confirmed against the convention before any code is written, because both change the design rather than
its details: that a `GZIP_1`/`GZIP_2` tile payload is an RFC 1950 stream, and the exact byte permutation the
`GZIP_2` shuffle applies.

**Transformation.** Two tiers, admitting different amounts of change.

- **Tier 1 (recompress).** In a tile-compressed image HDU (`ZIMAGE = T`) whose `ZCMPTYPE` is `GZIP_1` or
  `GZIP_2`: inflate every tile, deflate it again at a higher level, rebuild the table's heap and the `P`/`Q`
  descriptors that address it. The file remains a `BINTABLE`; no card other than the derived ones changes.
- **Tier 2 (compress).** In an uncompressed image HDU: write the same array as a tile-compressed `BINTABLE`
  under the registered tile-compression convention. The numbers are identical; the representation is different.
  This is the same class of change as TIFF `Compression 1 → 8` in §4.8.

**Accepted subset** — a file is rewritten only when all of these hold, and is skipped with a reason otherwise:

- `SIMPLE = T` starts the file at offset 0; every HDU is a whole number of 2880-byte records;
- every header ends with an `END` card inside its records, and no card is truncated;
- for each image HDU, `|BITPIX|/8 × Π NAXISn` equals the data length the file actually holds;
- `BITPIX` is one of 8, 16, 32, 64, −32, −64;
- `GROUPS = T` is absent;
- `BSCALE` is not zero;
- Tier 1: `ZCMPTYPE` is `GZIP_1` or `GZIP_2`; every `ZTILEn` divides the corresponding `ZNAXISn`; every heap
  descriptor points inside the heap; every tile payload is a well-formed RFC 1950 stream whose Adler-32 matches
  its inflated bytes;
- Tier 2: the HDU is an image HDU of a supported `BITPIX`; the run asked for it (`--fits-tile`);
- `CHECKSUM`/`DATASUM`, when present, are recomputed correctly or the file is refused — a stale checksum is a
  corrupted file by the format's own definition.

**Preserved, byte-identically:**

- every HDU that was not rewritten, including its header, its data and its padding;
- in a Tier 1 rewrite: every card except `NAXIS1`, `NAXIS2`, `PCOUNT`, `CHECKSUM` and `DATASUM` of that HDU —
  including `ZSIMPLE`, `ZBITPIX`, `ZNAXIS`, `ZNAXISn`, `ZTILEn`, `ZCMPTYPE`, `ZNAME`, `ZBLANK`, `BSCALE`,
  `BZERO`, `ZHECKSUM` and `ZDATASUM`;
- in a Tier 2 rewrite: every card of the original header survives, either as itself or as the `Z`-prefixed card
  the convention defines for it; no keyword is dropped ([I3](#3-global-invariants));
- the array of numbers, in every tier: the inflated bytes, compared element by element as bytes. `BSCALE` and
  `BZERO` are never applied, never normalised and never written back in a different form.

**Allowed to change:**

- the compressed tile bytes and their lengths;
- the `P`/`Q` heap descriptors, `NAXIS1`, `NAXIS2` and `PCOUNT` of the HDU that was rewritten;
- the padding at the end of header records and after the heap;
- `CHECKSUM` and `DATASUM`, recomputed from the new bytes;
- in Tier 2 only: `XTENSION`, `TFIELDS`, `TTYPEn`, `TFORMn`, the `Z*` cards and the `BITPIX`/`NAXIS` pair that
  become `ZBITPIX`/`ZNAXISn`;
- the total length of the file.

No byte offset elsewhere in the file has to be updated: FITS has no pointer table. That is the single largest
simplification this format offers over TIFF and PDF, and it is why the correctness argument here is short.

**Verification** — structure **V3** (this repository's own card and table reader), content **V1** (`flate2`
inflates the candidate's tiles and the bytes are compared with the original array):

1. both files parse, every HDU begins and ends on a 2880-byte boundary, and the HDU count is unchanged;
2. every HDU that was not rewritten is byte-identical;
3. in Tier 1, every card other than the five named above is byte-identical, and the number of rows and tiles is
   unchanged;
4. in Tier 2, every original keyword is present in the candidate either verbatim or in its `Z` form, and no
   external keyword was invented;
5. **the round trip**: decompressing the candidate's tiles and concatenating them in the order the convention
   defines yields bytes identical to the original array. This is the clause that carries the trust, and it is
   stronger than anything TIFF or PDF can offer — there is no decoder, no pixel model and no renderer in the
   path;
6. `CHECKSUM`/`DATASUM`, when written, verify against an independent computation;
7. `candidate.len() < input.len()`, and under `--strip-metadata` the rule of [§2.7](#27-g4--metadata-removed-on-request).

**Refused (skipped, exit 0):** `ZCMPTYPE` other than `GZIP_1`/`GZIP_2`; a tile payload that is not a valid zlib
stream; `SIMPLE = F`; `GROUPS = T`; an ASCII `TABLE` or a `BINTABLE` without `ZIMAGE` as a rewrite target (it is
copied verbatim); an inconsistent `Z*` set; a heap descriptor outside the heap; `BSCALE = 0`; a header without
`END`.

**Corrupt (failed, exit 1):** a data length that disagrees with `NAXIS`/`BITPIX`; a header record that runs past
the end of the file; a `CHECKSUM` that does not match the bytes it covers. All leave the original untouched.

**`--strip-metadata`** removes nothing here. Every FITS card is either structure or observation metadata —
`OBJECT`, `OBSERVER`, `DATE-OBS`, `HISTORY`, `COMMENT` are the record of an observation, not decoration — and
the compression convention has no field that exists only for human convenience. The flag is accepted and the
output is the same as without it, as for zlib in §4.10.

**Expected gain:** unmeasured, estimates in [fits-audit §3](fits-audit.md#3-where-the-bytes-are--honest-estimates-not-measurements).
The measurement and its pre-registered thresholds are in
[fits-audit §8](fits-audit.md#8-the-measurement-and-the-rule-fixed-in-advance): Tier 2 needs a median of 8 % or
more on at least 15 % of the corpus and ships behind a flag; Tier 1 needs 2 % and ships on by default; if
neither clears its bar, the format is refused with the numbers on record.

---

### 4.13. DICOM — contract written, not implemented

**Status: not implemented, and not yet justified.** No DICOM code exists and `--list-formats` does not name the
format. This contract is written for the same reason §4.11 was: it makes the question decidable.
[dicom-audit](dicom-audit.md) pre-registers the measurement that would admit or refuse each of the three tiers,
and what follows are the clauses an implementation would have to satisfy if one were. Nothing here is a claim of
support.

The shape of the opportunity is **not** the one PDF and FITS have, and that difference is the whole reason this
section exists. A Deflated Explicit VR Little Endian data set is compressed with **raw DEFLATE (RFC 1951)** — no
zlib header, no Adler-32 — as both pydicom and DCMTK implement it, and as DCMTK states in a comment when the
alternative is compiled in: a zlib header there *"IS NOT DICOM COMPLIANT"*. So this is the *bare* DEFLATE that
[formats](formats.md) §5 refuses for want of a signature, and it is readable here only because the File Meta
Information group names the encoding. The identification is the meta group; the transform is §4.10 without the
container.

**Tier 1 — recompress a deflated data set.** Inflate the stream after the meta group, deflate it again at a higher
level, and put a smaller stream back. The data set is **never parsed** and no data dictionary is required.

**Accepted subset, Tier 1** — a file is rewritten only when all of these hold, and is skipped with a reason
otherwise:

- `DICM` at offset 128;
- the File Meta Information group parses as Explicit VR Little Endian, contains only defined-length elements, and
  its `(0002,0000)` group length agrees with where the group ends;
- `(0002,0010)` is `1.2.840.10008.1.2.1.99`;
- the bytes after the group are one DEFLATE stream that inflates to completion;
- nothing follows the stream except, at most, one `0x00` padding byte, and only when the stream length is odd.

**Preserved, byte-identically:** the preamble, `DICM`, and every element of the meta group, including
`(0002,0000)`, whose value counts the meta group and not the file.

**Allowed to change:** the compressed bytes of the data set, their length, the presence of the single pad byte,
and the total length of the file.

**Tier 2 — deflate an explicit-VR data set** is specified and gated, not promised: the same file with transfer
syntax `1.2.840.10008.1.2.1` is walked element by element to prove the encoding is honest, then written as a
DEFLATE stream under `1.2.840.10008.1.2.1.99`. Every inflated byte is identical to the original data set; what
changes is the transfer syntax, the meta group length, and the encoding of the whole data set. That is a change a
reader without the deflated syntax cannot survive at all, so it is **opt-in behind `--dicom-deflate`** in the
pre-registered rule, whatever its gain. **Tier 3 — translating implicit VR to explicit VR — is refused**: it
needs a 5 189-entry dictionary (measured, [dicom-audit §0](dicom-audit.md#0-what-is-a-fact-here-and-what-has-to-be-re-read-before-coding))
to recover the VRs and it re-encodes every element header, which is re-encoding rather than compression.

**Verification** — structure **V3** (this repository's own meta-group reader), content **V1** with `flate2`:

1. both files parse, and every meta-group boundary lands where `(0002,0000)` says;
2. every meta byte is identical;
3. Tier 1: both streams inflate, and the two inflated byte sequences are equal;
4. Tier 2: the candidate inflates to exactly the original data set, byte for byte;
5. `candidate.len() < input.len()`, and under `--strip-metadata` the rule of [§2.7](#27-g4--metadata-removed-on-request).

**The weakness, stated rather than implied.** The deflated stream carries no checksum, so the original side of a
Tier 1 comparison is anchored by nothing — the same position TIFF's already-Deflate strips are in (§2.6), and one
step worse, because there is not even a header to validate. A defect symmetric in `flate2`'s inflate and deflate
would survive. Tier 2 does not have this weakness: its original side is raw bytes that no decoder touched.

**Refused (skipped, exit 0):** no `DICM`; a meta group that does not parse or whose length disagrees with itself;
any transfer syntax other than the two above, including all 39 encapsulated ones; encapsulated pixel data in a
file offered to Tier 2; a stream that does not inflate; trailing bytes that are not a single pad byte, or a pad
byte when the length is already even; a data set above the byte budget.

**Corrupt (failed, exit 1):** a `DICM` signature followed by a meta group that ends in the middle of an element.
It leaves the original untouched.

**`--strip-metadata`** removes nothing here: every meta-group element is either identification or the encoding
itself, and this project does not invent a new transfer syntax to save a few bytes.

**Expected gain:** unmeasured. Estimates are in
[dicom-audit §3](dicom-audit.md#3-where-the-bytes-are--honest-estimates-not-measurements), and the point of that
section is that the answer depends almost entirely on what a corpus contains. The measurement and its
pre-registered thresholds are in [dicom-audit §8](dicom-audit.md#8-the-measurement-and-the-rule-fixed-in-advance):
Tier 1 needs a median of 2 % and ships on inside a `dicom` feature; Tier 2 needs 15 % and ships behind
`--dicom-deflate`; the implicit-VR tier needs the corpus to be more than half implicit-VR bytes to justify its
dictionary; if nothing clears, DICOM is refused with the numbers on record, exactly as FLAC was.

---

## 5. What is not preserved

Tzim replaces the file on the filesystem, not only its bytes. The following are **not** preserved and are
listed here so that nobody has to guess:

| Property | Status |
| --- | --- |
| POSIX permission bits | preserved (best effort; see [safety](safety.md)) |
| Modification time (mtime) | preserved |
| Windows read-only attribute | preserved |
| Owner / group | **not guaranteed** — the new file is created by the current user; a group may differ on setgid directories. Do not run Tzim as root on files belonging to other users. |
| ACLs, extended attributes, alternate data streams | **not preserved** |
| Creation time / birth time, inode number, ctime | **not preserved** (replacement creates a new inode) |
| Hard links | **not** followed: a file with more than one hard link is skipped by default (`--allow-hardlinks` overrides), because replacing it would silently split the link |
| Sparse regions | **not preserved** |
| Filesystem-level compression or immutable flags | **not preserved** |
| Orphan temporary file after `SIGKILL` | possible; the original is always intact |

Any file whose metadata matters to you (ACLs, xattrs, ownership) should be backed up before running any
optimizer, including this one.

This section is about **filesystem** metadata. Embedded **content** metadata (Exif, ICC, comments, ZIP extra
fields) is a different thing and is preserved byte for byte — except under `--strip-metadata`, which removes
the fixed list in §2.7.

---

## 6. Contract evolution

- This document is the source of truth for behaviour, not the code.
- Adding a format or changing a contract clause requires, in the same pull request: the contract text, the
  verification routine, tests covering the new clause, and a `CHANGELOG.md` entry.
- Removing a preservation guarantee is a breaking change and requires a major version bump.
- A format may only be advertised in the README table after its contract exists here and its verification
  tests pass in CI.
