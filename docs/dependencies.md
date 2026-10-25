# Dependencies

Related: [size budget](size-budget.md) · [formats](formats.md)

Tzim exists partly as a reaction to file optimizers with enormous dependency trees. Every dependency is
therefore a deliberate exception, and the default answer to "should we add a crate?" is **no**. v0.10 is the
first release since v0.1 to answer yes: one crate, for the progress line, at the user's explicit request
(§2.9).

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
| `indicatif` | yes | always | the progress line | in every build |

Five direct dependencies. Four are behind a feature, which is what makes the feature matrix in
[size-budget §3](size-budget.md#3-feature-matrix) meaningful. `indicatif` is not: the progress line is part
of the interface rather than of a format handler, so every build carries it (§2.9), and a build with
`--no-default-features --features png` compiles `png`, `flate2`, `crc32fast` and the progress stack.

The JPEG, ICO, TIFF and zlib handlers add code, not crates. **v0.4, v0.6, v0.7 and v0.9 added none**:
§2.6 explains JPEG, and zlib costs even less — an Adler-32 is twelve lines of arithmetic next to a codec that
was already in the tree. **v0.10 adds exactly one**, at the user's request and after the argument in §2.9;
that grant is not a precedent, and the next candidate still has to answer every question in §1.

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
  it. The residual risk (a bug shared by encoder and decoder) is recorded in
  [guarantees](guarantees.md#47-jpeg--v04-opt-in) and [benchmarks](benchmarks.md).

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

Progress used to be the second half of this section: v0.8 shipped a hand-rolled line, about 200 lines and a
thread, on the argument that a framework buys bars and terminal-width queries that one `write!` per 100 ms
does not need. That argument held for an ASCII frame and a counter. It stopped holding when the line had to
carry a bar and a spinner that redraw from several worker threads — §2.9 is what changed, and what it cost.

### 2.9 `indicatif` — the progress line

- **Version:** 0.18.x (0.18.6 in the lock) · **Licence:** MIT · **Unconditional** · **Granted v0.10**
- **Why:** the line is now a spinner and a bar, redrawn while several worker threads run. That is three
  problems we were solving badly by hand: a redraw rate shared with the terminal, a cursor that has to be
  restored before the report prints, and the width of glyphs that are not ASCII. `indicatif` is the
  maintained crate for exactly this (it is what `cargo`-adjacent tooling uses), it is small, and it is pure
  Rust at the API level we touch.
- **Configuration:** `default-features = false`. That drops `unicode-width` — a width table for text we never
  truncate, where every glyph in the line (`⠋`, `█`, `░`) is one cell wide — and `wasmbind`, a clock for WASM
  targets that have no meaning for a native binary.
- **What we do not use:** `MultiProgress`, `wide_bar`/`wide_msg`, the iterator adapters, `HumanBytes` (the
  report keeps its own integer formatter — that is the change that saved 20 KiB in v0.7.1,
  [size-budget §5.3](size-budget.md#53-what-is-left-and-why-it-is-not-taken)), and any colour decision:
  the SGR rules stay in `src/style.rs` and reach the spinner as a template string.
- **What stays ours:** the two templates, the `mm:ss` clock (`{t}`, a `ProgressTracker`), the rule that the
  line never appears in a pipe, and the 150 ms delay before the first frame. `src/progress.rs` is the same
  size it was; what it no longer contains is the animation thread and the terminal handling.
- **Transitive additions:** `console` 0.16 (ANSI parsing and the Windows console, `ansi-parsing` and `std`
  only), `portable-atomic`, `unit-prefix`; `console` in turn brings `encode_unicode`, `libc` and, on Windows,
  `windows-sys` with `windows-link`. Eight new lock entries, of which four compile into a Linux binary.
- **`unsafe`:** `console` and `portable-atomic` contain `unsafe` — Windows console calls and fallback atomics.
  Our code still forbids it (`#![forbid(unsafe_code)]`), and the reviewed surface is terminal I/O and a
  counter; nothing there parses file content.
- **Cost:** measured, not estimated — [size-budget §3](size-budget.md#3-feature-matrix) has the per-feature
  table and the delta every row took. It is charged against the budget like any other growth, and the budget
  was not raised in advance.
- **Alternatives rejected:** keeping the hand-rolled line (that is what v0.8 shipped: ASCII frames, no bar,
  no spinner); `pbr` and `progress_bar` (unmaintained); `linya` (less maintained, narrower); `console` alone
  (gives a spinner, not the bar/clock plumbing, so the animation thread would have stayed ours);
  `crossterm`/`termion` (terminal manipulation, raw mode, key reading — none of which this program does).

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
| `jpeg-decoder`, `zune-jpeg` | correct and fast, but they return pixels; re-encoding from pixels would be lossy |
| `image` | a large umbrella crate; we need one decoder for one format |
| `libc` / `windows-sys` | not needed: `std` exposes permissions, timestamps and rename; platform-specific metadata we do not preserve anyway (see [safety](safety.md#3-metadata)) |
| `log` / `env_logger` | a small stderr printer is enough; logging frameworks are not part of the binary |
| `owo-colors`, `colored`, `anstream`, `termcolor` | five SGR sequences and a `bool` are the whole feature; the decision logic is 60 lines of tested code, and a crate costs more bytes than the strings it hides (§2.8) |
| `crossterm`, `termion` | raw mode, key events, terminal resize: none of it is needed. The one terminal problem this program has is a redrawn line, and `indicatif` (§2.9) is that answer alone |
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
