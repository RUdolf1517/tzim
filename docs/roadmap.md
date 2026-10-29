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
| zlib false positives | a *weak* signature never produces `failed`; a stream that decodes nothing is `unsupported`, a stream that decodes and then breaks stays `failed` | keeps the exit code honest for a directory of unrelated files without losing the corruption signal (O1) |
| Memory | enforce a computed budget (`--jobs` × the decoded size a file can need) and `skip` above it | the current limit counts files, not bytes, so it does not bound anything (O2) |
| Binary size | drop `indicatif`/`console` and draw the progress line by hand before cutting any format or `--zopfli` | 92–96 KiB on every row, and it cancels one promise instead of several (O3) |
| Traceability | `T-FS-*` and `T-STRIP-*` rows exist only for tests that exist | a row without a test is how `testing.md` drifted in the first place (O5) |
| `verify` semantics | byte, structure and pixel comparison, stated as such; no semantic model is claimed | cheaper, honest, and it is what the code does (O7) |

---

## 2. Releases

**Read the status notes as history, not as a description of the binary you have.** Each one was written when
that release shipped, and it keeps the numbers of that release: the test counts, the sizes and the corpus
figures below are frozen at the version they belong to. Today's numbers are in CI — the `size` job prints the
real binary, `corpus` prints the real-file totals — and in [benchmarks](benchmarks.md). A note that disagrees
with the tree is a historical note, not a lie, as long as it says which release it describes.

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

**Status (v0.1, kept as history):** implemented. That milestone shipped with 41 unit and 21 integration
tests, a clean clippy run with `-D warnings`, and an 832 KiB static musl binary. The numbers describe v0.1,
not the current tree: today there are 101 unit, 89 integration and 10 packaging tests
([testing](testing.md)), and the binary is 1024 KiB ([size budget](size-budget.md#1-budget)). The
remaining-work list of that milestone — committing `Cargo.lock`, publishing artifacts, filling the benchmark
rows — was closed before v0.2.

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

### v0.4 — JPEG (opt-in at the time) — **implemented**

**What shipped**

- Baseline sequential (`SOF0`, 8-bit, one interleaved scan), including files with restart intervals.
- Entropy-only re-encoding: every DCT coefficient is decoded and re-encoded with optimal Huffman tables;
  every marker is copied as a raw segment, including Exif, ICC, `COM`, `DRI` and the bytes after `EOI`.
- Safe Rust, no new dependency and no `unsafe` — the entropy codec is part of this repository.
- Enabled only by `--formats jpeg`; the same flag narrows the other handlers and nested children.

**Scope changes against the original plan**

- **Progressive files are refused, not optimized.** Successive approximation and spectral selection are
  the most error-prone part of the format, so v0.4 covers baseline only and reports `skipped` with a
  precise reason. Progressive support is deferred and is not advertised.
- The same applies to arithmetic coding, lossless (SOF3), hierarchical, 12-bit and SOF1 files.
- **Verification compares DCT coefficients, not pixels.** Coefficient identity implies pixel identity, so
  the guarantee is strictly stronger, and it does not need a second decoder. The residual risk — that a
  shared encoder/decoder bug passes both — is stated in the contract.

**Gate status**

- `T-JPG-01` … `T-JPG-09` are covered by nine integration tests and eight unit tests (including a
  4 000-case property test of the table generator).
- The corpus gate is met, one release later: 572 real images from four pinned upstream archives run
  through the binary in CI, with any new failure, any verification failure and any second-pass rewrite
  treated as a build break. See [testing](testing.md#real-file-corpus) and
  [benchmarks](benchmarks.md#31-the-real-file-corpus).
- Fuzzing is in CI too: `fuzz/` holds two `cargo-fuzz` targets, one of them the entropy reader, and a
  sixty-second-per-target smoke run gates pull requests that touch the parsers.

**Risk:** **high** — this is the most bug-prone codec in the project.

### v0.5 — the interface, corrected — **implemented**

**What shipped**

- The path is the command: `tzim photo.png`, `tzim ./photos -r`, `tzim . -r --dry-run`. The `optimize` verb
  is accepted and silently stripped for 0.1–0.4 compatibility, so no script breaks, but nothing documents
  it any more.
- `-i` / `--info`, `--list-formats` and `--check-deps`: information without a path, exit `0`, no side
  effects. `--check-deps` answers "none" — the binary shells out to nothing.
- The README and `docs/cli.md` were rebuilt around that shape, with install channels, dependency
  expectations and the guarantees in one place.

**Why it is a release and not a patch**: removing a subcommand is observable, so the minor version moves.
No handler changed, no byte of output changed, and every guarantee in the tables is the same object it
was in 0.4.3.

### v0.6 — narrowing the build, and metadata on request — **implemented**

**What shipped**

- One feature per handler (`jpeg` and `ico` were the missing two), and a CI matrix that measures what each
  one costs instead of asserting it. A `png,gzip` build is the smallest useful optimizer; the published
  binary is unchanged in content.
- `--strip-metadata`, with a per-format list, its own guarantee class (G4) and a fuzz target. The flag
  exists because users otherwise reach for a tool that strips silently; here it is explicit, printed on
  every line it rewrites, and it never touches colour-defining chunks.
- The verification model was documented honestly: JPEG and ZIP verify with code from this repository, and
  the document now says so, along with what that cannot catch.

**Why the version moves**: a new flag and a changed feature set are observable, even though no default
build behaves differently.

### v0.7 — TIFF, classic and strips — **implemented**

**What shipped**

- A classic-TIFF handler that rebuilds the file deterministically — header, IFD, out-of-line values, strips —
  and then proves the rebuild: every entry is compared as a *resolved value* (so a moved value still
  compares equal), and every strip is inflated and compared byte for byte.
- Uncompressed strips become Deflate; Deflate strips are re-compressed and keep whichever result is smaller.
  `Compression` is the only tag whose value may change, and verification allows exactly the two transitions
  that are still lossless.
- The accepted subset is narrow on purpose: classic `42` only, one IFD, strips not tiles, interleaved
  samples, no predictor, `Compression` in {none, Deflate}. LZW is refused rather than re-emitted by an
  encoder this repository would have to write and could not verify. BigTIFF, tiles, separate planes,
  predictors, EXIF/GPS sub-IFDs and unknown *out-of-line* tags are refused with a reason; unknown tags that
  fit in the entry itself are copied untouched.
- `--strip-metadata` removes the textual tags only ([cli §2](cli.md#2-options)), and the acceptance
  rule stays "never larger".

**Why the version moves**: a new format is observable. The binary grows by roughly 10 KiB, measured in
[size-budget](size-budget.md) like every other handler.

### v0.8 — the terminal, and a contract for FLAC — **implemented**

Nothing here changes a byte of output:

- colour and a progress line, both off unless a terminal is attached, with `--color`, `--no-color`,
  `--no-progress`, `NO_COLOR`, `CLICOLOR` and `CLICOLOR_FORCE` honoured ([cli §10](cli.md#10-colour-and-progress));
- clustered short flags (`-rq`) and attached values (`-j4`), a usage line on usage errors, and a closed
  stdout pipe that exits quietly instead of aborting ([cli §11](cli.md#11-conformance-with-cli-conventions));
- the FLAC **contract**: [guarantees §4.9](guarantees.md#49-flac--frame-re-encoding-79-non-default-opt-in); the opt-in handler is an unreleased #84 follow-up
  and the [audit](flac-audit.md) behind it.

### v0.9 — zlib, and the end of the format queue — **implemented**

- **zlib (RFC 1950)** joins the handlers: recompress the DEFLATE stream, keep the two header bytes and the
  Adler-32 verbatim ([guarantees §4.10](guarantees.md#410-zlib-rfc-1950--v09)). No new dependency — `flate2`
  was already there, and the Adler-32 is twelve lines of arithmetic.
- **Raw DEFLATE is refused**, not deferred: a bare DEFLATE stream has no signature, so recognising one would
  mean guessing ([formats §5](formats.md#5-refused-formats)).
- **FLAC is refused after measurement** ([flac-audit §7](flac-audit.md#7-decision-refused-and-the-measurement-behind-it)):
  0.4 % on musical material, 0 % on files already written at `flac -8`, for ~150–300 KiB of binary.

### v0.10 — the claims around the code — **implemented**

No handler changed and no byte written by the program is different. This release is about the sentences
around the code that had stopped being true:

- **CI is what [testing §4](testing.md#4-continuous-integration) says it is.** `msrv` (1.88.0), `fmt`, `deny`
  (with a real `deny.toml`) and `audit` exist; every build runs `--locked`, which
  [dependencies §6](dependencies.md#6-supply-chain) had been promising since the first release; the
  `lockfile` job actually pushes, and [testing §4.2](testing.md#42-the-two-jobs-that-write-to-the-branch)
  explains why two jobs write to the branch.
- **The benchmarks are re-measured**, including the TIFF and zlib rows that the v0.4-era table predated, and
  recorded with the run id ([benchmarks §3.2](benchmarks.md#32-against-the-established-tools)).
- **The progress line is `indicatif`**, at the user's request: a spinner for one file, a clock, a bar and a
  counter for a batch ([cli §10](cli.md#10-colour-and-progress)). It is the first dependency granted since
  v0.1 and the only one not behind a feature ([dependencies §2.9](dependencies.md#29-indicatif--added-in-v010-removed-in-v013)).
- **The README fits on one screen**: badges, five lines, the format table, and the pitch — contract and
  refusal — where a stranger reads it first.

### v0.11 — distribution — **implemented**

The format queue is empty, so this release is about the path from `curl` to a working tool rather than about
what the tool can read:

- **`install.sh` verifies its download** against the release's `SHA256SUMS`, runs the installed binary before
  reporting success, and aborts on a mismatch instead of falling back to a source build that would hide it.
  `TZIM_BASE_URL` points it at a mirror, with the same check.
- **A Linux ARM64 binary** and ARM64 `.deb`/`.rpm` packages: ARM servers and single-board computers stop
  needing a toolchain.
- **A manual page and three completions**, installed by the packages and the formula, with
  `tests/packaging.rs` failing when they drift from `--help`.
- **Two CI jobs that test packaging**, not just code: `cargo package` builds the crate as crates.io would
  receive it, and the `installer` job installs the published release and then proves a doctored archive is
  refused.

### v0.12 — the audit, answered — **implemented**

An independent audit of 0.10.0 and 0.11.0 produced twenty-three issues. The release answers them, and the one
behaviour it changes is a defect:

- **The equal-size candidate stops being an internal error** (#26, #44): one predicate, `formats::size_ok`,
  used by the pipeline, the verification dispatcher and both fuzz targets.
- **The verifiers are tested** (#43): `tests/verification.rs` gives every one of them a candidate that must be
  refused.
- **Documents and code agree in the same release**: [guarantees §3.1](guarantees.md#31-what-verified-actually-compares--and-what-it-does-not)
  says what "verified" compares, [safety §1](safety.md#1-algorithm) is the algorithm that runs, and
  `T-FS-*` / `T-STRIP-*` rows are in [testing §2](testing.md#2-traceability) with the test behind each one.
- **Seven traceability rows that had no test have one**; three say so in the row and give the reason (#49).

The tag was cut from `main` after those tests failed before the fix and passed after it.

### v0.12.1 — the first patch after the audit — **implemented**

- **A coincidental zlib header stops failing the run** (#52): nothing decoded is `unsupported` with exit `0`;
  a stream that decoded bytes and then broke is still `failed` with exit `1`.
- **The winget channel is gone** (#56). It never distributed anything — the package was never submitted — and
  Windows installs from the release zip now, which is the path that always worked.
- **The last three traceability rows are closed** (#49): `T-PNG-04` needed Adam7 in the fixture builder,
  `T-APNG-05` needed a frame-sequence check that did not exist, and `T-NEST-04` was deleted because the budget
  it described does not exist.
- **`T-FS-11` … `T-FS-16`** (#35): a FIFO, a device node, a full disk, a symlink cycle, a read-only directory
  and a 70-level tree. The cycle test disproved a sentence in `cli.md` on its first run: what stops a cycle is
  the kernel's `ELOOP`, not the 64-level guard.

### v0.13 — bytes, a line, and a second opinion — **implemented**

The six issues 0.12 opened against itself, and the first release since v0.1 that removes a dependency:

- **A memory budget** (#51, owner decision O2-D): the limit counts bytes rather than files, and a file above it
  is `skipped` with a reason instead of being read — checked before `fs::read`, with the verification buffers
  in the arithmetic ([safety §7.1](safety.md#71-memory--an-enforced-budget)).
- **`indicatif` is gone** (#53): the progress line is 250 lines of `src/progress.rs` again, 96 KiB lighter and
  *better* — a 64-position bar, a rate over the last eight completions, an ETA that waits for evidence, and
  clipping by display width ([cli §10](cli.md#10-colour-and-progress),
  [dependencies §2.9](dependencies.md#29-indicatif--added-in-v010-removed-in-v013)).
- **A JPEG decoder nobody here wrote** (#39): `jpeg-decoder` verifies both sides of an accepted candidate,
  which closes the symmetric-codec hole [guarantees §2.6](guarantees.md#26-verification-independence) had been
  describing since v0.4. At the v0.13 decision, the condition was that the binary not grow past 1 049 440
  bytes; the ordinary-profile measurement was **+118 784 bytes**, more than the 96 KiB the progress line gave
  back. The v0.13 release therefore kept the condition: `jpeg-v1` existed and CI tested it, but it was not in
  that release's default, and JPEG remained opt-in behind `--formats jpeg`.
  **This paragraph records the v0.13 decision, not the current policy.** The dependency-specific `opt-level =
  "s"` build measured 1 082 208 bytes, 32 768 over the then-current 1 049 440-byte limit; the owner later
  explicitly superseded that old size condition in #82. The v0.13 rationale and measurements remain history,
  while the unreleased #82 status below records the authorized default and its unchanged new ceiling.
- **The check that lives in CI** (#54): Pillow, stdlib `zlib` and `zipfile` compare every accepted candidate
  with its original, at the strongest level each offers, and a self-test mutates one candidate per run to
  prove the gate can fail ([testing §3](testing.md#the-third-party-decoder-job)). It found a defect on its
  first run: five Pillow TIFFs with `FillOrder 2` were being compressed into files libtiff cannot decode, and
  the writer now refuses them ([guarantees §4.8](guarantees.md#48-tiff-classic-baseline-strips--v07)).
- **The three coverage gaps** (#36): `tiff` and `zlib` in the fuzz build, seeds for `strip`, zlib in the
  corpus, and a baseline for the refused set — the assertion `testing.md` had been claiming since 0.10.
- **The 0.12.1 errata** (#62): the size job's own line confirms the binary did not move and the tarball deltas
  are packing; the zlib note stops claiming more than the decoder knows
  ([size-budget §1](size-budget.md#errata-the-v0121-same-size-claim)).

The tag is cut from `main` only after each test fails before its fix and passes after it.

### v0.14 — opt-in FITS and reproducible validation — **implemented**

- **FITS behind two switches** (#70): the `fits` feature is outside the default release build and the
  transformation requires `--fits-tile`; every non-target HDU is copied and non-smaller candidates are refused.
- **External and reproducible checks** (#71/#72): candidate-specific astropy and HEASARC gates, controlled
  refusals, full-SHA corpus identities and retained measurement reports. Their exact run and evidence limits
  are in [measurement status](measurement-status.md).
- **Scope stated without a stability shortcut**: historical FITS figures are not release/performance evidence;
  the current differences, unavailable Zopfli cells and supplemental samples remain disclosed. This release
  does not claim full FITS or PDF admission.

### v0.14+ — the queue after this

The v0.14 implementation queue is complete. The following #84 work is an **unreleased follow-up**, not part of
the v0.14.0 release notes or artifacts. Its acceptance status is deliberately kept separate from the historical
v0.13/v0.14 decisions above:

| Issue | Unreleased status | Remaining evidence or blocker |
| --- | --- | --- |
| #57 — PDF measurement | Fixed 21-output source set and measurement workflow are present; run pending | Generate the six fixed documents through all three named producers plus the three pinned scan-derived PDFs, freeze all rows before gain, then retain qpdf, Tzim, MuPDF renders and independent aggregates. No completed workflow artifact exists for this source SHA. |
| #59 — FITS measurement | Fixed 14-product TESS/GALEX/ROSAT source set and measurement workflow are present; run pending | Fetch exact full products, retain all source/error rows and run existing two-tier audit, Tzim, CFITSIO `fitsverify`/`funpack`, array/header oracles and recomputation. No completed workflow artifact exists for this source SHA. |
| #82 — default JPEG V1 | implemented; evidence is recorded per source SHA | The fixed stripped-musl ceiling is **1,082,208 bytes**. Use only the exact-head size report and `size-<source SHA>` / `jpeg-runtime-<source SHA>` artifacts; never carry evidence to a later head. |
| #83 — FITS confirmation | implemented; tests are part of the cross-platform CI matrix | The prompt is limited to one direct TTY `--fits-tile` run; the candidate is fully verified before default-No confirmation and only Yes reaches atomic install. |
| #79 — FLAC re-encoding | implemented in this unreleased branch behind the non-default `flac` feature | Rust compilation/tests, external tools, negative controls/fuzz, fixed corpus, and actual feature size/speed/gain measurements remain pending. |
| #80 — narrow PDF FlateDecode | implemented in this unreleased branch behind the non-default `pdf` feature; independent of #57 | Rust compilation/tests and the fixed #57 qpdf/MuPDF measurement/render checks remain pending. The historical repack remains forbidden. |

Full-format admission and other examined candidates remain separate follow-ups rather than reasons to smuggle
another format into the v0.14.0 release:

| Candidate | State | Why |
| --- | --- | --- |
| FLAC | non-default opt-in implementation present for #79; full admission pending | historical audit: 0.4 % on music and estimated +150…300 KiB; no current feature-size/speed/gain result or full-corpus evidence yet |
| WOFF2 | refused | Brotli re-encoding cannot be verified without the encoder itself |
| FITS full-admission follow-up | open, outside the v0.14.0 opt-in release | The v0.14.0 handler is behind `--fits-tile` and the non-default `fits` feature. Historical audit figures are not release/performance evidence; current differences and the supplemental survey sample remain open for full admission. Candidate-specific HEASARC evidence and limitations are tracked in [measurement status](measurement-status.md) |
| DICOM | audited, under measurement | the old row asked whether the dictionary or the refusal list costs more; the answer is the dictionary — 5 189 measured entries — and it is needed by one tier of three. The cheap tier recompresses a data set that is already deflated (raw DEFLATE, no checksum), specified in [guarantees §4.13](guarantees.md#413-dicom--contract-written-not-implemented); [dicom-audit §8](dicom-audit.md#8-the-measurement-and-the-rule-fixed-in-advance) fixes the thresholds in advance |
| PDF | non-default opt-in #80 implementation present; #57 fixed-manifest execution pending | the previous 2 208-file audit reported 45.7 % in-scope and a 0.08 % median; the exact 21-output supplement has not run for this source SHA. Implementation is independent of #57; repacking remains forbidden ([guarantees §4.11](guarantees.md#411-pdf--flatedecode-recompression-80-non-default-opt-in), [pdf-audit §9](pdf-audit.md#9-the-measurement-method-corpus-and-the-rule-that-is-fixed-in-advance)) |
| WebP, AVIF, HEIC, JXL, MP4, MP3, xz/zstd/7z, GIF | refused | [formats §5](formats.md#5-refused-formats) |
| raw DEFLATE | refused | no signature: recognition would be a guess |

A new format is admitted only by [formats §4](formats.md#4-admission-criteria-for-a-new-format), and the
first question it now asks is not "can it be verified" but "is there anything left to win".

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
