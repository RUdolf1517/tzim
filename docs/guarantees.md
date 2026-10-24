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
| **Verification** | A mechanical check performed on the candidate file, using a decoder that is independent of the code that produced the candidate. |
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

G1 is verified by comparing file length and a full content hash of the file before and after the run.

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
2. an **independent** decode of both input and candidate, comparing content;
3. comparing the inventory of significant properties and preserved byte ranges;
4. confirming the candidate is strictly smaller than the input.

---

## 3. Global invariants

These hold for every format, in addition to the per-format contract.

| ID | Invariant |
| --- | --- |
| **I1** | The original file is never modified unless a candidate has passed every check. |
| **I2** | A candidate is only used if `candidate.len() < original.len()`. A larger or equal candidate is discarded. |
| **I3** | No metadata, comment, attachment, vendor chunk, private field or unknown structure is removed or normalized. |
| **I4** | No lossy transformation, no format conversion, no quality, resolution, rate or bit-depth change. |
| **I5** | If the input changes on disk between the initial stat and the replacement, the operation is aborted. |
| **I6** | Verification never relies on the code that produced the candidate. Decoding uses an independent library or an independently written reader that is only used for reading. |
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

### 4.8. TIFF and FLAC — research, no contract yet

No contract exists for these formats, therefore **no support is claimed and no code will be written** until
a contract and its verification method are specified in this document. This ordering is deliberate: the
document comes first.

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

---

## 6. Contract evolution

- This document is the source of truth for behaviour, not the code.
- Adding a format or changing a contract clause requires, in the same pull request: the contract text, the
  verification routine, tests covering the new clause, and a `CHANGELOG.md` entry.
- Removing a preservation guarantee is a breaking change and requires a major version bump.
- A format may only be advertised in the README table after its contract exists here and its verification
  tests pass in CI.
