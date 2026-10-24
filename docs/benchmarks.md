# Benchmarks

Related: [size budget](size-budget.md) · [testing](testing.md) · [formats](formats.md)

Every number here was produced by CI and can be reproduced from the linked commit. Nothing in this file is
an estimate.

---

## 1. Binary size — measured

| Field | Value |
| --- | --- |
| Commit | `52a33cb` |
| Target | `x86_64-unknown-linux-musl` (static, no libc dependency) |
| Profile | `--release --all-features` (PNG, APNG, gzip, ZIP, ICO/CUR, JPEG, Zopfli) |
| Features in the binary | all |
| **Size** | **1 000 288 bytes (976 KiB)** |
| Normal dependency lines (`cargo tree`, de-duplicated) | 14 |
| Provisional budget for this profile | 2 200 KiB |
| Headroom | 1 224 KiB |

Growth by version, with **no new dependency lines at any step** — four direct crates since v0.1. The JPEG
handler in v0.4 adds code rather than a crate, because no Rust JPEG library exposes DCT coefficients (see
[dependencies](dependencies.md#26-jpeg-why-there-is-no-jpeg-dependency)):

| Version | Contents added | Binary |
| --- | --- | --- |
| v0.1 | PNG, gzip | 832 KiB |
| v0.2 | ZIP family | 868 KiB (+36 KiB) |
| v0.3 | APNG frames, ICO/CUR, nested archives | 904 KiB (+36 KiB) |
| v0.4 | baseline JPEG entropy codec (opt-in) | 976 KiB (+72 KiB) |

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
| Unit tests | 56 |
| Integration tests (real binary, real files) | 54 (52 on Windows, where two tests are Unix-only) |
| Platforms | Linux, macOS, Windows — all green at `52a33cb` |
| Clippy | `-D warnings`, clean |
| Exit codes covered by tests | 0, 1, 2 (4 is covered by a unit test on an injected verification error) |

---

## 3. Compression results

**Not measured yet.** A fair comparison needs the same corpus processed by Tzim, `oxipng`, `optipng`,
`zopflipng` and `gzip -9` on identical hardware, and the development sandbox has neither the tools nor a
Rust toolchain. Publishing a guess here would be worse than publishing nothing.

What can be said from measurements that do exist (stage-1 analysis, Python + zlib, real files shipped by a
Linux distribution):

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
| Ratio per format on a fixed public corpus | needs the external tools in the same environment | v0.2 |
| Wall-clock time, single job and parallel | same | v0.2 |
| Peak memory (RSS) per format | same | v0.2 |
| Regression tracking across releases | needs a stable corpus in CI | v0.2 |
| Windows and macOS binary sizes | only the Linux musl artifact is measured today | v0.2 |

The procedure for closing these gaps is fixed in [size-budget.md](size-budget.md#4-measurement-procedure) and
[testing.md](testing.md#5-benchmarks). Every row above must be filled before v0.2 is tagged.

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
