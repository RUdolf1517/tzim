# Formats

Related: [guarantees](guarantees.md) · [roadmap](roadmap.md) · [size budget](size-budget.md)

Tzim supports a format only when all of the following are true: a contract exists, a verification routine
exists, a fixture corpus exists, and CI proves the contract holds on that corpus. Until then the format is
listed as *planned* and is not advertised anywhere else.

---

## 1. Detection

Formats are detected by **content**, never by file name:

| Format | Signature / check |
| --- | --- |
| PNG | `89 50 4E 47 0D 0A 1A 0A` |
| APNG | PNG signature plus a valid `acTL` chunk before the first IDAT |
| gzip | `1F 8B` and `CM = 08` |
| ZIP | `50 4B 03 04` (local header) or `50 4B 05 06` (empty archive) |
| zlib | `CM = 8`, `CINFO ≤ 7`, `FDICT = 0`, and the two bytes big-endian divisible by 31 (RFC 1950 §2.2) |
| FITS | `SIMPLE  =` in the first nine bytes, the value `T`, and a file that is a whole number of 2880-byte records |
| FLAC | `fLaC` at byte zero |
| PDF | `%PDF-` within the first 1024 bytes; the handler validates the version, xref and object structure |
| ICO / CUR | `00 00 01 00` / `00 00 02 00` plus a valid directory |

The extension is used only for two things: the `--include` / `--exclude` filters, and human-readable
messages. A `.png` that is not a PNG is reported as unsupported and is never modified. A PNG named
`.dat` is still optimized. This prevents an entire class of "the tool destroyed my file because the
extension lied" incidents.

Detection is defensive: every signature check is followed by a structural sanity check with strict limits
on sizes and counts, so a few magic bytes in a random file cannot cause a large allocation or a long loop.

---

## 2. Format matrix

| Format | Version | Status | Transformation | Expected gain | Risk | Contract |
| --- | --- | --- | --- | --- | --- | --- |
| PNG | v0.1 | implemented | IDAT recompression; scanline filter re-selection; optional Zopfli | 5–45 % on unoptimized files; ~0–3 % on already-optimized ones | medium | [§4.1](guarantees.md#41-png--v01) |
| gzip (`.gz`, `.tgz`, `.svgz`) | v0.1 | implemented | DEFLATE body recompression, header verbatim | 0–15 % | low | [§4.2](guarantees.md#42-gzip--v01) |
| ZIP family | v0.2 | implemented | DEFLATE entry recompression, everything else preserved | 0–20 %, depends on the level used by the original archiver | high | [§4.3](guarantees.md#43-zip--v02) |
| ICO / CUR | v0.3 | implemented | Optimize embedded PNGs | equals the embedded PNG gain | medium | [§4.4](guarantees.md#44-ico--cur--v03) |
| APNG | v0.3 | implemented | Recompress all frames | as PNG, proportional to frame data | medium | [§4.5](guarantees.md#45-apng--v03) |
| Nested (`--nested`) | v0.3 | implemented, opt-in | Optimize supported files inside archives | additive | medium | [§4.6](guarantees.md#46-nested-containers-v03-opt-in) |
| JPEG (baseline, default) | v0.4 / #82 | implemented | Entropy re-encoding with optimal Huffman tables; exact DCT-coefficient comparison plus independent V1 pixel check; markers preserved | typically 1–3 %, sometimes 0 | high | [§4.7](guarantees.md#47-jpeg--v04-baseline-default) |
| TIFF | v0.7 | implemented | Deflate strip recompression, or Deflate for uncompressed strips; tags, entry order and byte order untouched | 5–30 % on uncompressed scans; 0–5 % on already-Deflate ones; nothing for LZW or `FillOrder 2` | high | [§4.8](guarantees.md#48-tiff-classic-baseline-strips--v07) |
| zlib (`.zlib`, `.zz`) | v0.9 | implemented | Recompress the DEFLATE stream; header and Adler-32 preserved byte for byte | 0–10 %, depends on the level that wrote the file | medium | [§4.10](guarantees.md#410-zlib-rfc-1950--v09) |
| FITS (opt-in) | v0.14 / #59 | implemented in v0.14.0, behind the `fits` feature and `--fits-tile` | An uncompressed image HDU becomes a tile-compressed `BINTABLE` (`ZIMAGE`, `GZIP_1`/`GZIP_2`, quantisation off, one tile per row); every other HDU copied verbatim. A single-file TTY run asks default No after verification; scripted use of the flag remains explicit consent | The a1 default-checksum `funpack` outcome is superseded and is not candidate-content evidence. The corrected #59 gate uses `funpack -C`, an external integer-only positive control through the same literal-header/storage-array comparator, and structured per-HDU diagnostics. Four TESS `fitsverify -l` checksum warnings remain blockers; threshold stays not applied and all 14 Tzim rows stay skipped/unchanged | high for the implemented subset; full admission remains open and the #59 gate is blocked | [§4.12](guarantees.md#412-fits--tile-compressed-image-hdus-v014-opt-in) · [status](measurement-status.md) |
| FLAC (opt-in) | #79 | **implemented in this unreleased branch behind the non-default `flac` feature; fixed-corpus evidence recorded, full format admission remains separate** | Re-encode supported audio with flacenc; verify exact PCM, STREAMINFO and remapped seekpoints with independent Claxon; metadata is retained unless explicitly stripped | At exact SHA `5967e318951f9c3945f0c256c3095b347483e234`, 23/30 valid rows optimized; all 23 passed external `flac`/`metaflac`, PCM, and STREAMINFO-MD5 checks. Saved 744,570 bytes; whole-corpus and optimized-only denominators are distinguished in [status](measurement-status.md) | high for the measured subset; broader format admission/review remains open | [§4.9](guarantees.md#49-flac--frame-re-encoding-79-non-default-opt-in) |
| PDF (opt-in) | #57 / #80 | **implemented in this unreleased branch behind the non-default `pdf` feature; #57 threshold applied and refused** | Recompress only smaller eligible FlateDecode streams; preserve object IDs/order, xref semantics, object streams and all non-target bytes; no repacking | At exact SHA `5967e318951f9c3945f0c256c3095b347483e234`, 18/21 fixed outputs were accepted; the original accepted-subset `faithful_pct` median was 0.00034127460376596526%, below the 1% refusal bar. Permitted-qpdf and forbidden-repack distributions are published in [PDF audit §13](pdf-audit.md#13-pr-84-exact-head-57-rerun-at-5967e31); the qpdf/MuPDF checks passed | high for the narrow implementation; the threshold result does not revoke the separate owner-authorized non-default #80 implementation | [§4.11](guarantees.md#411-pdf--flatedecode-recompression-80-non-default-opt-in) |

### ZIP-based formats covered by the ZIP handler

`.zip`, `.jar`, `.war`, `.ear`, `.apk`, `.aab`, `.docx`, `.xlsx`, `.pptx`, `.odt`, `.ods`, `.odp`, `.epub`,
`.cbz`, `.whl`, `.nupkg`, `.xpi`, `.ipa`, `.love`, `.vsix`, and anything else that is structurally a ZIP
file. Structurally safe to rewrite does not mean accepted: `.apk`, `.aab`, `.ipa`, `.vsix` and OOXML/ODF
documents usually carry signatures or signing metadata, and the signed ones are refused by the checks
below.

A refused archive is reported as `skipped`, left untouched, and does not fail the run; corrupt archives are
reported as `failed`. Two categories are deliberately refused, because a byte-level rewrite would silently
invalidate them:

- **Signed archives** — JAR/APK signatures (`META-INF/*.SF`, `*.RSA`, `*.DSA`, `*.EC`, APK signing block).
  The bytes would be preserved according to the contract while the artifact stopped being verifiable. That
  is exactly the kind of technically-true-but-practically-false claim this project exists to avoid.
- **APK alignment** — zipalign stores absolute offsets in extra fields; rewriting the archive invalidates
  them. Unsigned APKs are the only safe subset, and the signature check already catches most real APKs.

---

## 3. Expected gains, honestly

Two independent data points, both from the stage-1 analysis:

- **Already-optimized files.** On 17 real PNG icons shipped by a Linux distribution, recompression with the
  best of {same filters, re-selected filters} at `zlib -9` gave a **median gain of 1.2 %**; 10 of 17 files
  shrank at all, and 7 were larger than the original with every candidate, so the correct outcome was "leave
  it alone".
- **Unoptimized files.** Public benchmarks of comparable tools report 15–45 % reductions on PNGs exported
  from editors, and Zopfli adds roughly 1–5 % on top of `zlib -9` at 10–100× the CPU cost.

Consequences for the design, all of which are implemented rather than advertised:

1. Tzim always computes several candidates and keeps the smallest — never "apply level N and hope".
2. Tzim never writes a candidate that is not strictly smaller — or, with `--strip-metadata` and nothing to
   remove but a timestamp, one that is the same size. Never a larger one.
3. Zopfli is opt-in (`--zopfli`), because on already-compressed files it costs minutes and buys little.
4. Documentation must not promise large savings. **Tzim's value is breadth, trust and a small binary, not
   the best possible ratio for PNG** — for that, `oxipng -o max` is a better tool and is named as such.

---

## 4. Admission criteria for a new format

A format may be added only when the pull request contains all of the following:

1. **A contract** in [guarantees.md](guarantees.md): what is preserved byte-exactly, what may change, and
   the guarantee class.
2. **A verification routine** that is independent of the writer, using either a third-party decoder or a
   separately written reader.
3. **Detection** by content with strict structural validation and explicit size limits.
4. **A refusal list**: every construct the handler does not fully understand causes a refusal, not a
   best-effort rewrite.
5. **A fixture corpus** covering valid, edge-case, and corrupted samples, committed with provenance and
   licence information.
6. **Tests** proving the contract, including at least one case where the candidate must be discarded.
7. **A measured size cost**: the binary-size delta of the new format, recorded in
   [size-budget.md](size-budget.md).
8. **A dependency review** if new crates are required, following
   [dependencies.md](dependencies.md).
9. **Documentation**: README table, this matrix, and the known limitations.
10. **An honest note about expected gains**, including when the answer is "usually nothing".

If any item is missing, the format stays out. There is no "experimental support" tier.

---

## 5. Refused formats

A recorded refusal remains policy unless the owner authorizes a follow-up. #79 and #80 are authorized
implementation tracks and their handlers are present in this unreleased branch behind non-default features;
that implementation status does not constitute full format admission or performance acceptance. The #57/#59
fixed-manifest results are now recorded separately: #57 applied its original rule and refused the measured
sample, while #59's threshold remains **not applied**. The a1 `funpack` comparison omitted `-C`, so its 14/14
header/payload outcome is superseded as an instrument result, not evidence of candidate content failure. The
corrected reader path uses `funpack -C`, an external integer-only positive control through the same literal
comparison path, and structured per-HDU diagnostics. Four TESS `fitsverify -l` checksum warnings remain blockers;
no owner protocol decision is pending. Neither result changes the fixed inputs or the separate #80 implementation
authorization. The reader-mode details are in [FITS audit §12](fits-audit.md#12-corrected-59-reader-mode-protocol);
§11 preserves the historical comparison. Every actual refusal still has a reason that follows from the invariant.

| Format | Why Tzim refuses |
| --- | --- |
| **PDF outside the narrow handler subset / without the `pdf` feature** | **Not handled by default.** The #80 implementation in this unreleased branch rewrites only a bounded single-revision classic-xref or supported xref-stream subset. It does not repack object graphs, flatten revisions, or claim full PDF admission. The fixed #57 sample applied the original accepted-subset `faithful_pct` rule and was refused because its median was below 1%; the separately owner-authorized #80 implementation remains non-default. The old 2 208-file audit and thresholds are retained as historical evidence, and qpdf repacking remains forbidden. See [PDF audit §11](pdf-audit.md#11-pr-84-follow-up--57-fixed-21-output-sample) and [guarantees §4.11](guarantees.md#411-pdf--flatedecode-recompression-80-non-default-opt-in). |
| **DICOM** | Never refused — never examined, until now. The question this row used to ask (is the dictionary or the refusal list the larger cost?) is answered in [dicom-audit](dicom-audit.md): the dictionary is, it is **5 189 entries measured**, and it is needed by exactly one of three tiers. Under measurement is a recompress-only slice — a data set already compressed as Deflated Explicit VR Little Endian, whose stream is **raw DEFLATE (RFC 1951), not zlib**, and therefore carries no checksum — specified in [guarantees §4.13](guarantees.md#413-dicom--contract-written-not-implemented), with the decision rule fixed before the numbers. Until that run exists, the format is not supported. |
| **WebP (lossless)** | No mature pure-Rust lossless encoder exists; a C dependency breaks static cross-platform builds. Re-encoding could also change the bitstream in ways we cannot independently verify sample-by-sample without the same library that produced it. |
| **AVIF / HEIC / JPEG XL** | Same as WebP: no verifiable pure-Rust lossless re-encoder. Lossless JPEG transcoding to JXL is a format change (G2), which Tzim does not do. |
| **MP4 / MOV / MKV / WebM / AVI** | The only available savings come from removing metadata or re-muxing, which changes data. Re-encoding is lossy. |
| **MP3 / AAC / Opus / Vorbis** | Nothing to gain without re-encoding, which is lossy. Moving ID3 tags changes bytes and risks data. |
| **HTML / CSS / JS / SVG / XML / JSON** | "Optimization" here means minification: removing comments, whitespace and redundant constructs. That is information removal and is forbidden. SVGZ is supported because it is a gzip file, and only the gzip layer is touched. |
| **BMP / WAV / TXT / raw data** | No internal compression; the only way to shrink them is to change format, which Tzim does not do. |
| **xz / bzip2 / zstd / 7z / RAR** | No pure-Rust encoder exists for any of them, and [dependencies §1](dependencies.md#1-decision-procedure) forbids a C toolchain in the build. For **zstd** that is the only obstacle: the format itself is the best of this group by our own criteria — a four-byte magic (`28 B5 2F FD`), an optional XXH64 checksum in the frame, a flat sequence of frames and blocks rather than an object graph, and 10–30 % available between level 3 and level 19. If a pure-Rust encoder at level 15+ appears, zstd moves ahead of every other candidate here. For xz, bzip2 and 7z the obstacle is the same and the gain is smaller, because those encoders are already near their ceiling. |
| **Raw DEFLATE** | A bare DEFLATE stream has no header, no trailer and no signature: nothing identifies one, so recognising it would mean attempting to decompress every file passed to the tool and accepting whatever parses. That is guessing. The zlib container (above) is supported precisely because it has a header and a checksum to check. |
| **GIF** | LZW re-encoding yields at most a couple of percent while requiring a full LZW codec and careful handling of extensions, disposal and transparency. The risk/benefit ratio is worse than for any supported format. |
| **Office binary formats (.doc, .xls, .ppt)** | Compound File Binary containers with in-place structures; rewriting them safely is not tractable. Their OOXML successors are ZIP files and are supported through the ZIP handler. |
| **SQLite databases, mailbox files, disk images** | In-place page structures; any rewrite is a different operation (vacuum, reindex) with its own semantics and its own failure modes. |

A "no" here is not a judgement about other tools. Several of these are optimized well by specialized tools
that accept metadata loss or format changes. Tzim does not.

---

## 6. Nested processing policy

`--nested` is opt-in and off by default, because it changes what a file contains inside, which is a larger
promise than most users expect.

When enabled:

- only formats with a contract are optimized inside an archive;
- the parent contract is checked as usual, and each child must pass its own verification independently;
- a child that fails is kept unmodified;
- `--max-depth` (default 3) and `--max-uncompressed` bound the work and protect against decompression bombs;
- a signed archive is never opened for nested processing (it is refused as a whole).

---

## 7. Priority rationale

Why the order in the roadmap is PNG → gzip → ZIP → ICO/APNG → JPEG:

1. **PNG** is the format with the largest realistic average saving and the most mature verification story.
2. **gzip** is nearly free once DEFLATE support exists and covers `.gz`, `.tgz` and `.svgz`.
3. **ZIP** is the highest-value container after PNG, and one parser covers a dozen popular formats — but its
   contract is the hardest to get right, so it ships after the replacement pipeline is proven.
4. **ICO and APNG** reuse the PNG engine and add coverage cheaply.
5. **JPEG** is the most common image format, but the gain is small and the codec risk is the highest. It
   shipped last with pixel-level verification; #82 later admitted that verified baseline subset to the
   release default within the unchanged binary-size ceiling.

Documents and PDF are excluded not because they are unpopular, but because "popular" and "safe to rewrite"
are different properties, and only the second one matters here.
