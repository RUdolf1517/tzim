# Roadmap

Related: [formats](formats.md) · [guarantees](guarantees.md) · [testing](testing.md)

The roadmap is ordered by *risk*, not by popularity. A format ships only after the pipeline that protects
the original file is proven, and each release adds exactly one format family.

Estimates are person-days for one experienced Rust developer, including tests and documentation. They are
planning aids, not commitments.

---

## 1. Decisions already made

| Decision | Choice | Why |
| --- | --- | --- |
| Scope | Maximum coverage of *popular* formats, delivered in families | breadth is a product requirement, but each format multiplies the verification surface |
| Zopfli | opt-in (`--zopfli`) | on already-optimized files the gain is small and the CPU cost is 10–100× |
| ZIP implementation | own surgical rewriter | a library writer cannot guarantee byte-level preservation of extra fields, order and prefix |
| Documentation language | English only | the project is intended for external open-source use |
| Losslessness | one invariant, no opt-out | the entire value proposition |
| Verification | always independent of the writer | a bug in the writer must not be able to confirm itself |

---

## 2. Releases

### v0.1 — foundation, PNG, gzip

**Contents**

- CLI as specified in [cli.md](cli.md), including `--dry-run`, `--json`, exit codes and deterministic
  ordering.
- Input collection: multiple files and directories, recursion, dedup, symlink and hard-link policy,
  include/exclude filters, size limits.
- The safe replacement pipeline from [safety.md](safety.md), including metadata preservation, `fsync`, atomic
  rename, temporary-file guards and concurrent-modification detection.
- Bounded worker pool using `std::thread::scope` and a channel.
- PNG handler: IDAT recompression, filter re-selection for non-interlaced images, optional Zopfli.
- gzip handler: header-preserving DEFLATE recompression.
- Independent verification for both formats.
- Test suite, fixture corpus, failure-injection hooks, CI on three platforms, size measurement.

**Status:** implemented. 41 unit and 21 integration tests pass on Linux, macOS and Windows; clippy is clean
with `-D warnings`; the static musl binary measures 832 KiB. Remaining before a tagged release: commit
`Cargo.lock`, publish artifacts, and fill the comparative benchmark rows in
[benchmarks.md](benchmarks.md#4-what-is-still-missing).

**Gate**

- `T-PNG-*` and `T-GZ-*` pass in CI on Linux, macOS and Windows;
- every failure mode in [safety §5](safety.md#5-failure-modes) has a test;
- the binary is measured, and the provisional budget is replaced with measured numbers;
- `docs/benchmarks.md` exists with a first honest comparison against oxipng, optipng, zopflipng and gzip.

**Estimate:** 6–9 days. **Risk:** medium — the cross-platform replacement semantics are the hard part.

### v0.2 — ZIP family

**Contents**

- ZIP reader/rewriter with the contract in [guarantees §4.3](guarantees.md#43-zip--v02).
- Safety detectors: signed archives, ZIP64, multi-disk, encryption, unknown methods, prepended stubs,
  decompression-bomb limits.
- `--max-uncompressed`, `--max-ratio`.
- Corpus covering ODF/EPUB, data descriptors, duplicate and non-UTF-8 names, and every refusal case.

**Status:** implemented. The rewriter, the refusal detectors and the verification are in place, with 13
integration tests covering deflate, stored entries, data descriptors, non-UTF-8 names, signed JAR/ODF/APK,
ZIP64, encryption, truncation, corrupt CRCs, bombs and already-optimal archives. Binary size grew by 36 KiB
with no new dependency lines.

**Gate**

- `T-ZIP-01` … `T-ZIP-12` pass;
- signed and ODF archives are provably untouched (hash comparison);
- fuzz targets run clean for the parsers;
- size delta recorded.

**Estimate:** 6–8 days. **Risk:** **high** — metadata fidelity and signatures.

### v0.3 — ICO, APNG, nested containers

**Contents**

- ICO/CUR handler reusing the PNG engine.
- APNG frame recompression.
- `--nested` (off by default), `--max-depth`, `--formats`.
- Composition rules: a failing child is stored unmodified; the parent still has to verify.

**Status:** implemented. APNG frames are recompressed without changing chunk counts or sequence numbers;
ICO/CUR reuses the PNG engine and copies bitmap entries verbatim; `--nested` optimizes supported files inside
ZIP entries with a depth limit, and a child failure can never fail the parent. 9 new integration tests.

**Gate**

- `T-ICO-*`, `T-APNG-*`, `T-NEST-*` pass;
- bomb and depth limits tested;
- signed archives rejected before any nested work.

**Estimate:** 4–6 days. **Risk:** medium.

### v0.4 — JPEG (opt-in) — **implemented**

**What shipped**

- Baseline sequential (`SOF0`, 8-bit, one interleaved scan), including files with restart intervals.
- Entropy-only re-encoding: every DCT coefficient is decoded and re-encoded with optimal Huffman tables;
  every marker is copied as a raw segment, including Exif, ICC, `COM`, `DRI` and the bytes after `EOI`.
- Safe Rust, no new dependency and no `unsafe` — the entropy codec is part of this repository.
- Enabled only by `--formats jpeg`; the same flag narrows the other handlers and nested children.

**Scope changes against the original plan**

- **Progressive files are refused, not optimized.** Successive approximation and spectral selection are
  the most error-prone part of the format, so v0.4 covers baseline only and reports `skipped` with a
  precise reason. Progressive support is deferred to v0.4.1 and is not advertised.
- The same applies to arithmetic coding, lossless (SOF3), hierarchical, 12-bit and SOF1 files.
- **Verification compares DCT coefficients, not pixels.** Coefficient identity implies pixel identity, so
  the guarantee is strictly stronger, and it does not need a second decoder. The residual risk — that a
  shared encoder/decoder bug passes both — is stated in the contract.

**Gate status**

- `T-JPG-01` … `T-JPG-09` are covered by nine integration tests and eight unit tests (including a
  4 000-case property test of the table generator).
- The 1 000-file corpus from the original gate is **not** available in this environment; the tests use
  generated fixtures covering baseline, grayscale, RGB 4:4:4, restart intervals, trailing bytes and every
  refusal path. Running a real corpus remains outstanding and is listed as such in
  [benchmarks](benchmarks.md).
- Fuzzing of the entropy reader is not part of CI yet.

**Risk:** **high** — this is the most bug-prone codec in the project.

### v0.5+ — research queue

Each of these requires a contract written *before* implementation:

| Candidate | Open question |
| --- | --- |
| TIFF | Can every tag and strip layout be preserved with a pure-Rust reader/writer? |
| FLAC | Can metadata blocks, seek tables and the stream MD5 be preserved across re-encoding? |
| WOFF2 | Is Brotli re-encoding verifiable without the encoder itself? |
| zlib/raw DEFLATE files | Small addition, unclear real-world value |
| PDF | Only if a verifiable object-level comparison becomes possible; currently **no** |

---

## 3. Risks

| Risk | Impact | Likelihood | Mitigation |
| --- | --- | --- | --- |
| A bug in a custom container writer damages a file | critical | medium | independent verification, strict refusal lists, fuzzing, byte-level fixture comparisons, replacement only after verification |
| Verification passes but a consumer still rejects the file | high | low | contract-based comparison of preserved structures; real-world corpus; no structural normalisation |
| Signed archives are rewritten and become invalid | high | medium | explicit detectors + tests that assert untouched files |
| Windows rename/metadata behaviour differs | medium | medium | CI on Windows, documented limitations, no claims of crash atomicity |
| Zopfli memory use on large files | medium | medium | opt-in, lower default `--jobs`, size limits |
| Binary size creeps with each format | medium | high | per-family features, CI size gate, measured deltas |
| Gains on modern files are near zero | medium (product) | high | honest documentation, breadth and trust as the value proposition, no ratio claims |
| Scope growth delays the first release | medium | high | v0.1 is fixed at PNG + gzip; later formats are additive |
| Dependency churn breaks builds | low | medium | committed lockfile, MSRV job, `cargo-deny`, monthly review |

---

## 4. Permanent non-goals

Not scheduled, not planned, not "coming later":

- metadata, comment or chunk stripping of any kind;
- lossy or "visually lossless" optimization;
- format conversion (including PNG → WebP, JPEG → JXL, TIFF → PNG);
- minification of text formats;
- a GUI, a daemon, a local server, or a plugin system;
- repair of corrupted files;
- any flag that skips verification or replaces a file despite a failed check.

---

## 5. How this roadmap changes

A release can slip; a guarantee cannot. If a format cannot meet its contract in time, it is removed from the
release rather than shipped with a weaker promise. Changes to this document are recorded in
`CHANGELOG.md`.
