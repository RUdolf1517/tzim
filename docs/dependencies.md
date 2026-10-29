# Dependencies

Related: [size budget](size-budget.md) · [formats](formats.md)

Tzim exists partly as a reaction to file optimizers with enormous dependency trees. Every dependency is
therefore a deliberate exception, and the default answer to "should we add a crate?" is **no**. `indicatif`,
added in v0.10 for the progress line, was **removed in v0.13** (§2.9). `jpeg-decoder` is the independent JPEG
verifier (§2.10). The owner later authorized the non-default #79 FLAC feature, which adds `flacenc`,
`claxon`, and `md-5`; these optional dependencies are present only when that feature is selected.

The project targets **Rust 1.88** (edition 2021). The floor is set by the optional Zopfli encoder; the PNG
decoder requires 1.81 and `flate2` requires 1.67. Keeping the floor at 1.88 means one toolchain for
everything, including the optional feature.

---

## 1. Decision procedure

Before adding any dependency, answer these questions in the pull request. "Convenience" is not an answer.

1. Does it implement functionality that is **mandatory** for a supported format?
2. Can the standard library do the same thing in a reasonable amount of safe code?
3. Can an already-present dependency do it?
4. What does it cost in binary size, compile time, and supply-chain surface?
5. Is it maintained, and is its licence compatible?
6. Does it duplicate something we already have?

Additional requirements:

- default features disabled unless a default feature is genuinely required;
- no `build.rs` that downloads, generates code from the network, or compiles C (the whole build must work
  offline with `--offline` and without a C compiler);
- no `unsafe` in *our* code; `unsafe` in a dependency must be reviewed and justified;
- the binary-size delta measured and recorded in [size-budget.md](size-budget.md).

---

## 2. Direct dependencies

### 2.1 `flate2` — DEFLATE, zlib and gzip streams

- **Version:** 1.1.x · **Licence:** MIT OR Apache-2.0
- **Why:** DEFLATE is the payload format of PNG, gzip and ZIP. Implementing it by hand would be both slow and
  a correctness risk, and the specification has many edge cases (dynamic Huffman blocks, distance codes,
  malformed streams). A proven decoder is mandatory.
- **Configuration:** `default-features = false, features = ["rust_backend"]`.
  That selects `miniz_oxide`, a pure-Rust implementation: no C compiler and no `libz` linkage, so static
  cross-compilation for musl, Windows and macOS works out of the box.
- **Alternatives rejected:** `libz-sys` (C, links a system or vendored zlib), `libz-ng-sys` (C),
  `zlib-rs` (pure Rust but uses `unsafe` internally and is faster than needed for our decode-bound work;
  it remains available as an opt-in feature if profiling ever justifies it).
- **Used by:** PNG (IDAT), gzip (body), ZIP (entry data), and our verification decoders.
- **Size:** the largest single contributor. Measured per release (see size budget).

### 2.2 `zopfli` — strongest DEFLATE encoder

- **Version:** 0.8.x · **Licence:** Apache-2.0 · **Optional**
- **Why:** it is the only mature pure-Rust implementation of Zopfli, which typically beats `zlib -9` by a few
  percent. It is the difference between "good enough" and "smallest possible" for PNG and gzip.
- **Configuration:** `default-features = false, features = ["std"]`, using only `Format::Deflate` (raw
  DEFLATE). The gzip and zlib wrappers of the crate are not used — we build those containers ourselves so
  that headers can be preserved byte-for-byte.
- **Cost:** 10–100× CPU time compared with `zlib -9`, and a noticeable amount of memory per job. This is why
  it is opt-in (`--zopfli`) and why `--jobs` defaults to a lower value when it is enabled.
- **Alternatives rejected:** `libdeflater` (C, excellent speed and ratio, but a C toolchain in the build and
  a licence/portability review); the C `zopfli` via FFI (same problem, worse).
- **Used by:** PNG, gzip and ZIP when `--zopfli` is given.

### 2.3 `crc32fast` — CRC-32 checksums

- **Version:** 1.5.x · **Licence:** MIT OR Apache-2.0
- **Why:** PNG chunks, the gzip trailer and ZIP entries are all protected by CRC-32, and we must both
  validate incoming checksums and produce valid ones.
- **Note:** it is already in the tree transitively through `flate2` and `png`, so declaring it directly adds
  no new code to review and no meaningful binary cost.
- **Alternatives rejected:** hand-rolled table CRC (about 20 lines, but a wrong implementation would silently
  produce invalid files, and SIMD-accelerated crc32fast is faster on the large payloads we checksum).

### 2.4 `png` — PNG decoder (verification only)

- **Version:** 0.18.x · **Licence:** MIT OR Apache-2.0
- **Why:** verification must not be performed by the code that wrote the candidate. The `png` crate is a
  battle-tested, fuzzed, pure-Rust decoder maintained by the image-rs project, and it is used **only to read
  and validate** files — never to write them, because its encoder cannot reproduce arbitrary ancillary
  chunks and their order.
- **Configuration:** `default-features = false` (no `unstable`, no `zlib-rs`).
- **Used by:** PNG/APNG/ICO verification; decoding both input and candidate and comparing the bytes.
- **Transitive additions:** `bitflags`, `crc32fast`, `fdeflate`, `flate2` — all small and pure Rust.
  `fdeflate` is used by the encoder path and is expected to be largely eliminated by LTO in our build.

### 2.5 Summary

| Crate | Direct | Compiled by | Purpose | In the binary |
| --- | --- | --- | --- | --- |
| `flate2` | yes | features `png`, `gzip`, `zip`, `tiff`, `zlib`, `fits` | DEFLATE decode/encode | with any of those |
| `crc32fast` | yes | features `png`, `gzip`, `zip`, `fits` | checksums | with any of those |
| `png` | yes | feature `png` (and `ico`, which requires it) | independent verification decoder | with `png` |
| `zopfli` | yes | feature `zopfli` | strongest encoder | only with `zopfli` |
| `jpeg-decoder` | yes | `jpeg-v1` (included in `default` under #82) | independent pixel verification of JPEG candidates | release default and `jpeg-v1` builds |
| `flacenc` | yes | non-default feature `flac` | FLAC frame encoder | only with `flac` |
| `claxon` | yes | non-default feature `flac` | independent FLAC decoder/PCM verifier | only with `flac` |
| `md-5` | yes | non-default feature `flac` | STREAMINFO MD5 over decoded PCM | only with `flac` |

Every direct dependency remains behind a feature: a build with
`--no-default-features --features png` compiles `png`, `flate2` and `crc32fast`, and nothing else. There is no
unconditional crate any more — the progress line, which used to be one (§2.9), is drawn by
`src/progress.rs` again. Under #82 the release default includes `jpeg-v1`; only deliberately narrowed builds
without that feature omit the independent JPEG verifier.

The JPEG, ICO, TIFF, zlib, PDF and FITS handlers add no format-specific dependency; PDF uses the existing
`flate2` zlib path, and FITS uses `flate2` plus `crc32fast`. Historically, **v0.4, v0.6, v0.7, v0.9 and
v0.14 added none**. The unreleased #84 FLAC follow-up adds the three crates in §2.7, all behind `flac` and
outside `default`. **v0.10 added `indicatif`** for the progress line and **v0.13 removed it** (§2.9);
**v0.13 added `jpeg-decoder`** for the `jpeg-v1` feature after the argument in §2.10. Each grant is argued on
its own, and no new dependency is enabled in the default build.

### 2.6 JPEG: why there is no JPEG dependency

The v0.1 plan said the JPEG handler would need an independent decoder. That assumption turned out to be
wrong, and the reasoning matters because this is the one place where the project writes a codec by hand
instead of using a vetted crate.

- **Pixel decoders cannot do this job.** `jpeg-decoder`, `zune-jpeg` and `image` return pixels. The
  transformation is defined on *DCT coefficients*; going through pixels means an IDCT followed by an FDCT,
  which is lossy and forbidden by the invariant. A decoder that exposes coefficients is required, not
  merely convenient.
- **The crates that do expose coefficients are C libraries.** `mozjpeg-sys` and `turbojpeg` wrap
  libjpeg-turbo and do export `jpeg_read_coefficients` / `jpeg_write_coefficients`. Using them would mean
  (a) `unsafe` FFI, which `[lints.rust] unsafe_code = "forbid"` rejects; (b) a C toolchain on Linux, macOS
  and Windows plus the musl cross-build used for the size measurement; (c) a build that is no longer
  offline-and-`--offline`; (d) several hundred KiB of C object code in a binary with a 2 200 KiB budget.
  Each of those contradicts a rule in §1.
- **What is left is a bounded, testable amount of code.** The handler is a marker parser, a canonical
  Huffman decoder, an encoder with the same table construction the standard specifies in Annex K.2, and a
  coefficient-comparison verifier. It is roughly 1 100 lines of safe Rust with no `unsafe`, and every table
  it generates is validated before use (no overfull space, no duplicate symbols, no all-ones code), then
  exercised by a 4 000-case property test.
- **Verification is stronger, not weaker.** Because both encode and decode live here, the verifier can
  compare coefficients rather than pixels — a condition that implies pixel identity but is not implied by
  it. That is also the residual risk: a defect symmetric in the two would be invisible. v0.13 answers it
  where it can be answered cheaply, with a decoder nobody in this repository wrote: `jpeg-decoder` decodes
  both sides of an accepted candidate and the decoded pixels must be identical (§2.10). The pixel path is
  therefore used for *verification*, never for the transformation — the distinction the first bullet above
  turns on, and the reason v0.4 needed no crate even though v0.13 uses one.

### 2.7 FLAC: the opt-in dependency set authorized by #79

The historical audit measured 0.4 % on musical material and 0 % on files already written at `flac -8`, while
the encoder and independent decoder were estimated to add 150–300 KiB
([flac-audit §7](flac-audit.md#7-decision-refused-and-the-measurement-behind-it)). Those values remain
historical, not current size/speed evidence. The owner authorized #79 despite that result. The unreleased #84
implementation pins the reviewed versions below behind `flac`; none is enabled in `default`.

| Crate | Locked version | Licence | Role | Selected configuration and dependency effect |
| --- | --- | --- | --- | --- |
| `flacenc` | 0.5.1 | Apache-2.0 | frame encoder | `default-features = false`; no `log`, `serde`, or `crossbeam-channel`. Mandatory dependencies are `crc`, `heapless`, `md-5`, `num-traits`, `rustversion`, `seq-macro`; its `built` build dependency emits compile-time package metadata only. |
| `claxon` | 0.4.3 | Apache-2.0 | independent decoder and PCM verifier | Different implementation/author from the writer; no runtime dependencies of its own. |
| `md-5` | 0.10.6 | MIT OR Apache-2.0 | STREAMINFO MD5 over decoded PCM | Also required by `flacenc`; shared rather than duplicated in the dependency graph. |

`flacenc` can decode as well, but its decoder is not used for verification: the writer must not confirm
itself. The independent Claxon path compares decoded PCM and checks a nonzero input STREAMINFO MD5; the
handler also validates frame sequence/statistics and retained metadata. The features and lock graph are present
in the branch, but no local Rust compiler is available, so Cargo has not validated the lockfile or built the
code yet. CI must verify compilation/tests and `cargo deny` before these dependencies are accepted.

Binary cost and current size/speed/gain are unmeasured; the historical estimate is not a result. The feature
matrix in [size-budget §3](size-budget.md#3-feature-matrix) is where a measured number will appear.

---

### 2.8 Colour: why it is not a dependency

Colour is six byte strings (`\x1b[1m`, `\x1b[2m`, `\x1b[31m`, `\x1b[32m`, `\x1b[33m`, `\x1b[0m`) and one
`bool`. What is worth getting right is *when* to emit them — the terminal check, `NO_COLOR`, `CLICOLOR`,
`CLICOLOR_FORCE`, `TERM=dumb`, the Windows console question — and that is a pure function of a probe struct,
tested without a terminal (`src/style.rs`). Detection uses `std::io::IsTerminal`, stable since 1.70, which is
why `atty` and `is-terminal` are unnecessary.

Progress is the second half of this section again. v0.8 shipped a hand-rolled line — about 200 lines and a
thread — on the argument that a framework buys bars and terminal-width queries that one `write!` per 100 ms
does not need. v0.10 gave up on that argument when the line grew a bar, a spinner, a rate and an ETA, and
took `indicatif`. v0.13 gives the argument back: the line is drawn here again, and §2.9 is the account of
both decisions, because a dependency removed is worth as much documentation as one added.

### 2.9 `indicatif` — added in v0.10, removed in v0.13

**Removed.** `indicatif` and its `console` dependency cost **96 KiB** of a 1 049 440-byte binary — the
measurement is [size-budget §1](size-budget.md#1-budget) — and the progress line is about 250 lines
of code either way. [issue #53](https://github.com/RUdolf1517/tzim/issues/53) asked for the line to be drawn here again, with one condition: the result has
to be *prettier* than the `indicatif` line, not plainer. It is:

```
⠹ 01:12 [████████▏···]  37/148  6.2/s  eta 00:18
```

What the crate did that had to be rebuilt: one write per frame (100 ms cap) from a single ticker thread,
padded to the previous width so shrinking text does not leave debris, erased on drop and before the report,
`COLUMNS` for the width with a sane fallback, an ETA that waits for three seconds and two completions so it
cannot start out absurd, and text that is clipped by *display* width rather than byte count — the wcwidth
table lives in `src/report.rs` and is shared with the report, so the two cannot disagree about a CJK name.
The first frame still waits 150 ms, and nothing is drawn when stderr is not a terminal.

The lesson recorded, because it is the reason this section exists: a crate that renders a bar was 9 % of the
binary, and the argument that justified it ("a bar, a spinner and a rate are three problems") was answered by
250 lines of arithmetic that also fixed the two defects the crate had left — an ETA that appeared after one
sample and a frame that could be wider than the terminal.

- **History:** version 0.18.x, MIT, unconditional, granted v0.10 by the user after the argument above.
  `default-features = false` dropped `unicode-width` and `wasmbind`. Transitive additions were `console` 0.16
  (ANSI parsing and the Windows console), `portable-atomic`, `unit-prefix`, and through `console`:
  `encode_unicode`, `libc`, `windows-sys`, `windows-link`. Eight lock entries, five of them compiling into a
  Linux binary, and `unsafe` in two of the crates.
- **Alternatives to a hand-drawn line, still rejected:** `pbr` and `progress_bar` (unmaintained);
  `linya` (less maintained, narrower); `console` alone (a spinner, not the bar/clock plumbing, so the
  animation thread would have stayed ours); `crossterm`/`termion` (terminal manipulation, raw mode, key
  reading — none of which this program does).

### 2.10 `jpeg-decoder` — the independent pixel check for JPEG

- **Version:** 0.3 · **Licence:** MIT · **Feature:** `jpeg-v1`, included in the release default under #82.
  The owner-authorized stripped `x86_64-unknown-linux-musl` ceiling is fixed at **1,082,208 bytes**; the
  final-SHA run, not this dependency review, supplies the acceptance measurement.
- **Why:** coefficient, marker and byte comparisons in `verify` are implemented by this repository. A defect
  symmetric in the encoder and decoder could otherwise pass. `jpeg-decoder` shares no code with
  `src/formats/jpeg.rs`, so differing pixel digests expose a class of such defects.
- **Configuration:** `default-features = false` with `platform_independent`. That drops `rayon`, SIMD assembly
  paths and platform-specific intrinsics; the selected path is pure Rust and does not add `unsafe` or a second
  build toolchain. `Cargo.toml` applies the measured dependency-specific `opt-level = "s"` override while the
  rest of the codecs retain their existing release profile.
- **Role:** this crate cannot replace the handler. It returns pixels; re-encoding from pixels would be lossy.
  It is called only to verify the original and candidate byte buffers already held by the pipeline.
- **Memory:** each side is decoded, CRC-32 hashed and dropped before decoding the other. The exact local
  coefficient-plane results are dropped after comparison and before the independent decoder allocates its
  pixel frame; no pair of independent full-frame buffers overlaps. The existing #51 accounting charges
  `7 × pixels` ([safety §7.1](safety.md#71-memory--an-enforced-budget)).
- **Runtime outcome:** a candidate disagreement is `failed`, exit 1, original untouched. If the independent
  decoder refuses the original, the file is `skipped` with that reason; the oracle cannot give a verdict on a
  file it cannot read. Builds that deliberately omit `jpeg-v1` retain the coefficient/structure check only.
- **Additional evidence:** the `third-party` job independently decodes the real-file corpus with Pillow before
  and after, and includes a negative control ([testing](testing.md#the-third-party-decoder-job)).

---

### 2.11 FITS: no CFITSIO, and why that is not a gap

The obvious dependency for a FITS handler is CFITSIO, through a binding crate such as `fitsio`. It is refused
on two counts from §1: it is C, so the static cross-platform build stops being static, and it would be the same
implementation on both sides of the comparison — a candidate checked by the writer's own library is exactly
the symmetry [guarantees §2.6](../docs/guarantees.md#26-verification-independence) calls a hole.

What the transformation actually needs is a gzip member (§2.1), an inflater and a CRC-32, so `fits` is
`fits = ["dep:flate2", "dep:crc32fast"]` and nothing else. The CRC crate is the verifier's: the writer's
framing comes from `flate2`, and the candidate's trailer is recomputed with an implementation it never calls.
The second implementation lives outside the binary, in `.github/scripts/fits-oracle.py`, where Astropy's own
`astropy.io.fits` reader reads the GZIP_1/GZIP_2 candidates. Those paths use Python's `gzip` and Astropy's own
GZIP2 byte shuffle; they do not call CFITSIO's `funpack`. Astropy has CFITSIO-derived C routines for other
codecs, but tzim does not emit those codecs and this oracle does not exercise them. The separate HEASARC gate
runs the actual CFITSIO `fitsverify` and `funpack -C` tools. The Astropy reader costs the binary nothing and the
CI job one `pip install`.

## 3. Rejected dependencies

| Candidate | Why not |
| --- | --- |
| `clap` | a 200-line argument parser covers the whole CLI; proc-macro and builder machinery cost hundreds of KiB |
| `serde` + `serde_json` | NDJSON output needs an escaping routine of about 40 lines, which we test directly |
| `rayon` | `std::thread::scope` plus a bounded channel gives the same result for a work queue of files |
| `tempfile` | we need a specific naming scheme and a specific directory; about 60 lines, fully testable |
| `walkdir` | directory traversal with our own symlink and dedup policy is under 100 lines and removes a policy mismatch |
| `anyhow` / `thiserror` | one `enum Error` with `Display`; error chains are not part of our public interface |
| `zip` | its writer does not guarantee preservation of every extra field, the archive order, or a prepended prefix; our contract requires byte-level fidelity (see [formats](formats.md#5-refused-formats)) |
| `oxipng` | pulls `libdeflater` (C), `indexmap`, `rgb`, `rustc-hash`, `log`; and verifying oxipng with oxipng would defeat the independence requirement |
| `mozjpeg-sys`, `turbojpeg` | C codecs behind `unsafe` FFI; they do expose DCT coefficients, but they break the no-`unsafe` rule, need a C toolchain on three platforms plus musl, and add hundreds of KiB (see §2.6) |
| `zune-jpeg` | returns pixels; re-encoding from pixels would be lossy (§2.6). `jpeg-decoder` was accepted for *verification* only, under the `jpeg-v1` feature (§2.10) |
| `image` | a large umbrella crate; we need one decoder for one format |
| `libc` / `windows-sys` | not needed: `std` exposes permissions, timestamps and rename; platform-specific metadata we do not preserve anyway (see [safety](safety.md#3-metadata)) |
| `log` / `env_logger` | a small stderr printer is enough; logging frameworks are not part of the binary |
| `owo-colors`, `colored`, `anstream`, `termcolor` | five SGR sequences and a `bool` are the whole feature; the decision logic is 60 lines of tested code, and a crate costs more bytes than the strings it hides (§2.8) |
| `crossterm`, `termion` | raw mode, key events, terminal resize: none of it is needed. The one terminal problem this program has is a redrawn line, and since v0.13 that line is `src/progress.rs` (§2.9) |
| `atty` / `is-terminal` | `std::io::IsTerminal` has been stable since Rust 1.70, and the floor here is 1.88 |

---

## 4. Development-only dependencies

These never appear in a release binary:

| Crate / tool | Purpose |
| --- | --- |
| `proptest` | property tests: filters round-trip, "output never larger", "verification always passes" |
| `libfuzzer-sys` (in `fuzz/`) | `cargo-fuzz` targets for the PNG, gzip and ZIP parsers |
| `cargo-bloat` | per-crate size attribution when tuning the binary |
| `cargo-deny` | licence, duplicate and advisory checks in CI |
| `cargo-audit` | RustSec advisory database check in CI |

The fuzz crate lives in its own workspace member and is built only with nightly; `cargo build` never touches
it.

---

## 5. Licence policy

Allowed: **MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0**, and MPL-2.0 with a
file-level notice after review.

Denied: GPL, AGPL, LGPL (static linking would require a relinking exception), SSPL, BUSL, and anything
without a licence.

Every Apache-2.0 dependency requires that its `NOTICE` content, if any, is reproduced in
[`THIRD-PARTY.md`](../THIRD-PARTY.md), which lists every crate in the tree and its licence. `cargo-deny`
enforces the allow-list in CI (`deny.toml`) and fails the build otherwise.

---

## 6. Supply chain

- `Cargo.lock` is committed, and every build in CI runs with `--locked`: a build cannot silently resolve a
  different set of versions than the one that was reviewed. The lock is written by the
  `.github/workflows/lockfile.yml` job, which regenerates it on a push to any branch and commits the result
  back to that branch. That is not a convenience: the development environment for this project has no Rust
  toolchain, so CI is the only writer of this file, and a pull request that adds a dependency would otherwise
  arrive with a lock that `--locked` rejects.
- Every dependency update is a pull request that records the new binary size and passes the full test suite.
- CI runs `cargo deny check` (licences, duplicate versions, advisories) and `cargo audit`.
- Dependencies with a `build.rs` are reviewed manually; none may perform network access or compile C.
- Yanked or unmaintained crates are replaced rather than patched locally.
- `unsafe` in our code is forbidden (`#![forbid(unsafe_code)]`); `unsafe` in dependencies is documented in
  review.

---

## 7. Why not "zero dependencies"?

A pure-`std` Tzim would have to implement DEFLATE, CRC-32 and a PNG decoder by hand. Those are exactly the
components where a hand-written implementation is most likely to be subtly wrong — and a subtle bug in a
decoder used for verification is the one failure this project cannot afford. Five small, audited crates are
a better trade than 3 000 lines of unproven code, provided they are pinned, reviewed and measured.
