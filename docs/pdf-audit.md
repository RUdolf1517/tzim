# PDF: the audit the contract has to pass

Related: [guarantees §4.11](guarantees.md#411-pdf--flatedecode-recompression-80-non-default-opt-in) · [formats](formats.md) ·
[dependencies](dependencies.md) · [size-budget](size-budget.md) · [roadmap](roadmap.md)

This is stage 1 for PDF — the audit a contract is allowed to follow, the same rule the FLAC work followed
([flac-audit](flac-audit.md)). The order matters: the contract ([§4.11](guarantees.md#411-pdf--flatedecode-recompression-80-non-default-opt-in))
says what an implementation would have to preserve; this document says what such an implementation could
possibly win and what it would cost.

**Historical status:** the 2,208-file audit and its §9 refusal remain unchanged historical evidence. The owner
subsequently authorized a narrow, non-default #80 implementation, which is present in this unreleased branch.
The earlier exact 21-output #57 run at SHA `7b82c9edb6e5060bc8ce7ed756caebba91505b5b` is preserved in §11.
The corrected run at exact SHA `5721867ab1439814cead5645d4f443d8f8876c9c` is retained in §12, and the subsequent
exact-head rerun at `5967e318951f9c3945f0c256c3095b347483e234` is in §13. The original accepted-subset
`faithful_pct` rule was applied and returned **refused after measurement** in both runs because the median was
below 1%. This result does not revoke the separate #80 opt-in implementation decision, does not make PDF a release
default, and does not authorize the forbidden repack comparator. The exact distributions and reports are in
§§11–13 and the [measurement status](measurement-status.md).

Two things in this document are facts about the format and are checkable without a corpus: the structure in §1–§4
and the refusal list in §6. The gain figures in §3 are **estimates from what produces PDFs**, not measurements,
and are marked as such.

---

## 1. What the file is

| Part | Shape | Notes |
| --- | --- | --- |
| header | `%PDF-1.x`, at most 1024 bytes of junk before it | readers tolerate the junk; a rewriter has to preserve it or refuse |
| body | `n g obj … endobj`, in any order, gaps allowed | object number `n`, generation `g`; both are referenced from elsewhere |
| stream object | dictionary, `stream`, raw bytes, `endstream` | `/Length` gives the byte count and may be an indirect reference |
| `ObjStm` | a stream holding other objects' bodies | `/N` objects, `/First` offset; the contained objects have no byte range of their own |
| cross-reference | `xref` table with 20-byte entries, or a `/Type /XRef` stream with `/W` and `/Index` | both are legal in the same document at different revisions |
| trailer | `/Size`, `/Root`, `/Info`, `/ID`, `/Prev`, `startxref <offset>`, `%%EOF` | `/Prev` chains the revisions of an incrementally updated file |
| encryption | `/Encrypt` in the trailer | strings and streams are encrypted; RC4, AES-128, AES-256 |
| linearization | `/Linearized` dictionary, hint streams, a first-page xref at the front | every offset is recorded twice, at both ends of the file |
| signature | `/ByteRange [a b c d]` and a `/Contents` placeholder | the signature covers the bytes the range names, so any rewrite invalidates it |

## 2. The only operation that is ours

PDF offers three ways to make a file smaller. They are not variations on one theme; they are different
products, and only the first is compatible with the invariant of this project.

| Layer | Operation | Typical gain | Verdict |
| --- | --- | --- | --- |
| **recompress** | inflate each `FlateDecode` stream, deflate it again at a higher level, put it back | unmeasured, §3 | the only candidate |
| **repack** | merge objects into object streams, renumber, rebuild the xref, linearize | 5–15 % on some files | forbidden: the object graph changes, signatures and byte ranges break, and the result is a different document layout |
| **remove** | drop unreferenced objects, strip `/Info`, drop `PieceInfo` | unpredictable | forbidden without a flag: that is data removal, [G4](guarantees.md#27-g4--metadata-removed-on-request) |

**The important consequence, and the reason this audit exists at all:** a `FlateDecode` payload *is* an RFC 1950
stream — two header bytes with a check value, DEFLATE data, a big-endian Adler-32 — which is exactly what
[§4.10](guarantees.md#410-zlib-rfc-1950--v09) already implements, already verifies and already refuses when the
header is malformed. The PDF handler would be a **container walker plus the existing zlib path**, not a new
codec: no new dependency, no new compression algorithm, no new checksum to trust.

That is the whole difference between this audit and the FLAC one. FLAC needed 12 000 lines of someone else's
code and a quarter of the binary. PDF needs a parser for a format this repository already reads elsewhere (the
ZIP handler is also a container whose payloads are inflated by `flate2`), and it reuses the zlib handler's
verified recompression for the payload.

## 3. Where the bytes are — and an honest guess at the gain

| Producer | What its streams look like | Expected gain |
| --- | --- | --- |
| a scanner, or any PDF built from images | `DCTDecode` / `JPXDecode` / `JBIG2Decode`, already entropy-coded | **0 %** — nothing here is a zlib stream |
| pdfTeX, Ghostscript at its default, macOS "Print to PDF", Word/LibreOffice | `FlateDecode` at level 6–9 | **0–3 %** |
| old or home-grown generators, report tools, some library defaults | `FlateDecode` at level 6, no object streams | **3–10 %** |
| files with uncompressed streams (`/Filter` absent) | raw bytes | 40–70 %, but rare and often already broken |

Heavy PDFs — the ones that dominate a disk — are scans, and scans are the 0 % row. That is the same shape of
answer FLAC gave, and it is why the measurement in §7 has to report the **distribution** of accepted files and
not an average over a corpus that is half text.

`--strip-metadata` changes nothing here: `/Info`, `/Metadata` and `/PieceInfo` are objects, not streams, and
removing them is G4 work with its own arguments. This audit is about the flag being *absent*.

## 4. What a recompress may change

Only these five things, and every one of them is a place where a mistake is a broken file:

1. **`/Length`** of each rewritten stream, and the object holding it when it is an indirect reference.
2. **Every byte offset in the cross-reference section**, because a shorter stream moves everything after it. If
   the xref is a table, it is re-emitted; if it is a `/Type /XRef` stream, that stream is re-emitted too.
3. **`startxref`**, which names the offset of the xref section.
4. **The file length.**
5. **`/ID`'s second element**, if the implementation follows the recommendation to change it on update — the
   contract says preserve `/ID` verbatim instead, because a changed ID is a changed document identity and this
   project does not normalise metadata.

The design that keeps the diff reviewable: **copy the original byte ranges of everything that is not rewritten**
and re-emit only the streams that shrank, plus the xref and `startxref`. Object numbers, generation numbers,
object order, dictionaries, whitespace between objects and every other stream stay byte-identical.

### 4.1 A correction to an earlier reading

Predictors are **not** a problem, and an earlier draft of this audit listed them as a refusal. Recompressing a
stream does not touch the predicted data: `Predictor ≥ 10` in `/DecodeParms` applies *after* inflation, so the
inflated bytes that are compared and re-deflated are the same bytes either way. A predictor only matters to
something that re-encodes the image, which this project never does.

## 5. Encoder and decoder: who writes, who checks

| Role | Choice | Why |
| --- | --- | --- |
| deflate | `flate2` (already a dependency) | the same verified path [§4.10](guarantees.md#410-zlib-rfc-1950--v09) uses; no new crate |
| structure reader | this repository's own parser | V3, exactly as the ZIP handler reads its own central directory |
| structural oracle | `qpdf` in CI | a second implementation, written by someone else, that refuses malformed output and can flatten, check and re-render a document |
| render oracle | `mutool draw` or `pdftoppm` in CI | compares page images for a sample; catches the class of defect no object-level comparison can |

`qpdf` is C++, so it cannot enter the binary under
[dependencies §1](dependencies.md#1-decision-procedure). As a **test** oracle it costs nothing in size and is
worth more than anything the binary can do on its own — the same argument as issue #54 for the other formats.

## 6. Refusal list

Every row below is a refusal with a reason, not a best-effort rewrite. This list is the spine of
[§4.11](guarantees.md#411-pdf--flatedecode-recompression-80-non-default-opt-in); if a construct is not listed as accepted, it is
refused.

| Construct | Why refuse |
| --- | --- |
| `/Encrypt` in the trailer | stream bytes are ciphertext; `/Length` describes the encrypted data |
| any `/ByteRange` signature, including a PAdES signature in a `/Sig` field | the signature covers the bytes, so a rewrite makes it invalid — silently |
| `/Linearized` | hint streams and offsets at both ends of the file; the layout is a promise to viewers |
| more than one `%%EOF`, or a trailer with `/Prev` | an incremental update has to be flattened to be rewritten, and flattening is repacking |
| a xref that does not resolve, or a "repair" was needed to read it | if the file's own offsets lie, there is nothing to preserve |
| `ObjStm` | its contents have no byte range of their own, so no object inside it can be rewritten; it is copied verbatim and yields nothing |
| `/Filter` containing anything except a single `FlateDecode` | a filter chain has to be re-encoded end to end; `ASCII85Decode`, `LZWDecode`, `RunLengthDecode`, `DCTDecode`, `JPXDecode`, `JBIG2Decode`, `Crypt` are all outside the transform |
| `FlateDecode` payload that is not a well-formed zlib stream (raw deflate, missing or wrong `FCHECK`, wrong Adler-32) | the same rule [§4.10](guarantees.md#410-zlib-rfc-1950--v09) already applies: refuse, do not repair |
| a stream whose `/Length` disagrees with `endstream` | a lying length is the classic way a PDF is subtly broken |
| `/F` in a stream dictionary (data in an external file) | the bytes are not in the document |
| an object number that appears twice, or a generation number above what the xref declares | ambiguity in the object graph cannot be resolved safely |
| junk before the header that the parser cannot bound | the boundary of the document is unknown |

## 7. What verification can prove — and the one thing it cannot

1. both files parse, and every xref entry points at an object header at the offset it names;
2. the set of object numbers and generation numbers is identical, in the same order;
3. every dictionary is byte-equal, except `/Length` of the streams that were rewritten;
4. every stream that was rewritten inflates to **byte-identical** content, and its recomputed Adler-32 equals
   the original's — the payload comparison is V1 (`flate2` and this repository's own checksum code);
5. every stream that was not rewritten is byte-identical in the candidate;
6. `startxref` resolves to the xref section, and the trailer's `/Root` and `/Info` name the same objects;
7. the candidate is strictly smaller than the input;
8. the size rule of [I2](guarantees.md#3-global-invariants) applies unchanged.

**What it cannot prove: that the document renders the same.** No object-level comparison can, because rendering
is a program, not a function of the bytes. The claim is narrower and honest: *the object graph is identical and
every decompressed stream is byte-identical*, so any renderer that read the original reads the candidate the
same way — unless it depended on a byte offset, in which case clause 6 is the part that protects it. A rendering
comparison belongs in CI (§5), where a real renderer can be run, not in the binary.

This is the difference from FLAC that decides the trust argument: FLAC's verdict rested on comparing integers.
PDF's rests on an argument about offsets and on a renderer that lives in a test job.

## 8. Cost

| Item | Estimate | Basis |
| --- | --- | --- |
| new dependencies | **0** | `flate2` is already there; `qpdf` is CI only |
| handler code | 1 500–2 500 lines | the parser is comparable to the ZIP structure reader, plus the xref and `/Length` bookkeeping |
| binary size | **+40…70 KiB** | this repository's TIFF handler is +36 KiB, the JPEG codec 52 KiB; a parser with no codec sits between them |
| memory | the file plus its candidate, as everywhere | [safety §7.1](safety.md#71-memory--an-enforced-budget) |

The current headroom is 155 KiB ([benchmarks §1](benchmarks.md#1-binary-size--measured)). A `pdf` feature would fit — as an
opt-in feature, not in `default`, the way JPEG was introduced — but it would leave roughly half of that
headroom. **The measurement in §9 is worth doing after #53**, not before: dropping `indicatif`/`console` buys
92–96 KiB on every row, which is more than this feature costs.

## 9. The measurement: method, corpus, and the rule that is fixed in advance

**Corpus.** Three pinned upstream sets plus a real-world sample, assembled the way
[`.github/scripts/corpus.sh`](../.github/scripts/corpus.sh) already does it — URL, tag and SHA-256 of the
archive recorded, never the files:

| Set | Why it is there |
| --- | --- |
| `mozilla/pdf.js` `test/pdfs` | hand-built edge cases: object streams, xref streams, incremental updates, encrypted files |
| `qpdf/qpdf` `qpdf/qtest` inputs | malformed and repaired files, the cases that must be refused |
| `veraPDF` corpus | PDF/A and PDF/UA, the files institutions actually archive |
| 20–50 documents from real toolchains | what a person's disk looks like: LaTeX, Word/LibreOffice, macOS print, a scan or five |

**Tools.** `qpdf` 11 for the object level, `mutool draw` for rendering.

| Question | Command |
| --- | --- |
| What can our narrow slice win? | `qpdf --recompress-flate --compression-level=9 --object-streams=preserve in.pdf out.pdf` |
| What does the forbidden repack add on top? | `qpdf --recompress-flate --compression-level=9 --object-streams=generate in.pdf out.pdf` |
| Is the output sound? | `qpdf --check out.pdf` |
| Does it render the same? | `mutool draw -o out-%d.png out.pdf` against the original, byte-compare the PNGs |

**What the run must report** — one row per file, then aggregated:

| Metric | Meaning |
| --- | --- |
| accepted / refused, and refusals by reason | which of §6 fires most often; a corpus that is 80 % signatures is a different answer from one that is 80 % scans |
| bytes saved, per file, as a percentage | the **distribution**: median, quartiles, and the share of files above 0 % |
| extra bytes the forbidden repack would add | what the invariant costs, stated in bytes rather than in adjectives |
| render differences | must be zero; a non-zero count ends the discussion |

**The decision rule, fixed now.** After the numbers exist:

- **median gain in the accepted subset ≥ 3 % and at least 10 % of the corpus accepted** → the contract is
  accepted, and a handler is a candidate for a release, as an opt-in `pdf` feature;
- **median gain < 1 %** → refused with the numbers recorded, exactly as FLAC was, and the §5 row gains the
  measurement;
- **between 1 % and 3 %** → decided by cost: refuse if the binary delta turns out above 70 KiB, accept otherwise.

The rule is written here so that a later reader can check whether the decision followed it. A measurement that
is not pre-committed to a threshold is a search for an excuse.

### 9.1 What was run

The instrument is committed: [`.github/scripts/pdf-measure.py`](../.github/scripts/pdf-measure.py) reproduces
the numbers in §9.2 from the checkouts listed below, with the pinned commits and the two `pikepdf`/`PyMuPDF`
settings in its header. It is not part of the build and not a dependency of the crate. A number nobody can
re-run is a claim; this is the script that makes it a measurement.

`qpdf` 11 was not installable here; **libqpdf 12.4.2** came through `pikepdf` and `mutool draw`'s engine was
**MuPDF 1.28.5** through `PyMuPDF` — the same libraries, a major version later, driven in-process instead of
from a shell. The two jobs of the table above are exactly the two jobs below: `--recompress-flate
--compression-level=9` with `--object-streams=preserve` for the permitted transform, and with
`--object-streams=generate` for the forbidden repack.

**Corpus**, pinned by repository and commit (a checkout pins the exact bytes; an archive hash does not survive a
re-tarball):

| Set | Repository | Commit | Files | Bytes |
| --- | --- | --- | --- | --- |
| `test/pdfs` | `mozilla/pdf.js` | `606bd6a4e138` | 1 009 | 124.7 MB |
| `qpdf/qtest` inputs | `qpdf/qpdf` | `4eba95899886` | 630 | 38.2 MB |
| PDF/A-1b corpus | `veraPDF/veraPDF-corpus` | `bb75f4f0073d` | 569 | 8.9 MB |
| **total** | | | **2 208** | **171.9 MB** |

The fourth set the rule asks for — 20 to 50 documents from real toolchains — could not be assembled as a separate
sample, because a runner has no Word and no LaTeX. It is not missing from the numbers, only from the provenance:
the three upstream sets contain files from those toolchains, and their `/Producer` strings say so. Of the 1 009
accepted files, 224 carry a producer; the medians by producer are `cairo 1.18.0` 0.74 %, `PDFium` 0.72 %,
`iText` 0.24 %, `Adobe Acrobat Pro` 0.07 %, `Microsoft Word` 0.01 %, `Scribus` 0.01 %,
`TCPDF` 0.01 %. The other 788 carry no producer at all, which is itself a fact about hand-built test
corpora. **This is the run's weakness and is not papered over:** a corpus that is half hand-built edge cases is
not a person's disk, and the one row §3 expected to be heavy — scans — is represented here mostly as
`DCTDecode` streams that the transform never touches.

### 9.2 The result

| Metric | Value |
| --- | --- |
| files | 2 208 |
| accepted into the §4.11 slice | **1 009 (45.7 %)** |
| `already_optimal` — parses, nothing inside the slice to rewrite | 723 (32.7 %) |
| refused | 476 (21.6 %) |

**Refusals by reason**, which the rule calls half the answer:

| Reason | Files |
| --- | --- |
| incremental update (more than one `%%EOF`, or `/Prev`) | 332 |
| damaged — `qpdf --check` reports an error or a warning | 40 |
| unreadable even with recovery disabled | 40 |
| encrypted | 21 |
| filter chain (more than one filter, or not `FlateDecode`) | 15 |
| no `%PDF-` header in the first 1 024 bytes | 15 |
| `FlateDecode` payload is not a well-formed RFC 1950 stream | 4 |
| signed (`/ByteRange`) | 3 |
| truncated zlib stream | 2 |
| `FlateDecode` with a payload under six bytes | 2 |
| `/Filter` present but not DEFLATE | 2 |

**Bytes saved, per file, as a percentage of the file** — the distribution the rule asks for:

| Encoder | Median | Q1 | Q3 | Mean | Share > 0 | Max |
| --- | --- | --- | --- | --- | --- | --- |
| `flate2` at level 9 — **the transform §4.11 specifies**, and §5's chosen encoder | **0.08 %** | 0.00 % | 0.09 % | 0.39 % | 68 % | 29.7 % |
| `zopfli`, on a 150-file sample (the project already ships it as `--zopfli`) | **0.15 %** | 0.14 % | 2.81 % | — | 91 % | — |
| `qpdf --recompress-flate --object-streams=preserve` — the instrument of this table | 0.59 % | −0.07 % | 1.94 % | 1.68 % | 60 % | 96.4 % |

The three rows disagree in the direction that matters, and the reasons are worth stating rather than averaging:

- **`qpdf`'s number is the highest because it does more than the contract allows.** With `--object-streams=
  preserve` it still rebuilds `ObjStm` containers, which §6 refuses to touch, so its 0.59 % includes repacking
  that a conforming handler would not do. The measured payload sets differ for 91 of the 1 009 files for exactly
  this reason.
- **`zopfli` is not the pre-registered encoder** (§5 names `flate2`), so its number is recorded as a fact, not
  as the basis of the decision. It buys a few per cent per *stream* — a median of 5.6 % on a 60-stream sample —
  but almost nothing per *file*, because most accepted files are mostly not Flate payloads. It is also ~18 KB/s
  on this corpus, which is a minute for a one-megabyte document.

**What the forbidden repack would have added:** median **20.1 %**, mean 17.0 % of the file. That is the price of
the invariant in bytes: §4.11's refusal to repack costs about a fifth of every PDF that enters the slice, and it
is the single largest number in this audit.

**Soundness and rendering.** `qpdf --check` passed on **1 009 of 1 009** candidates. A page-image comparison was
run with MuPDF at 100 dpi on a random sample of **140 files, 173 pages**: **zero differences**, pixel for pixel.
No `--check` failure and no render difference, so the rule's "a single render difference ends the discussion"
clause was not triggered.

### 9.3 The decision

| Rule | Threshold | Measured | Verdict |
| --- | --- | --- | --- |
| admit | median ≥ 3 % **and** ≥ 10 % of the corpus accepted | 0.08 %, 45.7 % | gain fails by a factor of 37 |
| refuse | median < 1 % | **0.08 %** — and 0.15 % with the strongest encoder, 0.59 % with the loosest instrument | **refuse** |

**PDF is refused after measurement.** Not because the slice is small — 45.7 % of the corpus is in it, more than
the rule required — but because what it wins is a rounding error. The transform is arithmetically what §3
guessed: files whose streams are already at level 6–9 have nothing left, and the measured median says the
typical accepted document loses **eighty bytes per hundred kilobytes**. A handler would cost 40–70 KiB of binary
and 1 500–2 500 lines to save that, and would still have to refuse a third of the corpus as
`already_optimal` and a fifth as out of scope.

The one number that made this worth checking is the last one: **20.1 %** is what repacking buys, and repacking is
forbidden because flattening revisions, regenerating object streams and renumbering objects is not a recompress —
it is a rewrite with a different risk class. The audit's job was to find out whether that 20 % could be reached
without paying that price. It cannot.

The refusal is recorded in [formats §5](formats.md#5-refused-formats) and
[guarantees §4.11](guarantees.md#411-pdf--flatedecode-recompression-80-non-default-opt-in), which keeps the clause-by-clause
contract: it is now a written answer to "what would it take", and the reason to keep it is the same
reason FLAC's audit was kept.

## 10. What happened to this work

The two branches in the original stage-1 plan are historical. The owner later authorized the narrow #80 handler
behind the non-default `pdf` feature, independently of #57. The fixed 21-output #57 sample has now been rerun at the corrected exact source SHA, and a further exact-head
rerun is recorded in §13; the original accepted-subset `faithful_pct` rule still returns refusal because the
median is below 1%. The corrected immutable report and permitted-qpdf/forbidden-repack distributions are in §§12–13;
§11 preserves the earlier `7b82c9ed` result. This does not remove the separate opt-in #80 decision, enable
PDF by default, or permit repacking. The older 2,208-file audit and thresholds remain unchanged.

## 11. PR #84 follow-up — #57 fixed 21-output sample

The historical 2,208-file audit and its §9 values are preserved above. The fixed real-toolchain sample was
measured at exact source SHA `7b82c9edb6e5060bc8ce7ed756caebba91505b5b`, run
[41038741167](https://github.com/RUdolf1517/tzim/actions/runs/41038741167); the frozen manifest SHA-256 is
`8b4705e6b7ce450b97e01c247c29d7f032f245270a0e6ad9ccffea9f731dd903`. All 21 pre-registered outputs were
retained: 18 accepted (85.71% of the frozen corpus), and 3 refused as `incremental_update`. Raw rows,
`qpdf-and-tzim-tool-output.jsonl`, manifest and independent recomputation are in artifact `fixed-pdf-evidence`
(ID `12970270169`, SHA-256 `209bd12a83b8c616928a3da837eceb00a06a3979b56246877c26e42d787e2c2b`). The report is
[PR comment `6520274047`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6520274047).

### Pre-registered distributions and decision

These are computed from the **same 18 accepted rows**. `faithful_pct` is the registered flate2-level-9 metric;
the Tzim-only optimized-row median is reported separately below and is not used for the threshold.

| Distribution | n | Median | Q1 (registered index) | Q3 (registered index) | Share > 0 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Accepted-subset `faithful_pct` (flate2 level 9) | 18 | 0.00038990779980227345% | 0.0% | 0.19344060494152818% | 72.22% |
| Permitted qpdf (`--object-streams=preserve`) | 18 | 0.00042536785618854157% | −0.9378938421407427% | 0.19284450979138698% | 66.67% |
| **Forbidden** qpdf repack comparator (object streams generated) | 18 | 0.35566962243960054% | 0.03437523629623234% | 43.320392689225876% | 100.00% |

The pre-registered rule admits only when median `faithful_pct` is at least 3% and at least 10% of the frozen
corpus is accepted; a median below 1% refuses. Since the accepted-subset median is 0.00038990779980227345%,
the threshold was **applied and returned refused after measurement**. This does not change the separately
authorized #80 code, which remains behind the non-default `pdf` feature; no PDF baseline or threshold was
changed, and the repack remains a diagnostic comparator, not an allowed transformation.

### Tzim and soundness results (separate from the rule)

Tzim optimized 12 of 21 rows and saved 3,565 bytes from 72,982,531 optimized-input bytes (0.004884730566551604%
weighted). Its per-file optimized-row median gain was 0.07017961336301083%; neither is substituted for
`faithful_pct`. All 21 source rows and all three incremental-update refusals remain in the report. There were
zero qpdf candidate-check failures and zero registered MuPDF render differences. This section preserves the complete
`7b82c9ed` run as its own immutable historical measurement; the corrected subsequent exact-SHA result is recorded
separately in §12.

## 12. PR #84 corrected exact-head #57 sample

The fixed 21-output sample was rerun at exact source SHA `5721867ab1439814cead5645d4f443d8f8876c9c` in workflow
[41047304719](https://github.com/RUdolf1517/tzim/actions/runs/41047304719); the frozen manifest SHA-256 is
`d179ff1bb652fcf5c78b88eb59cf10686cd088369d55bfcead1a2c4e3b96d961`. All 21 pre-registered outputs were
retained: 18 accepted and 3 refused as `incremental_update`. The raw rows, commands, source/output identities,
qpdf checks, renders and independent recomputation are in artifact `fixed-pdf-evidence`; the report is
[PR comment `6521481242`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6521481242).

The **same 18 accepted rows** feed the registered metric and both comparators. No Tzim-only gain substitutes for
`faithful_pct`.

| Distribution | n | Median | Q1 | Q3 | Share > 0 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Accepted-subset `faithful_pct` (flate2 level 9) | 18 | 0.0003428000748065941% | 0.0% | 0.20546883358771292% | 72.22% |
| Permitted qpdf (`--object-streams=preserve`) | 18 | 0.0003289122823834112% | −0.9587218833140738% | 0.2043518962486015% | 66.67% |
| **Forbidden** qpdf repack comparator (object streams generated) | 18 | 0.3487681804689819% | 0.03305937990382424% | 44.06366320112951% | 100.00% |

The original registered rule is unchanged: admission requires median `faithful_pct` ≥3% and at least 10% of the
frozen corpus accepted; median below 1% refuses. The measured median is 0.0003428000748065941%, so the threshold
was **applied and returned refused after measurement**. Tzim optimized 12/21 rows and saved 3,670 bytes from
76,721,560 optimized-input bytes (0.004783531513175697% weighted); the optimized-row per-file median was
0.07246214628159946%, and was not substituted for the registered metric. Qpdf candidate-check failures and
registered render differences were both zero. This rerun does not alter #80's independent authorization, the
non-default `pdf` feature, any baseline, or the forbidden status of repacking. Section §11 preserves the earlier
exact `7b82c9ed` result; the a1 report remains in its immutable PR comment.

## 13. PR #84 exact-head #57 rerun at `5967e31`

The fixed 21-output sample reran at exact source SHA `5967e318951f9c3945f0c256c3095b347483e234` in workflow
[41049496855](https://github.com/RUdolf1517/tzim/actions/runs/41049496855). Its frozen manifest SHA-256 is
`a5ecf26b32bda38353f743e1cd450b7a7b44d3ccff2a35827858e4406092858c`. All 21 outputs were retained: 18 accepted
and 3 refused as `incremental_update`. Raw rows, commands, source/output identities, renders, qpdf results and
independent recomputation are in artifact `fixed-pdf-evidence` (ID `12973712842`, 198,867 bytes, SHA-256
`1dc57c9ed62ee31f327b91feaefc9c63cd28738a43701d7a3f67a5db5c9d1ef6`); no generated PDF corpus bytes are in the
artifact. The exact report is [PR comment `6521774706`](https://github.com/RUdolf1517/tzim/pull/84#issuecomment-6521774706).

The registered metric and both comparators use the **same 18 accepted rows**. `faithful_pct` remains the
pre-registered flate2-level-9 metric, not the Tzim optimized-row gain.

| Distribution | n | Median | Q1 | Q3 | Share > 0 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Accepted-subset `faithful_pct` | 18 | 0.00034127460376596526% | 0.0% | 0.20532715969914445% | 72.22% |
| Permitted qpdf (`--object-streams=preserve`) | 18 | 0.00031738124802770226% | −0.9369760331393628% | 0.19488249112494602% | 66.67% |
| **Forbidden** qpdf repack comparator | 18 | 0.29515964267860634% | 0.033062028207528904% | 44.10705775399784% | 100.00% |

The original registered rule is unchanged: admission requires median `faithful_pct` ≥3% and at least 10% of the
frozen corpus accepted; median below 1% refuses. The measured median was 0.00034127460376596526%, so the threshold
was **applied and returned refused after measurement**. Tzim optimized 12/21 rows and saved 3,723 bytes from
76,993,480 optimized-input bytes (0.004835474380428057% weighted); its optimized-row median was 0.07132130674592069%
and mean 0.09019490508048468%, neither a substitute for the registered metric. Qpdf candidate-check failures and
registered render differences were both zero. No PDF baseline, threshold, #80 authorization, default or repack
policy changed.
