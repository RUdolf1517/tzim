# Dependencies

Related: [size budget](size-budget.md) · [formats](formats.md)

Tzim exists partly as a reaction to file optimizers with enormous dependency trees. Every dependency is
therefore a deliberate exception, and the default answer to "should we add a crate?" is **no**.

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

| Crate | Direct | Optional | Purpose | In the binary |
| --- | --- | --- | --- | --- |
| `flate2` | yes | no | DEFLATE decode/encode | yes |
| `zopfli` | yes | yes (`--zopfli`, feature `zopfli`) | strongest encoder | only with the feature |
| `crc32fast` | yes | no | checksums | yes |
| `png` | yes | no | independent verification decoder | yes |

Four direct dependencies, one of them optional. That is the entire runtime tree for v0.1.

**Planned addition:** the JPEG handler (v0.4) will need an independent JPEG decoder for verification. No
crate has been selected yet; it will be reviewed and documented here, with its size measured, before any
JPEG code is merged.

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
| `image` | a large umbrella crate; we need one decoder for one format |
| `libc` / `windows-sys` | not needed: `std` exposes permissions, timestamps and rename; platform-specific metadata we do not preserve anyway (see [safety](safety.md#3-metadata)) |
| `log` / `env_logger` | a small stderr printer is enough; logging frameworks are not part of the binary |

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

Every Apache-2.0 dependency requires that its `NOTICE` content, if any, is reproduced in our `THIRD-PARTY`
notice file. `cargo-deny` enforces the allow-list in CI and fails the build otherwise.

---

## 6. Supply chain

- `Cargo.lock` is committed, so releases are reproducible from the lock file. It is generated by the
  `.github/workflows/lockfile.yml` job, which is dispatchable once it exists on the default branch; until
  then CI resolves dependencies without `--locked`. Switching CI to `--locked` is part of the first tagged
  release.
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
decoder used for verification is the one failure this project cannot afford. Four small, audited crates are
a better trade than 3 000 lines of unproven code, provided they are pinned, reviewed and measured.
