# Dependencies

Related: [size budget](size-budget.md) · [formats](formats.md)

Tzim exists partly as a reaction to file optimizers with enormous dependency trees. Every dependency is
therefore a deliberate exception, and the default answer to "should we add a crate?" is **no**. Two
exceptions have been granted and both are on this page: `indicatif`, added in v0.10 for the progress line and
**removed again in v0.13** (§2.9), and `jpeg-decoder`, added in v0.13 to put a decoder that shares no code
with this repository on the far side of the JPEG comparison (§2.10).

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
| `flate2` | yes | features `png`, `gzip`, `zip`, `tiff`, `zlib` | DEFLATE decode/encode | with any of those |
| `crc32fast` | yes | features `png`, `gzip`, `zip` | checksums | with any of those |
| `png` | yes | feature `png` (and `ico`, which requires it) | independent verification decoder | with `png` |
| `zopfli` | yes | feature `zopfli` | strongest encoder | only with `zopfli` |
| `jpeg-decoder` | yes | feature `jpeg-v1` (in `default`) | second opinion on a JPEG candidate | with `jpeg-v1` |

Five direct dependencies, and every one of them is behind a feature: a build with
`--no-default-features --features png` compiles `png`, `flate2` and `crc32fast`, and nothing else. There is no
unconditional crate any more — the progress line, which used to be one (§2.9), is drawn by
`src/progress.rs` again.

The JPEG, ICO, TIFF and zlib handlers add code, not crates. **v0.4, v0.6, v0.7 and v0.9 added none**:
§2.6 explains JPEG, and zlib costs even less — an Adler-32 is twelve lines of arithmetic next to a codec that
was already in the tree. **v0.10 added one** for the progress line and **v0.13 removed it** (§2.9);
**v0.13 added `jpeg-decoder`** for the `jpeg-v1` feature after the argument in §2.10. Each grant is argued on
its own, and the next candidate still has to answer every question in §1.

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

### 2.7 FLAC: the crates it would have needed

**FLAC is refused** ([flac-audit §7](flac-audit.md#7-decision-refused-and-the-measurement-behind-it)): a
stronger encoder buys 0.4 % on musical material and 0 % on files already written at `flac -8`, while the
encoder and an independent decoder together would add roughly 150–300 KiB to a 920 KiB binary. The table below
is kept as the record of what the "yes" would have cost, and of why the encoder and the decoder cannot be the
same crate.

| Crate | Version at audit | Licence | Size | Role | Why it is the right one |
| --- | --- | --- | --- | --- | --- |
| `flacenc` | 0.5.1 | Apache-2.0 | ~10 k lines | encoder | the only maintained pure-Rust FLAC encoder; no C, no `build.rs` beyond `built`; with `default-features = false` the `log`, `serde` and `crossbeam-channel` features are dropped, which leaves `crc`, `heapless`, `md-5`, `num-traits`, `rustversion`, `seq-macro` |
| `claxon` | 0.4.3 | Apache-2.0 | ~2.2 k lines | verification decoder | a different implementation by a different author, with no dependencies of its own — the independence requirement of [guarantees §2.6](guarantees.md#26-verification-independence) |
| `md-5` | 0.10.6 | MIT OR Apache-2.0 | ~400 lines | stream MD5 | RustCrypto; already a dependency of `flacenc`, so it adds no crate to the tree |

`flacenc` can decode as well, and using its own decoder for verification is explicitly rejected: the encoder
would be confirming itself, which is the failure mode the independence rule exists to prevent. The same
reasoning rejects `flacenc`'s `decode` feature even as a convenience.

Binary cost is unmeasured, and the budget will not be raised in advance: the feature matrix in
[size-budget §3](size-budget.md#3-feature-matrix) is where the number will appear.

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
measurement is [size-budget §1](size-budget.md#1-current-numbers) — and the progress line is about 250 lines
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

### 2.10 `jpeg-decoder` — the second opinion on a JPEG candidate

- **Version:** 0.3 · **Licence:** MIT · **Feature:** `jpeg-v1`, on in `default` · **Granted v0.13** (issue
  #39, owner decision "option 3")
- **Why:** the JPEG comparison had one author on both sides. `verify` compares coefficients, marker structure
  and bytes, all through code in this repository; a defect symmetric in the encoder and the decoder — a
  wrong DC prediction direction that both agree on — would be invisible. `jpeg-decoder` is an independent
  implementation: it shares no line with `src/formats/jpeg.rs`, so its pixels and ours disagreeing is
  evidence one of us has a bug.
- **Configuration:** `default-features = false` with `platform_independent`. That drops `rayon` (this
  repository has no async runtime and no thread pool in a handler), `nasm`/SIMD assembly paths, and the
  platform-specific intrinisics that would put `unsafe` and a second build toolchain in the tree. The
  remaining code is pure Rust — the crate's own `unsafe` is in the paths we do not compile — and the
  `forbid(unsafe_code)` lint on our crate is untouched.
- **What it is not for:** it cannot replace the handler. It returns pixels, and re-encoding from pixels is
  the lossy transformation the invariant forbids (§2.6). It is a *verifier*, called once per accepted
  candidate, on the two byte buffers that verification already has in memory.
- **Cost:** the pixels of one side at a time are hashed with CRC-32 and dropped, so verification is not
  charged two extra decoded images; the memory accounting in [safety §7.1](safety.md#71-memory--an-enforced-budget)
  says `7 × pixels` per side with that in mind. The binary cost is measured, not estimated, and it is the
  reason the feature is opt-out rather than opt-in: the release gate is the *previous* release's size
  (1 049 440 bytes), and this crate has to fit inside what `indicatif` gave back.
- **What it changes for a user:** a file whose candidate disagrees with an independent decoder is `failed`,
  exit 1, original untouched — a class of defect that previously would have been reported as `optimized`. If
  the independent decoder refuses the *original*, the file is `skipped` with that reason: the oracle cannot
  be used as a verdict on a file it cannot read, and a JPEG this program understands but `jpeg-decoder` does
  not is not a reason to fail a run.
- **Turning it off:** `--no-default-features --features png,gzip,zip,ico,jpeg,tiff,zlib` builds exactly the
  binary of 0.12.1, and `docs/guarantees.md#26-the-verification-method-per-format` says so in the JPEG row.

---

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
