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
4. accepting the candidate only when it is not larger than the input (strictly smaller unless
   `--strip-metadata` removed something, see [§2.7](#27-g4--metadata-removed-on-request)). This rule is
   enforced where the candidate is accepted — in the handler and, as the last word, in the pipeline — not
   inside `verify()`, which compares content and not size.

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
| **V1 — independent decoder** | A decoder written by someone else, from the specification, with no knowledge of this codebase. | PNG and ICO pixels (`png` crate), gzip and ZIP payloads (`flate2`, `miniz_oxide`) |
| **V2 — separate reader** | Code in this repository, used only for reading, that the writer's output never passes through. | *Not used anywhere today.* |
| **V3 — shared implementation** | The candidate is parsed by the same reader the handler used to build it. | ZIP structure, ICO directory, JPEG markers and JPEG coefficients |

| Format | Structure | Content | What a shared bug could hide |
| --- | --- | --- | --- |
| PNG | V3 (own chunk walker) | **V1** (`png` crate decodes both files, pixels compared) | A framing error that both sides accept; the IDAT payload is compared through an independent inflater, so it cannot hide a content change |
| gzip | V3 (own header walker) | **V1** (`flate2` decodes both bodies; CRC-32 and ISIZE recomputed) | A header misreading; the payload comparison would still be independent |
| zlib | V3 (own header reader) | **V1** (`flate2` decodes both streams; `Adler-32` recomputed here, not by the encoder) | A header misreading; the payload comparison and the checksum would still be independent |
| ZIP | V3 (own central-directory reader) | **V1** per entry (`flate2` + recomputed CRC-32) | A structural field both sides misread; content is checked by an independent inflater and a checksum the writer had to compute from the bytes |
| ICO | V3 (own directory reader) | **V1** for embedded PNGs, byte comparison for bitmaps | A directory field both sides misread |
| TIFF | V3 (own IFD walker; every tag value compared field by field) | **V1** for strips that were uncompressed — the original side is raw bytes and needs no decoder; shared `flate2` for strips that were already Deflate | A structural field both sides misread would pass. Unlike gzip, zlib and ZIP there is **no external checksum** anchoring the original side: those three carry a CRC-32, an Adler-32 or an ISIZE written by the original archiver, while TIFF strips carry none |
| JPEG | V3 | **V3** — coefficients are decoded and re-encoded by the same codec | A defect that is *symmetric* in the encoder and the decoder: a wrong symbol mapping used in both directions round-trips "successfully" while the pixels change |

**The honest consequence for JPEG.** Its verification is the weakest of the seven, and it cannot be
strengthened without an independent JPEG decoder, which would mean a new dependency and a much larger
binary. What is in place instead: a strict parser that refuses anything it cannot fully validate, a
coefficient-level comparison (stronger than comparing pixels, which a second decoder would give), the
fixtures and the real-file corpus, and a fuzz target for the entropy reader. None of that is a proof. A
symmetric codec defect would pass every check in this repository. This is the main reason JPEG is opt-in and
why its gain is not oversold.

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
| **I2** | A candidate is only used if `candidate.len() < original.len()`. A larger or equal candidate is discarded, with one exception: under `--strip-metadata`, an equal-size candidate is accepted when the only change is a removed metadata field ([§2.7](#27-g4--metadata-removed-on-request)). A larger candidate is never accepted, under any flag. |
| **I3** | No metadata, comment, attachment, vendor chunk, private field or unknown structure is removed or normalized. The single exception is the explicit `--strip-metadata` request, which removes the fixed list in §2.7 and nothing beyond it. |
| **I4** | No lossy transformation, no format conversion, no quality, resolution, rate or bit-depth change. |
| **I5** | If the input changes on disk between the initial stat and the replacement, the operation is aborted. |
| **I6** | Verification never reuses the encoder. Where the checker necessarily shares a parser with the writer — the JPEG codec and the ZIP structure — the sharing, and what it cannot catch, are stated in §2.6 instead of being called independence. |
| **I7** | The candidate is written to a temporary file in the same directory and replaces the original only by an atomic rename where the platform supports it (see [safety](safety.md)). |
| **I8** | Anything that cannot be parsed with full confidence is refused with a clear message, never "repaired". Tzim does not attempt to fix broken files, and never writes an output for an input it could not fully validate. |

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

**Refused:** malformed frame sequences (`fcTL`/`fdAT` counts inconsistent with `acTL`), frames whose data
does not decode.

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

**Limits:** `--max-depth` (default 3) and the parent's `--max-uncompressed` budget apply to the whole tree.

**Verification IDs:** `T-NEST-01` … `T-NEST-04`.

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

**Verification.** The candidate is re-parsed from bytes and decoded again. Two conditions must hold:

1. **Coefficient identity.** Every DCT coefficient of every component is compared with the input's. This
   is strictly stronger than comparing decoded pixels: identical coefficients imply identical pixels,
   while identical pixels do not imply identical coefficients. No pixel comparison is performed, because
   that would need a second decoder and the same result is already implied.
2. **Structural identity.** The ordered sequence of non-`DHT` markers and their payload bytes must match
   exactly, the frame header must match field by field, every `DHT` table that the scan does not use must
   be carried over byte for byte, and the trailing bytes after `EOI` must be identical.

If either condition fails the candidate is discarded and the original is not replaced. The residual risk
is stated in [benchmarks](benchmarks.md): the decoder that proves the candidate readable is Tzim's own,
so a decoder bug shared by the encoder and the checker would not be caught by this step; the structural
comparison and the strict table validation narrow that to a genuinely malformed table, which the table
validity checks reject before anything is written.

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

**Refused (skipped, exit 0):** BigTIFF, tiles, more than one IFD, planar `2`, predictor other than `1`/
absent, LZW/JPEG/PackBits/CCITT compression, an unknown out-of-line tag, unplaceable non-zero bytes,
`StripOffsets`/`StripByteCounts` with different counts. A file of this kind is valid and untouched.

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

**Corrupt (failed, exit 1):** a header that fails the check value, a truncated stream, an `Adler-32` that
does not match, a DEFLATE stream that does not decode. All leave the original untouched.

**`--strip-metadata`** does nothing here, and says nothing: a zlib header carries no name, no comment and no
timestamp, and `FLEVEL` describes the encoder rather than the file. The flag is accepted and the output is
the same as without it.

**A note on detection.** A zlib header is two bytes with internal redundancy — `CM = 8`, `CINFO ≤ 7`,
`FDICT = 0`, a multiple of 31 — which is weaker than a magic number: roughly one random file in two thousand
passes it. Detection is therefore a *hint*, and the handler validates everything again before writing
anything. A false positive costs one `failed` line and an untouched file, never a rewrite. The bytes are
never at risk, only the line count.

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

**Verification IDs:** `T-ZL-01` … `T-ZL-08`.

---

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
