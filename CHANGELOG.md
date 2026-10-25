# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

**v0.4.0 was the first published release.** Versions 0.1 to 0.3 were development milestones: they exist as
commits, were tested in CI, and were never tagged or packaged. Everything in them is contained in 0.4.0.
Planned releases are described in [docs/roadmap.md](docs/roadmap.md); the packaging policy is in
[docs/releases.md](docs/releases.md).

## [Unreleased]

### Added

- **`--info` states the JPEG verification level of the build it is in** (#66). The shipped binary verifies a
  JPEG candidate with the codec that wrote it, and [guarantees §2.6](docs/guarantees.md#26-verification-independence)
  has said so since v0.4 — but the tool did not, so a user could only learn it from the documentation. The line
  is read from the build's own features: with `jpeg-v1` it names the independent decoder *and* the local codec,
  because the oracle is added to the comparison rather than replacing it; without it, it says the codec in this
  build only and spells out the flag that gets the other one; in a build with no JPEG handler there is no line
  at all, since there is no verification to describe. The cost is a string, and
  `info_names_the_jpeg_verification_level_of_this_build` reads the same flags the binary was compiled with.

### Fixed

- **A `FillOrder 2` file that is already Deflate is told the truth about itself, and it is not good news.**
  v0.13.0 refuses that combination with "Deflate would make the file one libtiff rejects", which is the right
  sentence for an uncompressed file and the wrong one for a file that **is** already compressed — and files of
  that kind are what tzim 0.7 through 0.12.1 produced, by reversing the bits of the samples and of the stream
  together. Such a file cannot be read by libtiff, cannot be repaired by re-running anything, and its owner
  needs to know that instead of being sent round the same loop: the note now says the compressed data's bits
  are reversed, that re-compressing cannot undo it, and that the way out is a backup or the original source.
  Behaviour is unchanged — `skipped`, exit `0`, byte-identical file — and
  `tiff_fill_order_two_with_deflate_says_the_file_cannot_be_repaired` pins it. The note also stops claiming to
  know where the file came from: reversed bits with Deflate is the combination 0.7 through 0.12.1 produced, but
  any tool that reversed the bits of a stream would leave the same bytes, so the sentence now names the
  combination rather than the culprit. `docs/guarantees.md` §4.8 now
  states plainly that **files damaged by 0.7 through 0.12.1 are not recoverable** and why nothing can tell them
  apart from a legitimately written reversed-bit file.
- **The `jpeg-v1` refusal note lost a line continuation** and shipped as one sentence with twenty-two spaces
  in the middle of it. It reads as a sentence again, and `the_refusal_note_is_one_spaced_sentence` fails on any
  run of two spaces in it.

### Documentation

- **`jpeg-decoder` at `opt-level = "s"` is measured, and it is not enough** ([size budget
  §3](docs/size-budget.md#3-feature-matrix)): the feature's cost falls from 118 784 to **102 400 bytes**, which
  still leaves the `jpeg-v1` build 32 768 bytes above the 1 049 440-byte condition issue #39 was approved
  under. The override is not in the tree, and the number is recorded so the next attempt starts from it rather
  than from a guess. **The decision that followed is on record too**
  ([roadmap](docs/roadmap.md#v013--bytes-a-line-and-a-second-opinion--implemented)): turning the feature on was
  within reach at 1 082 208 bytes and was declined, because JPEG is the one handler that is also off at run
  time, so those 102 KiB would be carried by every user for a format most of them never enable — and paying
  for it would mean moving the `size` gate off the zero-tolerance number it was just tightened to.

### Changed

- **PDF is refused after measurement** (#57). [pdf-audit §9](docs/pdf-audit.md#9-the-measurement-method-corpus-and-the-rule-that-is-fixed-in-advance)
  ran against 2 208 files from `mozilla/pdf.js`, `qpdf` and the veraPDF corpus, with the thresholds fixed before
  the run. **1 009 files (45.7 %)** fall inside the `FlateDecode`-only slice, far more than the rule required,
  and the typical accepted file saves **0.08 %** of its bytes when every permitted stream is re-deflated at level
  9 — 0.15 % with `zopfli`, 0.59 % with `qpdf`'s own `--recompress-flate`, which also rebuilds object streams
  and so measures more than the contract allows. `qpdf --check` passed on all 1 009 candidates and 173 rendered
  pages from 140 files were pixel-identical, so nothing was refused for unsoundness: it was refused for being a
  rounding error. What repacking would buy is **20.1 %**, which is the price of the invariant stated in bytes.
  [guarantees §4.11](docs/guarantees.md#411-pdf--refused-after-measurement) keeps the contract and names it
  refused, as FLAC's did.
- **FITS is admitted by measurement** (#59). [fits-audit §8](docs/fits-audit.md#8-the-measurement-and-the-rule-fixed-in-advance)
  measured 178 files from `astropy`, `cfitsio`, `drizzlepac` and `reproject`. Rewriting an uncompressed image HDU
  as a tile-compressed binary table saves a median **21.9 %** of the file (Q1 17.4 %, Q3 27.3 %) on **84.8 %** of
  the corpus, against a bar of 8 % and 15 %, with every array compared bit for bit after the round trip;
  recompressing tiles that are already `GZIP_1`/`GZIP_2` measured **0.00 %**, because the ones in the corpus are
  already at roughly level 6. The first tier is admissible as an opt-in `--fits-tile`; the second does not ship.
  No handler exists — [guarantees §4.12](docs/guarantees.md#412-fits--admitted-by-measurement-not-implemented)
  now says "admitted, not implemented" rather than "not yet justified".
- **A correction the FITS audit earned:** the tile payload of `GZIP_1`/`GZIP_2` is a **gzip** member (RFC 1952,
  CRC-32 and `ISIZE`), not an RFC 1950 zlib stream as [fits-audit §0](docs/fits-audit.md#0-what-is-a-fact-here-and-what-had-to-be-re-read-from-the-source)
  had assumed, and the `GZIP_2` permutation is the decreasing order of significance the convention states. Both
  were marked "re-read before coding"; re-reading them is what produced the correction.

### Errata for 0.12.1

The release notes said the binary was the same size and the five release **tarballs** each grew by 514–634
bytes, which looked like an unmeasured growth (issue #62). It was not: the `size` job printed
`static musl binary: 1049440 bytes (1024 KiB)` on all eight runs of that pull request, from the first commit
to the merge commit, so no commit changed the binary at all. The deltas are `tar`'s 512-byte member padding
and gzip's packing: the man page's version string is the one member that differs, and its compressed length
moves the DEFLATE stream the unchanged binary sits in. The claim stands, the evidence is now written down in
[size-budget §1](docs/size-budget.md#errata-the-v0121-same-size-claim), and the gate in `ci.yml` now compares
the binary with the previous release's measurement instead of a ceiling 150 KiB above it — a green `size`
job that only checks an upper bound is what made the question reasonable.

## [0.13.0] - 2026-10-25

Six issues, one of them a removal: the progress line is drawn here again, a crate nobody here wrote now
stands on the far side of the JPEG comparison, `--jobs` stops pretending to bound memory, and CI gains the
three coverage gaps and the independent-decoder job that were listed against it. The binary is
**979 808 bytes (956 KiB)**, 68 KiB smaller than v0.12.1: the 96 KiB that `indicatif` and `console` were
paying for came back, and the independent JPEG decoder — 116 KiB — does not ship, for the reason recorded
under *Changed* below.

### Added

- **`--memory-budget <BYTES>`** (issue #51, owner decision O2-D). `--jobs` bounds how many files are in
  flight, never how many bytes they need, and four workers at the defaults could reach several GiB on inputs
  of a few megabytes — killed by the OOM killer with no message. The new flag is checked against a computed
  worst case **before the file is read**: the input, the candidate read back for verification, the decoded
  buffers on both sides, and `--max-uncompressed` for a container. A file above the budget is `skipped` with
  a reason naming the flag, exit `0`, and it never reaches `fs::read`. The default is derived from the other
  ceilings (`4 × --max-size + 8 × --max-pixels + --max-uncompressed`), so no file they admit is ever skipped
  by default — which also means the default is not protection against a machine with less memory than those
  ceilings promise. Say how much you have:
  `tzim --jobs 4 --memory-budget 2147483648 ~/Photos` ([safety §7.1](docs/safety.md#71-memory--an-enforced-budget)).
- **`jpeg-v1`**, an independent decoder for verification only (issue #39), **available but off in the
  published builds** — see the size note at the end of this section. `verify`
  compares coefficients, structure and bytes through code that lives in this repository; a defect symmetric
  in the encoder and the decoder was invisible. `jpeg-decoder` now decodes both sides of an accepted
  candidate, one buffer at a time, and the pixels must be identical: a disagreement is `failed`, exit `1`,
  original untouched. If the independent decoder cannot read the *original*, the file is `skipped` with that
  reason, because an oracle that cannot read a file is not a verdict about it. The feature is separate from
  `jpeg` and the whole comparison can be removed with one flag ([dependencies
  §2.10](docs/dependencies.md#210-jpeg-decoder--the-second-opinion-on-a-jpeg-candidate)).
- **A third-party verification job** (issue #54): Pillow for PNG/JPEG/TIFF/ICO, the standard library's `zlib`
  for gzip and zlib streams, `zipfile` for archives — on the downloaded corpus, comparing pixel data, mode
  and dimensions for images and decompressed bytes for everything else, with the library versions printed.
  `--self-test` mutates a candidate on every run and requires the gate to reject it, so a comparison that
  silently stops comparing fails instead of passing. Zero bytes in the binary and no new Rust dependency
  ([testing §3.3](docs/testing.md#the-third-party-decoder-job)).
- **The refused set has a baseline** (issue #36): `corpus.sh` writes `refusals.txt` and compares its counts,
  per reason, with `.github/corpus-refusals.txt`. `docs/testing.md` had been promising that "the set of
  refused files is unchanged" fails the run since 0.10, and nothing checked it.
- **The fuzz build covers all seven handlers**: `tiff` and `zlib` were missing from `fuzz/Cargo.toml`, so two
  parsers never saw fuzz input, and `strip` had no seeds at all. Pillow's TIFF fixtures and generated zlib
  streams now seed `roundtrip`, `strip` and a new `tiff`/`zlib` corpus (issue #36).
- **zlib is inside the corpus run** (issue #36): four streams are assembled by `corpus-fetch.sh` from real
  payloads, so the handler is regression-gated on real data instead of measured only by the benchmark.

### Changed

- **The progress line is hand-drawn again** (issue #53): `indicatif` and `console` are gone, 96 KiB with
  them — and that saving is why the published binary does not grow while a whole defensive codec was added
  and removed from the default set, and `src/progress.rs` draws the same information — spinner, clock, bar, counter, rate, ETA — with one
  write per frame from a single ticker thread. The conditions [issue
  #53](https://github.com/Rudolf1517/tzim/issues/53) set on the removal are met: the line is prettier than
  the crate's, not plainer, and it fixes the two things the crate's version got wrong — an ETA that appeared
  after a single sample and a frame that could be wider than the terminal. Text is clipped by *display*
  width, through the wcwidth table the report already used, so a CJK file name cannot break the alignment;
  the width comes from `COLUMNS`. Nothing is drawn when stderr is not a terminal, and the first frame still
  waits 150 ms.
- **A file that used to be processed may now be skipped**: that is `--memory-budget` working as configured.
  Nothing changes with the default budget, and a run that sets it should expect the skips and read the note.
- **The `size` job's gate is the binary this release ships**, to the byte: 979 808 bytes, after a release
  whose gate left 150 KiB of headroom underneath a green check (issue #62). `jpeg-v1` was approved on the
  condition that the shipping binary does not grow past 1 049 440 bytes — the v0.12.1 measurement — and
  **it does not fit**: the run that carried it measured `bytes: 1098592, budget: 1049440`, so the feature
  ships off rather than the condition being renegotiated. That is the gate doing the one job a reviewer
  could not do by reading the diff.

### Fixed

- **TIFF: `FillOrder 2` is refused instead of being compressed into a file libtiff rejects** (found by issue
  #54's job, on its first run). Five Pillow fixtures — `tiff_gray_2_4_bpp_hopper{2,4,2I,4I}R.tif` and
  `16bit.r.tif` — were read by Pillow, rewritten by tzim, and then refused by Pillow with `decoder error
  -2`. All five store the bits of every byte reversed and leave the strips uncompressed; libtiff reverses the
  bits of a *compressed* strip before it inflates it, so `FillOrder 2` is decodable only while the strip is
  uncompressed, and the rewrite had been shrinking files by breaking them. Reproduced by hand against Pillow
  13/libtiff: deflating `hopper2R.tif` and keeping the tag reproduces the error, while the same file with
  `FillOrder 1`, or without the tag, reads back pixel-identical. The file is now `skipped` with a reason, like
  LZW and tiled layouts ([guarantees §4.8](docs/guarantees.md#48-tiff-classic-baseline-strips--v07)), and
  `tiff_reversed_bit_order_is_skipped` fails against the writer as it was. Nothing was wrong inside tzim's own
  comparison — which is the blindness the external decoder exists to cover.
- **The zlib note no longer claims more than the decoder knows** (issue #62). `nothing decodes behind the
  header` used to be reported as `this is not a zlib stream`, which is false for a real stream truncated to a
  few dozen bytes. The note now says `either the header was a coincidence or the stream is too short to
  tell`, and `docs/guarantees.md` §4.10 says the same ([cli §2](docs/cli.md#2-options)).

### Documentation

- `docs/dependencies.md` §2.9 records the removal of `indicatif` next to the argument for adding it, and
  §2.10 argues `jpeg-decoder`; §2.5 has five direct dependencies and now every one of them is behind a
  feature. `docs/safety.md` §7.1 replaces "an upper bound, not a budget" with the enforced limit, its
  default and the arithmetic. `docs/benchmarks.md` §3.1 is the v0.12.1 corpus table — the numbers that
  stopped being true when twelve APNG fixtures began to be refused — and `docs/size-budget.md` carries the
  errata for the tarball deltas above. `docs/testing.md` gains the third-party job, the zlib corpus row and
  the refusal baseline.

## [0.12.1] - 2026-10-25

One user-visible behaviour change, one channel removed, and the last three traceability rows that had no test.
The binary is the same size.

### Fixed

- **A coincidental zlib header no longer fails the run.** RFC 1950's header check is two bytes divisible by 31,
  which 1 random two-byte pair in 1984 satisfies — so a tree of 2000 unrelated files has a **~63 %** chance of
  containing one, and until now that file was reported as corrupt: `failed`, exit 1, whole run red. The rule is
  now by how far the stream got: **nothing decoded → `unsupported`, exit 0**, with a note saying the header was
  a coincidence and the file is untouched; **something decoded and then failed → `failed`, exit 1**, unchanged.
  A truncated `.zlib` that produced bytes still fails, and the two cases are never merged; the cost of making
  the rule decidable is that a stream cut so short that nothing decodes is `unsupported` too, because at that
  length it is byte-for-byte indistinguishable from a coincidental header. Nothing is lost but the label: the
  file is untouched either way. The classifier is the error the handler
  raises, not a second rule in the pipeline: `Error::Unsupported` is the one variant a handler may use to
  overrule the detector, and detection itself is untouched — `BTYPE = 3` would divide the probability by 1.33
  and a trial decode at detection time would cost a pass over every unrecognised file.
- **An inconsistent APNG frame sequence is now refused by this program.** Nothing in the handler validated
  `fcTL`/`fdAT` numbering: the writer copies the chunks number for number, so a broken sequence survived to
  verification, where the `png` crate was the only thing that objected — and a decode that stops on *both*
  sides of a comparison is indistinguishable from the end of the frame loop, so that is not a verdict this
  program can rely on. `png::check_frame_sequence` now requires `fcTL`/`fdAT` to number themselves 0, 1, 2, …
  with no gaps and the number of `fcTL` chunks to equal the frame count `acTL` declares. A broken animation is
  `failed`, exit 1, and nothing is written. The real-file corpus had the evidence: twelve of Pillow's own
  negative APNG fixtures were being rewritten and reported as `optimized`, and they are now `failed` and listed
  in `.github/corpus-allowlist.txt` with that reason.

### Added

- **Adam7 in the fixture builder**, and the scanline filter `PngSpec` asks for is now applied to the data
  instead of being written in the row's first byte — a builder that could produce a "Sub" row that was never
  filtered could produce a file no decoder accepts. `T-PNG-04` has a test at last: an interlaced image is
  recompressed, every non-`IDAT` chunk is byte-identical, and the decoded pixels are compared.
- **`T-FS-11` … `T-FS-16`**, the filesystem scenarios that `docs/testing.md` had been listing as untested: a
  FIFO (skipped, and the mtime proves nothing opened it), a device node, a full disk, a symlink cycle at the
  64-level depth limit, a read-only directory, and a tree 70 levels deep.
- **A 2 MiB tmpfs in the Linux `test` job** so the full-disk test runs rather than skipping: the test fills the
  filesystem to the last page, then requires `failed`, the original intact and no `.tzim-*.tmp` left behind. It
  is gated on `TZIM_TEST_FULL_DISK` and prints that it was skipped when the variable is unset; if the runner
  refuses the mount the step warns instead of failing.

### Removed

- **The winget channel.** `packaging/winget/` — nine versions of three manifests and a verifier, 152 KiB —
  described a submission to `microsoft/winget-pkgs` that never happened, plus a `windows-latest` CI job, two
  tests and twenty lines of submission instructions to keep the fiction consistent (issue #56). Windows
  installs from the release zip: download `tzim-x86_64-pc-windows-msvc.zip`, extract it, put `tzim.exe` on
  `PATH`. The Homebrew formula and the Linux packages are untouched, and a future package manager on Windows
  belongs in the release workflow, submitted from CI, not in nine hand-kept directories here.
- **`T-NEST-04`**, with the reason written where the row was: there is no per-child "remaining budget" —
  `zip::check_limits` refuses the archive on the sum of its declared uncompressed sizes before decompressing
  anything, so a child is never larger than `--max-uncompressed` and the per-entry guard cannot fire. The
  contract now says that, instead of asserting a limit that does not exist.

### Changed

- **What stops a symlink cycle is now the measured answer.** `cli.md` said cycles are stopped by the 64-level
  depth limit; the test for that row failed, because the kernel stops a path with more than a fixed number of
  symlink hops first (`ELOOP` — 40 on Linux, 32 on macOS) and the cycle never reaches 64. The depth limit is
  what stops a tree of *real* directories at 64 levels and what protects the stack, and `T-FS-16` proves that
  part. A cycle is `failed` with the OS reason, exit `1`, and the rest of the run is unaffected.
- `docs/guarantees.md` §4.10 states the zlib split and the two probabilities; §4.5 says the frame sequence is
  checked by this program; §4.6 says what `--max-uncompressed` actually limits. `docs/cli.md` gains the one
  case where `unsupported` carries a note, and says which error kinds can never appear in `error`.

## [0.12.0] - 2026-10-25 — the audit

An independent audit of 0.10.0 and 0.11.0 produced twenty-three issues. This release answers them, with the
rule the audit itself states: `docs/guarantees.md` is the contract, so a divergence between the code and the
document is a bug in one of them, and every fix ships with a test that fails before it. Nothing here changes
what a supported file turns into, and the single exception is a defect being removed. The binary measures the
same 1 049 440 bytes.

### Fixed

- **`--strip-metadata` no longer turns an equal-size result into a failure.** A ZIP with a stored entry and
  non-zero timestamps, or a gzip member with a non-zero `MTIME`, can be rewritten to exactly the same length:
  zeroing a timestamp removes metadata without removing bytes. The handlers accepted that candidate and the
  pipeline then called it an internal error and exited 1. The rule now lives in one function,
  `formats::size_ok` (strictly smaller, or the same length under the flag, never larger, never empty), and it
  is called by the pipeline, by the verification dispatcher and by both fuzz targets, so the three cannot
  disagree again.
- **G3 clause 4 ("the candidate is not larger") is checked in verification**, as the contract says, instead
  of being implied by whichever caller remembered it.
- **`--info`, `--list-formats` and `--check-deps` describe the build they are in.** They claimed that metadata
  is never removed (untrue since the flag exists) and named JPEG and Zopfli unconditionally, which is wrong
  for `--features png`. Both are now read from `cfg!`.
- **`unsupported: unsupported format`** — the note repeated the status word on every such line and in every
  JSON record. The status is the message, so the note is `null`.
- **`docs/safety.md` §1 describes the algorithm the code runs.** The snapshot comes from `lstat` and the file
  is opened twice, not "from the open handle" and not once; the temporary file is
  `.tzim-<pid>-<nanos>-<attempt>.tmp` and a successful run renames it rather than deleting it; §8's
  ownership rule is Unix-only and says "skipped with a reason", not "warned". The step order now matches
  `pipeline::run` — size limits, hard links and read-only before detection, `--formats` before the candidate
  exists.
- **G1 is no longer "verified by a content hash".** No hash exists in the program; G1 holds because nothing is
  written on those paths, and the only comparison made is the re-check of `(device, inode, length, mtime)`
  before replacement.
- **The TIFF `--strip-metadata` list is in the contract.** `guarantees.md` §2.7 pointed at §4.8 for a list
  that was not there. §4.8 now carries both lists — the tags whose out-of-line values may be moved and the
  exact tags the flag removes — and `the_stripped_tag_list_is_the_documented_one` fails if the constant in
  the code moves without the section, with a message that says so.
  `cli.md` gains `Artist` (315) and states that GDAL nodata (`42113`) is kept.
- **A path with CJK or emoji characters no longer breaks the column alignment**: the report measures display
  cells, not characters, and a combining mark counts as zero.
- **`docs/cli.md` matches the output.** The human example showed `unsupported: unsupported format` (the
  status, then the note), the JSON example carried a note that no longer exists, the format ids are now the
  ones `Format::id()` emits (no `apng`, `zlib` included), `--max-pixels` is marked as applying to PNG and
  JPEG, and the symlink section says what really protects a walk: a 64-level depth limit, not a visited set
  of directories.
- **`docs/testing.md` stops promising mechanisms that do not exist.** The failure-injection hooks
  (`TZIM_TEST_*`), the `proptest` suite, the committed `tests/fixtures/` corpus and the "refused set"
  baseline are gone or explicitly marked as not implemented; the fixture section describes the builders and
  what each one actually covers.
- **Eight broken internal links and anchors** are fixed (`CHANGELOG.md`, `docs/flac-audit.md`,
  `docs/roadmap.md`), and `CONTRIBUTING.md` no longer names a `png_reader` fuzz target or a
  `tests/fixtures/generate/` directory that do not exist.
- **The Adler-32 test comment** derived the wrong numbers for an all-zero input (it summed 0..9999 instead of
  the bytes). The value was right; the explanation would have led the next reader to break the code.

### Added

- **`tests/verification.rs`**: one negative test per verifier. Until this, `png::verify`, `jpeg::verify`,
  `tiff::verify`, `zip::verify`, `gzip::verify` and `verify_icon` were never called by a test, so a verifier
  that accepted everything passed the suite. Two rules are deliberately not covered, and the module says
  which and why: a test that survives the deletion of the check it guards is decoration.
- **`docs/guarantees.md` §3.1 states what "verified" compares**, format by format: which bytes, which
  fields, which decoded pixels, and what is deliberately not compared — including that two byte-identical
  APNG frames swapped is not detected, that the comparison is strict rather than semantic, and that a defect
  symmetric in the encoder and the decoder passes every check here.
- **`T-FS-01…10` and `T-STRIP-01…08` in `docs/testing.md` §2**: the filesystem, replacement and
  `--strip-metadata` tests now have ids, so the traceability table covers the promises that protect the
  original file.
- **Seven traceability rows that had no test now have one**: `--zopfli` never worse than the default, gzip
  `FCOMMENT` and `FHCRC`, ZIP unknown-method and multi-disk archives, `--max-uncompressed`, and an ICO
  directory that runs past the end of the file. Three rows say **no test** in the row itself and give the
  reason: an interlaced PNG (the fixture writer cannot produce one), an inconsistent APNG sequence (the
  `png` crate refuses it first) and the nested size budget.
- **Tests for filesystem cases that had none**: a hard link (skipped, then broken on purpose with
  `--allow-hardlinks`), a Unix socket, and — on Linux, where the filesystem allows the name — a path that is
  not valid UTF-8, with `path_raw_b64` in the record.
- **Tests that pin `--formats` as a policy rather than a filter**: a PNG under `--formats gzip` is `skipped`
  with a reason, and a nested child inherits the allowlist.
- **PNG scanline filters are round-tripped at every bytes-per-pixel value from 1 to 8**, not only RGBA8's
  four.
- The fuzz build compiles `tiff` and `zlib`, so those parsers are reachable, and the `strip` target has seeds.
- **`docs/pdf-audit.md`, and PDF's contract in `docs/guarantees.md` §4.11.** The format stays refused, but from
  now on it is refused by evidence rather than by opinion: the audit describes the only operation that would be
  in scope (recompressing `FlateDecode` payloads inside a byte-preserved object graph — the existing zlib path
  in a new container, no new dependency), lists every construct that forces a refusal (encryption, signatures,
  linearization, incremental updates, object streams, any other filter), and fixes the decision rule **before**
  the numbers exist: median gain ≥ 3 % with ≥ 10 % of the corpus accepted admits the format, median < 1 %
  refuses it with the measurement on record. No figure in it is measured, and every one is labelled as an
  estimate.

- **`docs/fits-audit.md`, and FITS's contract in `docs/guarantees.md` §4.12.** FITS was never refused — it was
  absent from `docs/formats.md` §5, which is worse, because a format nobody looked at reads like one nobody
  considered. The audit separates the two operations that matter: recompressing the zlib tiles of an already
  compressed image HDU (small gain, no structural change), and writing an uncompressed image HDU as a
  tile-compressed table (20–50 % on float data, and the same class of change as TIFF `Compression 1 → 8`). It
  also states, in a table, **which claims were not re-read from the standard** — the tile payload being RFC 1950
  and the exact `GZIP_2` byte shuffle — because both change the design rather than a detail, and guessing either
  would produce files that decompress to garbage. Tier 2 is recommended behind a flag, on social grounds:
  pipelines index `NAXIS1` on an `IMAGE` HDU and would break.
- **`docs/formats.md` §5 corrected in two places.** FITS and DICOM now have rows — FITS as under measurement,
  DICOM as never evaluated — and the `xz / bzip2 / zstd / 7z` row no longer blames "binary cost and unpredictable
  gains". For zstd that was the wrong reason: the format passes our own criteria (four-byte magic, XXH64
  checksum in the frame, flat frames instead of an object graph, 10–30 % between level 3 and 19) and is blocked
  by exactly one thing, `dependencies.md` §1 forbidding a C toolchain. The row now says so, and says what would
  change the answer.

### Changed

- **`docs/safety.md` §7.1 no longer implies that `--jobs` bounds memory.** It bounds how many files are in
  flight. With the defaults, four workers can reach several GiB on inputs of a few megabytes: the whole file
  and its candidate are held in memory, and a 100 MPix PNG costs about 800 MiB of decoded buffers on its own.
  The numbers are in §7.1, with the two flags that actually reduce them; a computed budget is issue #51.
- **The symlink story is the real one**: a cycle is stopped by a depth limit of 64 directories, not by a
  visited set of `(device, inode)` pairs.
- **`docs/roadmap.md` marks every status note as history**, so its v0.1 test counts cannot be read as today's
  state, and records the seven decisions taken after the audit.

### Not in this release

Two audit items are owner decisions, not defects, and stay open: what to cut to make the binary smaller
(#38), and whether to verify JPEG with an independent decoder and a new dependency (#39).

## [0.11.0] - 2026-10-25 — distribution

No handler changed and no file is treated differently. This release is about the three minutes between
`curl` and a working tool: what the installer checks, what the packages contain, and which architectures
exist at all.

### Added

- **A Linux ARM64 binary** (`aarch64-unknown-linux-musl`), cross-linked on an x86_64 runner with the
  toolchain's own `rust-lld` and the self-contained musl libc that ships with `rustc`, so no cross C
  toolchain is involved. ARM64 servers and single-board computers now install a binary instead of compiling
  one, and `tzim_arm64.deb` / `tzim.aarch64.rpm` exist next to the x86_64 packages.
- **A manual page** (`man/tzim.1`) and **completion for bash, zsh and fish** (`completions/`), installed by
  the `.deb`, the `.rpm` and the Homebrew formula and shipped inside the POSIX archives. `man tzim` and tab
  completion now work after a package install.
- **`TZIM_BASE_URL`** in `install.sh`, for a mirror. The checksum check applies there too.
- **A `package` CI job**: `cargo package --locked`, then a build from the packaged tarball — the crate as
  crates.io would receive it, so `cargo install tzim` is one `cargo publish` away rather than a rewrite.
- **An `installer` CI job**: runs `install.sh` against the latest published release on Linux and macOS, then
  serves a doctored archive with one byte appended next to the release's real `SHA256SUMS` and asserts that
  the install aborts, says `SHA-256 mismatch`, and leaves nothing behind.

### Changed

- **The winget manifest is checked against the release, not just against a schema.** `winget validate` reads
  the manifest as a document; `packaging/winget/verify.sh` downloads the archive it names and compares the
  hash, which is the half that decides whether an install works. A new `winget` CI job on Windows runs both,
  and `docs/releases.md` §5 has the exact steps for submitting to `microsoft/winget-pkgs`, since that pull
  request is against a repository this project does not own.
- **`install.sh` verifies what it downloads.** The archive is checked against `SHA256SUMS` before it is
  unpacked; a mismatch prints both hashes and exits instead of falling back to a source build that would hide
  it. The installed binary is run once before success is reported, and with `TZIM_VERSION` set the reported
  version has to match. A `curl | sh` installer that trusts the network more than the project's own checksums
  is a strange thing to ship next to this program.
- **The release archives carry the man page and the completions** next to the binary, so the formula, the
  packages and the repository install the same text.
- **`install.sh` covers Linux ARM64 and Intel macOS**, and names every platform the release matrix builds.
- **`tests/packaging.rs`** fails when an option in `--help` is missing from the man page or any completion,
  when the man page's version drifts from the crate, or when a release target is not fetchable by the
  installer.
- The `.deb` and `.rpm` description now lists the formats the binary actually has: it had been stuck at the
  v0.4 set, without TIFF and without zlib.

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.11.0/` are pinned to the published v0.11.0 artifacts and their
  SHA-256, copied from the release notes after the build. The formula gained the two platforms this release
  made possible: Intel macOS and Linux ARM64.

### Size

- **+0 bytes.** 1 049 440 bytes (1 024 KiB), the same number as v0.10.0: this release adds packaging, a
  manual page and an architecture that is built from the same source. One feature row moved by 4 KiB because
  the linker crossed a page boundary, which is recorded in [size-budget §3](docs/size-budget.md#3-feature-matrix)
  rather than smoothed over.

### Fixed

- **`--strip-metadata` was accepted by the parser and missing from `--help`** since v0.6. It is documented
  now, and the completions and the man page list it.

## [0.10.0] - 2026-10-25 — the claims around the code

No handler changed and no file is treated differently: the corpus writes the same total, and every contract
and refusal is as it was. This release is about the sentences around the code that had stopped being true —
in CI, in the benchmarks, in the README, and on the progress line.

### Added

- **`indicatif` 0.18 draws the progress line**, at the user's request and for the first time since v0.1 with a
  new dependency. One file gets `⠋ converting…`; a batch gets `⠋ 00:01 [████░░░░] 3/12` — the clock, an
  eight-cell bar and the position. Still stderr only, still nothing in a pipe, still absent with `--quiet`,
  `--json` and `--no-progress`, and still erased before the report prints. The templates, the `mm:ss` clock
  and the 150 ms delay stay in `src/progress.rs`; the animation thread and the terminal handling do not.
  [cli §10](docs/cli.md#10-colour-and-progress) has the shapes,
  [dependencies §2.9](docs/dependencies.md#29-indicatif--added-in-v010-removed-in-v013) argues the crate, its eight new
  lock entries and the `unsafe` surface that arrives with them.
- **`msrv`, `fmt`, `deny` and `audit` jobs.** `docs/testing.md` §4 has listed eleven CI jobs since v0.1 and
  the workflow contained three. The missing gates exist now: 1.88.0 compilation, `cargo fmt`, `cargo deny`
  against a new `deny.toml` that carries the licence allow-list, and `cargo audit` against RustSec.
  [`THIRD-PARTY.md`](THIRD-PARTY.md) is the notice file §5 of the dependency policy already referred to.
- **TIFF and zlib rows in the comparative benchmarks**, with a note in the table itself that neither has a
  reference tool that recompresses like this one does.

### Changed

- **Every CI build runs with `--locked`**, which [dependencies §6](docs/dependencies.md#6-supply-chain) has
  been promising since the first release. `Cargo.lock` is written by the `lockfile` job, whose `git push`
  could never have worked — `actions/checkout` leaves a detached HEAD. It now checks out the branch, pushes
  with an explicit refspec, runs on every branch, and shares a concurrency group with `fmt`.
- **`rustfmt` runs over this tree for the first time.** The environment the project is written in cannot run
  it, so the `fmt` job formats and commits, and fails on the run that had to change something. Two jobs now
  write to the branch; [testing §4.1](docs/testing.md#42-the-two-jobs-that-write-to-the-branch) says why and
  what happens on a fork.
- **The comparative benchmarks are this release's.** §3.2 of [benchmarks](docs/benchmarks.md) carried v0.4.0
  numbers taken before the `-Os`/`-O3` profile split, with a paragraph explaining why they were not re-taken.
  They are re-taken, and the run id is recorded next to them.
- **The README fits on one screen**: badges, the promise, the before/after, the format table, and the pitch —
  contract and refusal — where a stranger reads it first. The depth stays in `docs/`.

### Size

- **+96 KiB** (928 → 1 024 KiB), and none of it is a handler. `indicatif` and `console` cost 112 KiB at the
  wildcard `opt-level = 3` and 96 KiB at `"s"`, which is what §2 of the size budget now says; the zlib handler
  meanwhile shrank into a page boundary and now measures 0 KiB. The budget follows the documented rule
  (measured + 15 %): 1 069 → 1 179 KiB. Numbers in
  [benchmarks §1](docs/benchmarks.md#1-binary-size--measured), per-feature table in
  [size-budget §3](docs/size-budget.md#3-feature-matrix).

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.10.0/` are pinned to the published v0.10.0 artifacts and their
  SHA-256, copied from the release notes after the build.
- The release build runs with `--locked` too: the published bytes come from the dependency set that was
  reviewed, not from whatever resolves on the day of the release.

### Fixed

- **The `musl` check accepted only the older `file` wording.** The musl target now links position-independent
  and `file` reports `static-pie linked`; the gate called a perfectly static binary dynamic.

## [0.9.0] - 2026-10-25 — zlib, and a format queue that closes

### Added

- **zlib (RFC 1950)** — the DEFLATE stream is recompressed, the two header bytes and the four Adler-32 bytes
  are copied verbatim, and the contract is [guarantees §4.10](docs/guarantees.md#410-zlib-rfc-1950--v09).
  No new dependency: `flate2` was already in the tree and the checksum is twelve lines of arithmetic.
  Refused with a reason: `CM ≠ 8`, a declared window below 32 KiB (our encoder may exceed it), a preset
  dictionary (the payload cannot be decoded without bytes that are not in the file), and trailing data.

### Changed

- **FLAC is refused after measurement**, not deferred. `libFLAC` at every level on three deterministic
  signals: a stronger encoder buys **0.38 %** on musical material, 1.6 % on a pure tone, and **0 %** on files
  already written at `flac -8`; the encoder plus an independent decoder would cost roughly **150–300 KiB** of
  a 920 KiB binary. The contract stays on record, the code does not get written, and
  [docs/flac-audit.md §7](docs/flac-audit.md#7-decision-refused-and-the-measurement-behind-it) has the table.
- **Raw DEFLATE is refused**, not deferred: a bare stream has no header, no trailer and no signature, so
  recognising one would mean guessing.
- The format queue in [docs/roadmap.md](docs/roadmap.md) is now empty, and every remaining candidate is
  recorded with the reason it is not there.

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.9.0/` are pinned to the published v0.9.0 artifacts and their
  SHA-256, copied from the release notes after the build.

### Size

- **+8 KiB** (920 → 928 KiB), of which 4 KiB is the zlib handler itself — the cheapest format in the table —
  and the rest is a page boundary. The budget follows the documented rule (measured + 15 %): 1 059 → 1 069 KiB.

## [0.8.0] - 2026-10-25 — the terminal, and a contract for FLAC

No output byte changed and no file is treated differently: this release is the interface around the same
work. The corpus still writes 379 177 bytes.

### Added

- **Colour**, on a terminal only: the status word, the saving percentage and the two summary numbers.
  Controlled by `--color=auto|always|never` and `--no-color`, and following `NO_COLOR`, `CLICOLOR=0`,
  `CLICOLOR_FORCE` and `TERM=dumb`. `--json` is never coloured. Windows terminals that cannot interpret
  ANSI sequences are detected and left alone.
- **A progress line** on stderr for runs that take longer than a moment: ASCII frames, the file counter,
  the clock and the running total. Drawn only when stderr is a terminal; `--no-progress` turns it off.
- **The FLAC contract** ([guarantees §4.9](docs/guarantees.md#49-flac--refused-after-measurement)),
  written before any code as every contract here is, with the audit behind it in
  [docs/flac-audit.md](docs/flac-audit.md). No FLAC support is claimed: the handler and its tests do not
  exist, and the implementation is blocked on three crates that have not been approved.
- [cli §10](docs/cli.md#10-colour-and-progress) and [cli §11](docs/cli.md#11-conformance-with-cli-conventions):
  the colour rules, and an audit against the CLI guidelines that found four defects — all fixed below.

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.8.0/` are pinned to the published v0.8.0 artifacts and their
  SHA-256, copied from the release notes after the build.

### Size

- The binary grows by **20 480 bytes** (900 → 920 KiB) in every build: colour, the progress thread, buffered
  reporting and `IsTerminal`. Handlers are unchanged, and the round of size work in 0.7.1 is what pays for it.
  The budget in [size-budget §1](docs/size-budget.md#1-budget) is re-measured, not widened by hand.

### Fixed

- **A closed pipe no longer aborts the process.** `tzim . -r | head -1` used to fail with `EPIPE` inside
  `println!` and, under `panic = "abort"`, kill the process with an unhelpful message. The report now writes
  to a sink, and a closed stdout ends it quietly with the exit code the run had earned.
- **Clustered short flags and attached values** (`-rq`, `-nr`, `-j4`) are accepted, as every getopt-based
  tool accepts them; before they were rejected as unknown options.
- **Usage errors print the usage line**, not only a hint to read `--help`.
- **`--help` matches the binary**: the `--formats` list now names `tiff`, and the colour, progress and
  command-line convention sections are part of it.

### Documentation

- The example in [cli §3](docs/cli.md#3-human-output) showed one decimal and a percentage in the summary,
  neither of which the program has ever printed. The text now matches the binary.
- [dependencies §2.7–2.8](docs/dependencies.md#27-flac-the-crates-it-would-have-needed)
  record the FLAC crates that are not added and why colour and progress are not dependencies.

## [0.7.1] - 2026-10-25 — the same binary, 124 KiB lighter

No format, flag, guarantee or output byte changed. The corpus writes 379 177 bytes in
both 0.7.0 and 0.7.1 builds.

### Changed

- **Release profile**: this crate is compiled with `opt-level = "s"`, its dependencies — DEFLATE, PNG,
  Zopfli, where the time is spent — stay at `3`. Measured on 694 real files: −104 KiB for +3.6 % runtime.
  Optimising the whole tree instead would have saved 124 KiB and cost 11 %. Numbers and rejected variants
  (thin LTO, `opt-level = "z"`, `codegen-units = 16`, `gc-sections`, `force-unwind-tables=no`) are recorded in
  [size budget §5](docs/size-budget.md#5-options-considered).
- **`src/report.rs`** formats sizes, percentages and durations with integer arithmetic instead of the
  general `core::fmt` path: −20 KiB, byte-identical output, with boundary tests on the unit thresholds.
- **Size budget** tightened from 1 178 KiB to 1 036 KiB, so CI refuses growth 142 KiB earlier.

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.7.1/` are pinned to the published v0.7.1 artifacts and their
  SHA-256, copied from the release notes after the build.

## [0.7.0] - 2026-10-25 — TIFF, and a rebuild that can prove itself

### Added

- **TIFF**: classic TIFF (`II`/`MM`, magic `42`) with strips. Uncompressed strips are compressed with
  Deflate; Deflate strips are re-compressed and keep whichever of the two results is smaller. The contract
  is [guarantees §4.8](docs/guarantees.md#48-tiff-classic-baseline-strips--v07); it was written before the
  handler, as every format contract is.
- The file is **rebuilt** rather than patched — a TIFF stores absolute offsets, so a smaller strip moves
  everything after it. Verification then compares what the rebuild could break: every entry as a resolved
  *value* (not as bytes at a fixed position), the tag set, the entry order, the byte order, the strip count,
  and every strip decoded through `flate2` and compared byte for byte. `Compression` is the only value that
  may change, and only across the two transitions that stay lossless.
- `tiff` joins the feature list. It reuses `flate2`, so it adds **no dependency** — only code.

### Changed

- Refusals are part of the format's value, so they are specific: BigTIFF, tiled layouts, more than one IFD,
  separate planes, predictors, `SubIFDs`/EXIF/GPS sub-IFDs, and unknown *out-of-line* tags (which may be
  offsets) are skipped with a reason and the file is left alone. LZW is refused instead of being written by
  an encoder this repository would have to invent and could not verify. Unknown tags that fit inside the
  entry are copied untouched.

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.7.0/` are pinned to the published v0.7.0 artifacts and their
  SHA-256, copied from the release notes after the build — the order that never publishes a hash for a file
  that does not exist yet.

### Documentation

- [formats](docs/formats.md#2-format-matrix) moves TIFF from *research* to *implemented* in the same commit
  as the tests, and [roadmap](docs/roadmap.md) records what shipped; the research queue now starts at v0.8.
- The honest gain is stated next to the claim: 5–30 % on uncompressed scans, 0–5 % on already-Deflate files,
  and nothing at all for LZW — which is why the handler refuses it rather than pretending.

## [0.6.0] - 2026-10-24 — narrowing the build, and metadata on request

### Added

- **`--strip-metadata`**: removes a fixed, documented list of embedded metadata — PNG text and `tIME`/
  `eXIf`, the gzip name/comment/extra/mtime, ZIP comments and DOS timestamps, JPEG `APP1`/`APP13`/`COM` —
  and nothing else. ICC profiles, colour hints, unknown chunks and every pixel, coefficient, payload and
  name stay. Off by default; the only transformation that is not G3, so it gets its own class, G4
  ([guarantees §2.7](docs/guarantees.md#27-g4--metadata-removed-on-request)).
- **Features per handler**: `jpeg` and `ico` join `png`, `gzip`, `zip` and `zopfli`, so a build can drop
  what it will never see. The default set is unchanged and still ships every handler.
- The fuzz target `strip` covers the removal paths, and CI measures every feature combination
  (`ci` job `binary size`, step `Feature matrix`) instead of only the full build.

### Changed

- **For packagers, not users:** JPEG used to be compiled in unconditionally. A build with
  `--no-default-features --features png,gzip` is now smaller and no longer handles JPEG; `--formats jpeg`
  on such a build is a usage error that names the handlers it does have, instead of a silent no-op.
  `ico = ["png"]`, so the two cannot be split.
- With `--strip-metadata` the acceptance rule becomes "never larger" instead of "strictly smaller":
  zeroing a timestamp removes metadata without removing bytes. A larger candidate is still refused under
  every flag.

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.6.0/` are pinned to the published v0.6.0 artifacts and their
  SHA-256, copied from the release notes after the build.

### Documentation

- [guarantees](docs/guarantees.md) now names the **verification level** of every format: an independent
  decoder (PNG, gzip, ZIP payloads), a separate reader (none yet) or a shared implementation (JPEG
  coefficients, ZIP structure), with what a shared checker cannot catch. The strongest claim in the old
  text — "verification never relies on the code that produced the candidate" — was true only for PNG and
  gzip, and is now replaced by the table.
- [size-budget](docs/size-budget.md#3-feature-matrix) lists the feature set as `Cargo.toml` declares it and
  the measured size of each combination; [dependencies](docs/dependencies.md) says that all four crates are
  behind features.
- [safety](docs/safety.md#3-metadata) and the README now separate **content** metadata (preserved, or
  removed on request) from **filesystem** metadata (mode and times preserved; owner, ACLs and extended
  attributes not), because conflating them is how a user loses an ACL.

## [0.5.0] - 2026-10-24 — the file is the command

### Changed — breaking

- **No more subcommand.** `tzim optimize <PATH>` becomes `tzim <PATH>`; the path is the whole command, and
  files, directories and duplicates may still be mixed in one run. The old spelling keeps working — a
  leading `optimize` is stripped before parsing — and so does `tzim formats`, but neither is documented
  outside this entry. Scripts written against 0.1–0.4 do not need editing.
- **`--help` no longer lists verbs.** The usage block is
  `tzim [OPTIONS] <PATH>...` plus the four information commands, and a new `INFORMATION:` section groups
  them.

### Added

- **`-i` / `--info`**: version, the promise in four lines, the formats in this build, G1/G3, homepage,
  docs, licence and contact address, printed from the same tables `--list-formats` uses and exiting `0`
  without touching a single path.
- **`--list-formats`**: the `formats` verb as a flag, so every non-processing action has the same shape.
- **`--check-deps`**: states that there are no external programs at run time, and what the binary is
  statically linked against. It exists because tools that wrap ffmpeg and friends have taught people to
  ask; the honest answer is short.

### Packaging

- **Intel macOS artifact** (`tzim-x86_64-apple-darwin.tar.gz`). The crate is pure Rust, so it cross-compiles
  on the same Apple silicon runner; `install.sh` and `docs/releases.md` pick it up.
- The release smoke test accepts both `--list-formats` and the older `formats`, so tags before 0.5 still
  pass it.
- `Formula/tzim.rb` and `packaging/winget/0.5.0/` are pinned to the published v0.5.0 artifacts and their
  SHA-256, taken from the release notes after the build. The formula now covers Intel as well as Apple
  silicon macOS, and its caveats and test use `--list-formats`.

### Documentation

- `README.md` rewritten around the new shape: contents table, one section per install channel,
  `tzim --check-deps` output, the full flag list in a `<details>`, the three guarantees as a table, the
  format matrix, and a "what tzim will never do" list. Every example is `tzim <path>`.
- `docs/cli.md` documents the path-first grammar, the information commands and their exit codes.

## [0.4.3] - 2026-10-24 — checksums where they can be read

### Added

- **Homebrew formula** (`Formula/tzim.rb`) for the third-party tap, pinned per platform to the published
  artifact and its SHA-256. It unpacks the release, so the tap builds nothing.
- **winget manifest** (`packaging/winget/0.4.3/`) for Windows x64: a `zip` installer with a portable
  nested `tzim.exe`, nothing written outside the package store.
- The README now leads with the install channels and carries the contact address.

### Changed

- The release workflow writes the `SHA256SUMS` block into the release notes and into a check annotation,
  replacing any previous block instead of appending. The asset alone is not reachable from the release
  page, and "copy the hash into the formula" should not need a download of the file being described.

### Fixed

- v0.4.2's notes were built by the previous workflow and carry no checksum block; the assets and
  `SHA256SUMS` were correct. Nothing else differs.

## [0.4.2] - 2026-10-24 — install from a package manager

### Added

- **`install.sh`**: `curl -fsSL .../main/install.sh | sh` takes the prebuilt binary for the platform into
  `~/.local/bin`, and falls back to `cargo install --git` where no build is published. `TZIM_REPO`,
  `TZIM_VERSION`, `TZIM_INSTALL_DIR` and `TZIM_BRANCH` override its choices.
- **Debian and Fedora packages**: `tzim_amd64.deb` and `tzim.x86_64.rpm`, built by
  `packaging/linux-packages.sh` from the same static musl tarball, with no runtime dependency declared.
- **Homebrew formula** (`Formula/tzim.rb`) for a third-party tap, and a **winget manifest**
  (`packaging/winget/`) for Windows x64.

### Changed

- Release assets are named after the Rust target triple and **carry no version**
  (`tzim-x86_64-unknown-linux-musl.tar.gz` instead of `tzim-v0.4.1-linux-x86_64.tar.gz`), so
  `releases/latest/download/<asset>` is a stable URL for the installers.
- The release workflow now also builds the two Linux packages and folds them into `SHA256SUMS`.

### Fixed

- Nothing user-visible. The binary content is unchanged from 0.4.1.

## [0.4.1] - 2026-10-24 — the evidence behind the claims, no functional change

### Added

- **A real-file corpus in CI** (`corpus.yml`): four pinned upstream archives — libpng 1.6.44,
  libjpeg-turbo 3.1.0, image-rs 0.25.6, Pillow 11.3.0 — each verified by SHA-256 and assembled into 572
  images plus four gzip and ZIP containers built from them. The binary runs over the tree twice; the job
  fails on any `failed` status that is not a known-corrupt upstream fixture, on any `verify` or `internal`
  error, and on any file still being rewritten on the second pass.
- **Fuzz targets** (`fuzz/`, `cargo-fuzz`, nightly): `jpeg_decode` covers the marker parser, the layout
  analysis and the entropy decoder; `roundtrip` covers detection, the handler and verification for every
  format. Sixty seconds per target on pull requests that touch the parsers, longer before a release.
- **Comparative benchmarks** (`bench.yml`) against oxipng 10.2.1, optipng 0.7.8 and jpegtran (libjpeg-turbo
  2.1.5) on copies of the same corpus, reporting wall clock, peak RSS and bytes saved.
- The format handlers are a library now (`src/lib.rs`), so the fuzz targets can reach the parsers. The entry
  points exist only behind the `fuzz-api` feature, which no release build enables.

### Changed

- `docs/benchmarks.md` §3 reports measured results instead of "not measured yet", and §4 shrinks to the two
  gaps that are still real.
- Release, size and benchmark builds use default features. `--all-features` would compile the fuzz entry
  points into the binary that is measured and shipped.
- Measured static binary: 980 KiB (+4 KiB, a linker page boundary, not new behaviour). Still four crates.

### Fixed

- Nothing user-visible. No format handler changed, and no file that was optimized by 0.4.0 is treated
  differently by 0.4.1.

## [0.4.0] - 2026-10-24 — first published release

### Added

- **JPEG (opt-in, `--formats jpeg`).** Entropy-coded data is decoded to DCT coefficients and re-encoded with
  optimal Huffman tables (Annex K.2, lengths limited to 16 bits, all-ones code excluded). No coefficient
  changes, so the decoded image is bit-identical. Every other marker — Exif, ICC, `COM`, `DQT`, `SOF0`,
  `DRI`, the `SOS` header and any bytes after `EOI` — is copied as a raw segment.
- Baseline sequential only: progressive, arithmetic-coded, lossless, hierarchical, 12-bit, SOF1 and
  multi-scan files are refused (`skipped`, exit 0) with a reason. Restart intervals are supported and
  preserved, including the `RSTn` markers inside the scan.
- Verification compares **every DCT coefficient** of every component, plus the ordered marker sequence,
  the frame header, unused Huffman tables and the trailing bytes. Coefficient identity implies pixel
  identity, so no second decoder is involved.
- The Huffman table generator is covered by a 4 000-case property test that validates every table it
  produces (no overfull code space, no duplicates, no all-ones code, every used symbol encodable).
- `--formats <LIST>`: an allowlist over `png`, `gzip`, `zip`, `ico`, `jpeg`. JPEG is off unless named;
  nested children inherit the same allowlist.
- Nine integration tests for JPEG and one for the allowlist.

### Changed

- Documentation: `docs/guarantees.md` §4.7 rewritten as the actual contract, `docs/dependencies.md` §2.6
  records why this codec is hand-written instead of delegated to a crate.
- No new dependencies and no `unsafe`; the dependency count stays at four. Measured static binary size:
  976 KiB (+72 KiB over v0.3).

## [0.3.0] - development milestone, shipped in 0.4.0

### Added

- **APNG.** Every frame is recompressed. The recompressed stream is split back into the same number of
  `fdAT` chunks, each keeping its original sequence number, so `fcTL` payloads and the animation sequence
  stay byte-identical; a frame that cannot fill its chunks is left untouched. Verification decodes and
  compares every frame, including the frame control data.
- **ICO/CUR.** Embedded PNG images are optimized; bitmap entries are copied byte for byte. The directory is
  parsed strictly: overlapping images, images outside the file, and unreferenced trailing data are refused.
  Only the size and offset fields of moved images change.
- **`--nested`** (off by default) with **`--max-depth`** (default 3): supported files stored inside ZIP
  entries are optimized, verified against their own contract, and used only when the entry gets smaller.
  A failing or unprofitable child keeps the original bytes and never fails the parent archive.
- ZIP verification accepts a changed entry CRC and uncompressed size only for a verified nested rewrite;
  this is the single documented exception to the preservation rule, and it is spelled out in
  `docs/guarantees.md` §4.6.
- 9 integration tests for APNG, ICO/CUR, nested archives and the depth limit; new fixture builders for
  APNG frames and ICO files.

### Changed

- Handlers now take bytes rather than paths, so the pipeline reads each file once and nested processing is
  possible without a second read.
- Measured static binary size: 904 KiB (+36 KiB over v0.2, still no new dependencies).

## [0.2.0] - development milestone, shipped in 0.4.0

### Added

- **ZIP family.** Surgical rewriter: local headers, central directory records, extra fields, names,
  comments, flags, attributes and timestamps are copied and only the compressed size and offset fields are
  patched. Only DEFLATE entries are recompressed, per entry, keeping whichever is smaller; stored entries
  stay stored, so ODF/EPUB `mimetype` requirements hold.
- Data descriptors (bit 3) are validated against the central directory and rewritten with the new size.
- Refused, with the file left untouched: signed archives (JAR/APK signing block, `META-INF` signatures,
  signed ODF `documentsignatures.xml`, `_xmlsignatures/`, OPC digital signatures), ZIP64, multi-disk,
  encrypted archives, unknown compression methods and prepended stubs.
- `--max-uncompressed` (default 8 GiB) and `--max-ratio` (default 1000:1) guard against decompression bombs.
- New error class `refused`: a valid input that policy declines is reported as `skipped` and the run exits
  `0`, while corrupt inputs remain `failed` with exit `1`. The JSON `kind` field gained `refused`.
- 5 unit tests and 14 integration tests for ZIP, with a fixture builder covering deflate, stored entries,
  data descriptors, extra fields, comments and non-UTF-8 names.

### Changed

- gzip: an unsupported compression method and multi-member files are now reported as `refused` (skipped,
  exit `0`) instead of failures.
- Measured static binary size: 868 KiB (+36 KiB over v0.1, no new dependencies).

## [0.1.0] - development milestone, shipped in 0.4.0

### Added

- **PNG handler.** IDAT recompression with `zlib -9`, scanline filter re-selection for non-interlaced,
  non-animated images, optional Zopfli. Every non-IDAT chunk is copied byte for byte, including unknown and
  private chunks. Verification re-parses the candidate (all chunk CRCs) and decodes both files row by row
  with the independent `png` crate.
- **gzip handler.** The header (`FEXTRA`, `FNAME`, `FCOMMENT`, `FHCRC`, `MTIME`, `XFL`, `OS`) is preserved
  verbatim; only the DEFLATE body is rewritten and the trailer is copied after proving it matches.
  Multi-member files and trailing data are refused.
- **Safe replacement.** Temporary file created next to the destination with `create_new`, RAII cleanup,
  permission and timestamp preservation, `fsync`, single `rename`, directory `fsync` on Unix, and detection
  of concurrent modification before the swap.
- **CLI.** `optimize`, `formats`, `--help`, `--version`; multiple files, directories, `--recursive`,
  `--dry-run`, `--jobs`, `--json`, `--quiet`, `--fail-fast`, `--include`, `--exclude`, `--min-size`,
  `--max-size`, `--max-pixels`, `--zopfli`, `--follow-symlinks`, `--allow-hardlinks`,
  `--allow-root-ownership-change`, `--no-fsync`. Exit codes `0`–`4`.
- Deterministic output ordering, de-duplication by `(device, inode)`, symlink and hard-link refusal.
- 41 unit tests and 21 integration tests; CI on Linux, macOS and Windows; clippy `-D warnings`; static musl
  build with a measured size of 832 KiB and 14 dependency lines.

## [Unreleased]

### Added

- Project README with the lossless invariant, guarantee classes and format matrix.
- `docs/guarantees.md`: normative contracts for PNG (v0.1), gzip (v0.1), ZIP (v0.2), ICO, APNG, nested
  containers (v0.3) and JPEG (v0.4), plus the list of properties that are not preserved.
- `docs/formats.md`: detection rules, format matrix, admission criteria and refused formats with reasoning.
- `docs/cli.md`: command-line specification, exit codes, human and JSON output.
- `docs/safety.md`: the file replacement algorithm, temporary-file policy, metadata table, atomicity and
  failure modes.
- `docs/dependencies.md`: four direct dependencies (one optional), rejected alternatives, licence and supply
  chain policy.
- `docs/size-budget.md`: provisional budgets, release profile, feature matrix, measurement procedure.
- `docs/testing.md`: invariants, contract-to-test traceability, fixture corpus, fuzzing and CI plan.
- `docs/roadmap.md`: releases v0.1–v0.5 with gates, estimates and risks.
- `CONTRIBUTING.md` and issue templates.
