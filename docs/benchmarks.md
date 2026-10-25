# Benchmarks

Related: [size budget](size-budget.md) · [testing](testing.md) · [formats](formats.md)

Every number here was produced by CI and can be reproduced from the linked commit. Nothing in this file is
an estimate.

---

## 1. Binary size — measured

| Field | Value |
| --- | --- |
| Commit | v0.13.0 — `ci` job `binary size`, the run that measured the feature matrix in [size-budget §3](size-budget.md#3-feature-matrix) |
| Target | `x86_64-unknown-linux-musl` (static-pie, no libc dependency, no interpreter) |
| Profile | `--release`, `opt-level = "s"` for this crate, `3` for the codecs, default features (PNG, APNG, gzip, ZIP, ICO/CUR, JPEG, TIFF, zlib, Zopfli) |
| Features in the binary | every runtime feature; `fuzz-api` is excluded by construction and `jpeg-v1` is off by default |
| **Size** | **979 808 bytes (956 KiB)** |
| Change from v0.12.1 | **−69 632 bytes (−68 KiB)** |
| Normal dependency lines (`cargo tree`, de-duplicated, Linux target) | 14 (was 19) |
| Gate | 979 808 bytes, exactly — [size-budget §1](size-budget.md#1-budget) |

**The 68 KiB came back from the progress line.** v0.12.1 carried `indicatif` and `console` — 96 KiB, a tenth
of the binary — and v0.13 draws the same line in `src/progress.rs` for nothing
([dependencies §2.9](dependencies.md#29-indicatif--added-in-v010-removed-in-v013)). Five dependency lines left
the tree with them.

The other half of the release is a decoder that is **not** in this number: `jpeg-v1`, the independent JPEG
verifier, measures **+118 784 bytes** (116 KiB) and therefore ships off ([dependencies
§2.10](dependencies.md#210-jpeg-decoder--the-second-opinion-on-a-jpeg-candidate)). It was approved on the
condition that the shipping binary does not grow past 1 049 440 bytes, and it does not fit even with the 96 KiB
that was freed; the condition was kept and the feature is a flag.

The measured build is the released one: default features. `--all-features` now includes a `fuzz-api`
feature that exists only to expose the parsers to the fuzz targets, and compiling it into the measured
binary would measure code that never ships.

Growth by version. **No new dependency lines at any step until v0.10.0**, which adds the progress line's
five on Linux — the first dependency granted since v0.1. The JPEG handler in v0.4 adds code rather than a
crate, because no Rust JPEG library exposes DCT coefficients (see
[dependencies](dependencies.md#26-jpeg-why-there-is-no-jpeg-dependency)):

| Version | Contents added | Binary |
| --- | --- | --- |
| v0.1 | PNG, gzip | 832 KiB |
| v0.2 | ZIP family | 868 KiB (+36 KiB) |
| v0.3 | APNG frames, ICO/CUR, nested archives | 904 KiB (+36 KiB) |
| v0.4 | baseline JPEG entropy codec (opt-in) | 976 KiB (+72 KiB) |
| v0.4.1 | library split so the parsers can be fuzzed; no new code paths | 980 KiB (+4 KiB) |
| v0.5.0 | path-first command line, `-i`, `--list-formats`, `--check-deps` | 980 KiB (+0) |
| v0.6.0 | `--strip-metadata`, one feature per handler | 988 KiB (+8 KiB) |
| v0.7.0 | classic TIFF: strips, tags, rebuild-then-verify | 1 024 KiB (+36 KiB) |
| v0.7.1 | release profile: `-Os` for this crate, `-O3` for the codecs; no behaviour change | 900 KiB (−124 KiB) |
| v0.8.0 | colour, progress line, buffered report, `IsTerminal` — interface, not handlers | 920 KiB (+20 KiB) |
| v0.9.0 | zlib handler: DEFLATE recompression, header and Adler-32 preserved; no new crate | 928 KiB (+8 KiB) |
| v0.10.0 | progress line drawn by `indicatif`, not by hand; CI and benchmark honesty; no handler changed | 1 024 KiB (+96 KiB) |
| v0.11.0 | distribution: ARM64 artifact, manual page, completions, a verifying installer; nothing added to the binary | 1 024 KiB (+0) |
| v0.12.0 | the audit: one size predicate, honest interface text, verifier tests, a display-width table; nothing added to the binary | 1 024 KiB (+0) |
| v0.12.1 | two defects fixed, one channel removed, three traceability rows closed; the binary is byte-for-byte the same | 1 024 KiB (+0) |
| v0.13.0 | `indicatif` and `console` removed, the progress line drawn by hand again (86 lines of it are new arithmetic, 96 KiB of crates are gone); `jpeg-v1` available and off | 956 KiB (−68 KiB) |

The v0.6.0 row is a linker artefact rather than new behaviour: the same code compiled as a library plus a
binary crosses one more 4 KiB page boundary. It is recorded because the budget is measured, not inferred.
The v0.7.0 row is the TIFF handler: a parser, a rebuild and a verifier, and still no new dependency.
The v0.7.1 row is the only one that removed bytes instead of adding them, and it removed nothing from the
program: the same commit built at `opt-level = 3` measures 1 028 960 bytes and writes the same output. Two
changes did it — `opt-level = "s"` for this crate with `-O3` kept for the codecs (−104 KiB, +3.6 % on the
corpus, see [size-budget §5.1](size-budget.md#51-the-measurements-behind-the-current-profile)), and integer
formatting in `src/report.rs`, which removes the general `core::fmt` path (−20 KiB, no output change).

The binary is statically linked, contains no C code, and requires no runtime beyond the OS.

What this size includes: the CLI and argument parser, directory walker, the replacement pipeline with
fsync/rename handling, the PNG chunk walker and scanline filter engine, the gzip container code, the DEFLATE
codec (`miniz_oxide`), the independent PNG decoder used for verification (`png` crate), CRC-32, and the
Zopfli encoder.

Comparison points, for scale only: oxipng's official Linux release archive is ~493 KiB compressed and
Leanify advertises "under 1 MB", both for one language and a narrower contract. Tzim's number is in the same
order of magnitude while carrying an independent verification decoder.

---

## 2. Correctness evidence — measured

| Metric | Value |
| --- | --- |
| Unit tests | 101 |
| Integration tests (real binary, real files) | 89 (87 on Windows, where two tests are Unix-only) |
| Platforms | Linux, macOS, Windows — all green on the v0.10.0 pull request |
| Clippy | `-D warnings`, clean |
| Rustfmt | clean, from v0.10.0 (the tree had never been formatted as a whole) |
| Exit codes covered by tests | 0, 1, 2 (4 is covered by a unit test on an injected verification error) |

---

## 3. Compression results — measured

### 3.1 The real-file corpus

Four pinned upstream archives, assembled by `.github/scripts/corpus-fetch.sh` (URLs and checksums in
[testing.md](testing.md#real-file-corpus)), 690 images — PNG, JPEG and TIFF — plus 4 containers built from
them with `gzip -9` and `zip -9`. CI job `corpus`, one pass over the tree.

Measured on v0.12.1, run
[`40212911347`](https://github.com/Rudolf1517/tzim/actions/runs/40212911347), commit `c012ba5`:

| Metric | Value |
| --- | --- |
| Files | 694 |
| Optimized | 425 |
| Already optimal | 156 |
| Unsupported | 0 |
| Skipped (refused by contract: progressive/arithmetic/limit/unsupported layout) | 81 |
| Failed (corrupt upstream fixtures, all allowlisted — see `.github/corpus-allowlist.txt`) | 32 |
| Bytes before | 21 864 484 |
| Bytes after | 19 734 464 |
| **Saved** | **2 130 020 (9.74 %)** |
| Files rewritten on a second pass | 0 |

Optimized files by format: **jpeg 46, png 340, tiff 39**. By container: **images 690, gzip 3, zip 1**.

Skipped files by reason: more than one IFD (12); compression 4 is not rewritten by this build (12); tiled
layout is not supported (9); only baseline sequential (8); tag 34665 carries an offset to another structure
(6); IFD entries are not in ascending tag order (5). The remaining 29 are long tails of the same kinds; the
full list is written to `refusals.txt` by the run and attached to the job.

The 425/19 734 464 numbers are **lower than the v0.10.0 table this section used to show** (437 / 19 733 541),
and the difference is a defect being removed rather than compression improving: twelve of Pillow's negative
APNG fixtures used to be rewritten and reported `optimized`, and v0.12.1 refuses them (§`0.12.1` in the
[changelog](../CHANGELOG.md)). Those twelve files also account for the twelve extra `failed` rows. Nothing
else moved: the byte total changed by 923 bytes because those twelve files are no longer written, and the
per-format counts for JPEG and TIFF are unchanged.

The 156 "already optimal" files are the honest part of this number: files that upstream projects already
compressed well, and leaving them alone is the contract working, not the tool failing.

v0.13 adds four zlib streams to the corpus (`corpus-fetch.sh`) and stops leaving the zlib handler outside the
regression gate, so the next run's `Files` count is 698 and the byte totals move with it. That is a corpus
change, not a compression change, and the two are kept apart on purpose: a table that moves because the
fixtures changed says nothing about the tool.

### 3.2 Against the established tools

The same corpus through Tzim and the reference tools, one machine (4 cores), copies of the same files, CI
job `bench`, run [`40190288721`](https://github.com/RUdolf1517/tzim/actions/runs/40190288721) on commit
`f714339`, binary `tzim 0.10.0`. Reference tools: oxipng 10.2.1, OptiPNG 0.7.8, jpegtran from libjpeg-turbo
2.1.5. The timed comparison is capped at files of 200 KiB or less, because `oxipng -o max` needs minutes per
megabyte; the ratio for the whole corpus is the table in §3.1.

Wall clock on a shared runner is the least repeatable column here. The run before this one measured 12:06 for
`--zopfli` and 26.02 s for `oxipng`; this one measured 21:45 and 44.54 s. **Every byte column was identical
between the two runs**, which is what these tables exist to compare — treat the times as an order of
magnitude, not as a measurement of your machine.

**PNG** — 3 603 182 bytes in files of 200 KiB or less:

| Tool | Wall clock | Peak RSS | Bytes after | Saved |
| --- | --- | --- | --- | --- |
| tzim (`--jobs 1`) | 4.77 s | 4 MiB | 3 328 364 | 274 818 (7.63 %) |
| tzim (default jobs) | 1.81 s | 7 MiB | 3 328 364 | 274 818 (7.63 %) |
| tzim `--zopfli` (`--jobs 1`) | 21 min 45 s | 43 MiB | 3 059 467 | 543 715 (15.09 %) |
| oxipng `-o max --strip none` | 44.54 s | 76 MiB | 2 637 461 | 965 721 (26.80 %) |
| optipng `-o7 -preserve` | 2 min 48 s | 16 MiB | 2 817 142 | 786 040 (21.82 %) |

**JPEG** — 1 399 119 bytes:

| Tool | Wall clock | Peak RSS | Bytes after | Saved |
| --- | --- | --- | --- | --- |
| tzim (`--jobs 1`) | 0.14 s | 5 MiB | 1 369 032 | 30 087 (2.15 %) |
| jpegtran `-copy all -optimize` | 0.34 s | 13 MiB | 1 308 075 | 91 044 (6.51 %) |

**TIFF** — 2 444 088 bytes, no reference tool in the table: `tiffcp -c deflate` rewrites the tag layout and
the strip geometry, which Tzim does not, so the two would not be doing the same job.

| Tool | Wall clock | Peak RSS | Bytes after | Saved |
| --- | --- | --- | --- | --- |
| tzim (`--jobs 1`) | 0.09 s | 4 MiB | 1 996 362 | 447 726 (18.32 %) |

**zlib** — 56 374 bytes: twelve real source files, each compressed with `zlib.compress(data, 6)` — the level
every library and HTTP client uses by accident. No reference tool: `advdef` handles gzip and ZIP, not a bare
zlib stream.

| Tool | Wall clock | Peak RSS | Bytes after | Saved |
| --- | --- | --- | --- | --- |
| tzim (`--jobs 1`) | 0.02 s | 3 MiB | 55 877 | 497 (0.88 %) |

What the numbers say, without softening either half:

- **On PNG, `oxipng -o max` wins and this project does not pretend otherwise.** It tries many filter
  strategies and deflate settings per image; Tzim tries a smaller set. In exchange Tzim is roughly nine times
  faster single-threaded, uses a nineteenth of the memory, and handles five formats instead of one.
- **`--zopfli` closes a little over half of that gap — 15.09 % against 26.80 % — and costs 273× Tzim's own
  single-job wall clock, or 721× its default parallel run,** on the same files. That is why it is opt-in: it
  is a different tool with the same contract, not the default.
- **The JPEG row is not like-for-like.** `jpegtran` re-optimizes progressive files, which Tzim refuses by
  contract, and it re-serialises markers, which Tzim copies byte for byte. Part of the 4.4-point difference is
  that difference in scope, not a weaker encoder; this run does not separate the two. Splitting it is listed
  as an open measurement in §4 rather than explained away here.
- **The TIFF number is the one place Tzim's ratio beats every reference tool**, and the reason is boring:
  uncompressed strips. There is no tool in the comparison that recompresses them without rewriting the file,
  and 39 of the corpus TIFFs are exactly that.
- **The zlib number is small on purpose.** Recompressing text that a modern zlib already wrote at level 6
  finds 0.88 %; a stream from an old encoder, or one written at level 1, has far more room. This set is
  published at the level it is because a benchmark that only shows the flattering case is not a benchmark.
- These files are heavily skewed towards `pngsuite`, which is deliberately poorly compressed in places. That
  is why Tzim saves 7.63 % here and 9.74 % on the whole corpus: same code, different input. Both numbers are
  published because either one alone would mislead.

### 3.3 Older spot measurements

Before the corpus existed, the numbers below came from a stage-1 analysis (Python + zlib) on files shipped
by a Linux distribution. They are kept because they explain where the gains come from, not as a claim:

| Corpus | Method | Result |
| --- | --- | --- |
| 17 real PNG icons from `/usr/share` | recompress at `zlib -9`, and refilter + `zlib -9`; keep the smaller | median **−1.2 %**, mean −0.5 %; 10 of 17 shrank, 7 were already better compressed than every candidate |
| oxipng synthetic fixtures (`filter_0_*`) | refilter + `zlib -9` | up to **−36 %** on 16-bit RGB, because the original used a poor filter choice |
| Files with large metadata (`c2pa-signed.png`) | any | no gain; 99.9 % of the file is metadata, which Tzim will not remove |

Interpretation, stated plainly: **on already-optimized files Tzim will often report `already optimal`**, and
that is the correct outcome. The value of the tool is breadth, predictability and a verifiable contract, not
being the fastest way to squeeze a PNG. For the last percent of PNG ratio, `oxipng -o max` is the better
tool and the README says so.

---

## 4. What is still missing

| Measurement | Why it is missing | Planned |
| --- | --- | --- |
| Windows and macOS binary sizes | the artifact is built and smoke-tested on both, but only the Linux musl bytes are recorded | backlog |
| Regression tracking across releases | the corpus is stable and gated, but nothing plots the trend over time | backlog |
| A ratio comparison on files above 200 KiB | `oxipng -o max` takes minutes per megabyte; the timed run is capped | not planned |
| Peak RSS for ZIP and ICO separately | the benchmark measures per tool, not per handler | backlog |
| JPEG ratio on baseline-only input | `jpegtran` also re-optimizes progressive files, which Tzim refuses, so the published row mixes scope with encoder quality | backlog |

Everything else that used to be listed here is now measured: ratio per format on a fixed corpus, wall clock
single-job and parallel, peak RSS, and the corpus itself as a stable target.

---

## 5. Known resource behaviour (v0.1)

These are properties of the current implementation, recorded so they are not mistaken for bugs:

- **Memory:** each file is read into memory once, and a PNG candidate is buffered compressed (bounded by the
  input size). A PNG is decoded row by row during verification, so pixel memory is one row per decoder, not
  one image. The 2 GiB default `--max-size` therefore bounds worst-case memory per job.
- **Parallelism:** `--jobs` defaults to `min(cores, 4)`, or `min(cores, 2)` with `--zopfli`, which also bounds
  the number of in-flight candidates.
- **Disk:** one temporary file per in-flight file, in the destination directory, removed on every exit path
  except `SIGKILL`.
- **CPU:** `--zopfli` is 10–100× slower than `zlib -9`; it is off by default.
