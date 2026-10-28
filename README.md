<div align="center">

# tzim

**צמצום** — *tsimtsum*: contraction, making space

**Shrink the file, never the information.**

[![ci](https://github.com/RUdolf1517/tzim/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/RUdolf1517/tzim/actions/workflows/ci.yml)
[![release](https://img.shields.io/github/v/release/RUdolf1517/tzim?style=flat-square&color=0b3d91)](https://github.com/RUdolf1517/tzim/releases/latest)
[![binary](https://img.shields.io/badge/static%20binary-1024%20KiB-111111?style=flat-square)](docs/size-budget.md)
[![license](https://img.shields.io/badge/license-MIT-0b3d91?style=flat-square)](LICENSE)

[Install](#install) · [Usage](#usage) · [Formats](#formats) · [Why tzim](#why-tzim) · [Compare](#choose-the-right-tool) · [Guarantees](#guarantees) · [Docs](#documentation)

</div>

---

Tzim makes a file smaller **or leaves it alone**. Every supported format has a written preservation contract:
a candidate replaces the original only when it is strictly smaller and passes that contract's checks. If a
check fails or the format is outside the supported subset, the original stays byte-for-byte untouched and
Tzim says why. Verification never reuses the encoder; decoder independence and the remaining blind spots are
[stated per format](docs/guarantees.md#26-verification-independence), not hidden behind a blanket claim.
Metadata is removed only if you type `--strip-metadata`. There is no quality setting, no "visually lossless"
mode and no automatic fallback to a lossy path, because none of them are in the program.

```bash
$ tzim ./photos -r
photos/beach.png    PNG    1.24 MiB -> 1.11 MiB  -10.5%
photos/scan.tiff    TIFF   8.90 MiB -> 4.02 MiB  -54.8%
photos/cat.jpg      JPEG   already optimal
Summary: 12 files, 9 optimized, 5.24 MiB saved, 3 already optimal, 0 unsupported, 0 skipped, 0 failed, 1.71 s
```

The three files marked *already optimal* are the point as much as the nine that shrank: they were read,
tried, and left byte-for-byte untouched.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/RUdolf1517/tzim/main/install.sh | sh   # binary, no Rust
brew tap RUdolf1517/tzim https://github.com/RUdolf1517/tzim && brew install RUdolf1517/tzim/tzim
cargo install --git https://github.com/RUdolf1517/tzim                               # Rust 1.88+
```

Linux (x86_64 and ARM64), macOS (Apple silicon and Intel). The install script checks the archive against the
release's `SHA256SUMS` and refuses to install anything that does not match — and so should you, if you install
by hand.

<details>
<summary>Debian, Fedora, Windows</summary>

```bash
# Debian / Ubuntu — tzim_arm64.deb on ARM
curl -fsSL -O https://github.com/RUdolf1517/tzim/releases/latest/download/tzim_amd64.deb
sudo apt install ./tzim_amd64.deb

# Fedora / RHEL — tzim.aarch64.rpm on ARM
curl -fsSL -O https://github.com/RUdolf1517/tzim/releases/latest/download/tzim.x86_64.rpm
sudo dnf install ./tzim.x86_64.rpm
```

Both packages install `man tzim` and completion for bash, zsh and fish.

Windows, x64 — a zip on the releases page:

```powershell
Invoke-WebRequest -Uri https://github.com/RUdolf1517/tzim/releases/latest/download/tzim-x86_64-pc-windows-msvc.zip -OutFile tzim.zip
Expand-Archive tzim.zip -DestinationPath $HOME\tzim
```

Then put `tzim.exe` on `PATH`. There is no package manager on Windows: the zip is the channel, and nothing is
written outside the directory you unpack it into.

</details>

The binary is static, contains no C code, needs no runtime, writes no configuration file, opens no socket and
sends no telemetry. `tzim --check-deps` prints exactly that. The packages and the Homebrew formula install a
manual page and shell completion for bash, zsh and fish, so `man tzim` works after an install.

## Usage

```bash
tzim image.png                  # one file
tzim a.png b.zip c.jpg          # several, mixed kinds in one run
tzim ./files -r                 # a whole tree
tzim ./files -r --dry-run       # do everything except the final rename
tzim image.png --zopfli         # best ratio, 10-100x slower
tzim . -r --json                # one JSON object per file
```

The path is the command: there is no subcommand and no output file to name. A single file is a first-class
input, and files, directories and duplicates mix freely in one invocation.

| Exit | Meaning |
| --- | --- |
| `0` | Success — including files that were already optimal, skipped or refused |
| `1` | At least one file failed; the others were still processed |
| `2` | Invalid usage; nothing was processed |
| `3` | Fatal error; the run could not be completed |
| `4` | **Verification failure** — a candidate broke its contract. The original was not replaced, and this means a bug |

Full flag reference: [docs/cli.md](docs/cli.md).

## Formats

| Format | Since | Transformation | Guarantee |
| --- | --- | --- | --- |
| PNG | v0.1 | Recompress IDAT; re-select scanline filters; optional Zopfli | G3 |
| gzip (`.gz`, `.tgz`, `.svgz`) | v0.1 | Recompress the DEFLATE body; header preserved verbatim | G3 |
| ZIP family (`.zip`, `.jar`, `.apk`, `.docx`, `.epub`, `.whl`, …) | v0.2 | Recompress DEFLATE entries; names, extra fields, comments, order and attributes preserved; signed, ZIP64 and encrypted archives refused | G3 |
| ICO / CUR | v0.3 | Optimize embedded PNGs; bitmap entries copied verbatim | G3 |
| APNG | v0.3 | Recompress every frame, keeping chunk count and sequence numbers | G3 |
| Nested containers | v0.3 | Optimize supported files inside ZIP archives (`--nested`) | G3 |
| JPEG | v0.4 / #82 | Entropy re-encoding with optimal Huffman tables; exact coefficient comparison plus independent V1 pixel verification; baseline sequential only, enabled by default | G3 |
| TIFF (classic, strips) | v0.7 | Recompress strips; every tag, entry order and geometry preserved; BigTIFF, tiling and LZW refused | G3 |
| zlib (`.zlib`, `.zz`) | v0.9 | Recompress the DEFLATE stream; header and Adler-32 preserved byte for byte | G3 |
| FLAC | #79 | **Opt-in implementation authorized, not implemented**: the historical audit measured 0.4 % on music for ~150–300 KiB of encoder/decoder cost; no FLAC handler ships until its independent-PCM contract and evidence pass ([audit](docs/flac-audit.md)) | — |
| FITS | v0.14 (not in the default build) | Tile-compress an uncompressed image HDU into a `BINTABLE` (`GZIP_1` for integers, `GZIP_2` for floats, quantisation off); every other HDU copied verbatim; opt-in via `--fits-tile` (a single-file TTY run asks default No after verification); the `fits` feature is off by default | G3 |
| PDF | #57 / #80 | The historical 2,208-file audit reported a 0.08 % median, but the required separate 20–50-file real-toolchain sample and current disposition are pending; no PDF handler is admitted. Repacking the object graph remains forbidden ([contract](docs/guarantees.md#411-pdf--refused-after-measurement) · [audit](docs/pdf-audit.md)) | — |
| WebP, AVIF/HEIC, MP4/MKV, MP3, GIF, xz/zstd/7z | — | **Not planned** ([why](docs/formats.md#5-refused-formats)) | — |

A format is listed as supported only when its contract and its tests exist. The release default processes only
eligible baseline JPEGs with exact coefficient comparison and independent V1 pixel verification. `--formats`
remains an allowlist for narrowing a mixed run; use `--formats png` to exclude JPEG.

FITS is the first format with **two** switches, and both are off. `--fits-tile` is the transformation: without
it the handler only says why it did nothing, because turning an `IMAGE` HDU into a `BINTABLE` is visible to
every reader that indexes `NAXIS1`. The `fits` cargo feature is the code: it is not in `default`, so a build
that wants FITS is `cargo build --release --features fits`, and [size-budget §3](docs/size-budget.md#3-feature-matrix)
measures what it costs: **+36 KiB**, which is enough to break the byte-exact gate on the default build. `--list-formats` names FITS only when the feature is compiled in.
FITS is an opt-in implementation, not full format admission or a stability claim. Historical FITS audit figures
are not current release/performance evidence; the remaining measurement and supplemental-sample limits are
recorded in [measurement status](docs/measurement-status.md).

## Why tzim

| | |
| --- | --- |
| **It refuses instead of guessing** | Progressive and arithmetic-coded JPEG, LZW TIFF, signed ZIPs, preset-dictionary zlib, BigTIFF: each is skipped with a precise reason and exit code 0. None of them is approximated. |
| **Its verification does not rely on one reader** | PNG pixels are decoded with the `png` crate; gzip and zlib payloads through `flate2`; JPEG coefficients with the project codec plus independent pixel verification (`jpeg-decoder`) in release-default builds. Where readers are independent and where code is shared are stated explicitly in [the verification matrix](docs/guarantees.md#26-verification-independence). |
| **It writes nothing unless it is smaller** | Equal or larger candidate: discarded, file untouched, `already optimal`. |
| **It is one static binary** | No C, no `unsafe` in our code, no shell-outs. Built with `panic = "abort"`, LTO and `opt-level = "s"`; the size is measured by CI on every pull request ([budget](docs/size-budget.md)). |
| **It costs what it costs, measured** | `docs/benchmarks.md` compares the ratio, the wall clock and the memory against `oxipng`, `optipng`, `jpegtran` and `gzip -9` — including the rows where the other tool wins. |

## Choose the right tool

All three tools below have lossless uses. This table compares scope and trade-offs; it is **not** a safety
ranking.

| Tool | Scope | Where it stands out | Trade-off |
| --- | --- | --- | --- |
| **Tzim** | PNG/APNG, JPEG, TIFF, gzip, ZIP, ICO/CUR and zlib | One command for mixed file trees, explicit per-format preservation contracts and a reason for every refusal | Not the ratio winner in the published PNG or JPEG comparison |
| **oxipng** | PNG / APNG | Best PNG ratio in this comparison: `oxipng -o max --strip none` saved **26.80 %** against Tzim's **7.63 %** on the published small-file benchmark | One format family; that run used more CPU time and memory than Tzim |
| **jpegtran** | JPEG | Lossless JPEG transformations and entropy optimization; `jpegtran -copy all -optimize` saved **6.51 %** against Tzim's **2.15 %** in the published mixed-scope JPEG row | Different preservation scope: it handles progressive JPEG, which Tzim refuses, and re-serialises markers, which Tzim copies verbatim |

Versions, commands, timings, memory and corpus limitations are in
[benchmarks §3.2](docs/benchmarks.md#32-against-the-established-tools). In particular, the JPEG percentages
above are **not a like-for-like encoder comparison**.

On PNG, choose `oxipng -o max` for the last percent of compression. Choose Tzim when the mixed-format
workflow, written contracts and explicit refusals matter more than winning that ratio contest.

## Guarantees

| Class | Meaning | Used by |
| --- | --- | --- |
| **G1 — byte-identical** | The output is identical to the input | anything left alone, every failure, `--dry-run` |
| **G2 — exactly reversible** | The original can be rebuilt bit-for-bit | not used: it needs a container, and the output would no longer be a normal file of its type |
| **G3 — format-equivalent** | Bytes differ; every piece of content and every significant property is preserved by a per-format contract, and verified | every real optimization |
| **G4 — metadata removed on request** | Content, names and order survive; a fixed, documented list of metadata is removed because it was asked for | `--strip-metadata` only |

Metadata — EXIF, ICC, comments, ZIP extra fields, archive comments — is content as far as this tool is
concerned, and survives byte for byte unless `--strip-metadata` says otherwise. If an optimization,
verification or the write fails, the original stays exactly as it was. The contracts, the verification method
for each and the refusal lists are in [docs/guarantees.md](docs/guarantees.md).

## The terminal

Output is plain text when it is not going to a terminal, so it survives being piped into a file. On a terminal
the report is coloured and a long run draws one line on stderr:

```
⠋ converting…
⠋ 00:01 [████░░░░] 3/12
```

A single file gets a spinner; a batch gets the clock, the bar and the counter. `--quiet`, `--json`, a pipe and
`--no-progress` all stay silent, and `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE` and `TERM=dumb` are honoured as
they are everywhere else ([docs/cli.md §10](docs/cli.md#10-colour-and-progress)).

## Documentation

| Document | Contents |
| --- | --- |
| [guarantees](docs/guarantees.md) | Guarantee classes and the contract of every format |
| [formats](docs/formats.md) | Format matrix, admission criteria, refused formats |
| [cli](docs/cli.md) | Flags, exit codes, JSON schema, colour and progress, CLI conformance |
| [safety](docs/safety.md) | Replacement algorithm, atomicity, failure modes |
| [dependencies](docs/dependencies.md) | Every dependency and why it exists |
| [size budget](docs/size-budget.md) | How small the binary is, and what each feature costs |
| [testing](docs/testing.md) | Test strategy, real-file corpus, fuzzing, CI |
| [benchmarks](docs/benchmarks.md) | Measured ratios, timings and memory against the alternatives |
| [releases](docs/releases.md) · [roadmap](docs/roadmap.md) | Artifacts and channels · versions and gates |
| [audits](docs/audits/) · [flac](docs/flac-audit.md) · [pdf](docs/pdf-audit.md) · [fits](docs/fits-audit.md) | Frozen independent audits, and the format audits behind the refusals and the open questions |
| [CONTRIBUTING](CONTRIBUTING.md) | How to build, test and submit a change |

## The name

Hebrew **צמצום** (*tsimtsum*): contraction, reduction, making space. In Lurianic Kabbalah it names the
withdrawal that makes room for something else to exist. For a file optimizer the meaning is plain: the bytes
contract, the information does not.

Written by AI. Directed by [RUdolf1517](https://github.com/RUdolf1517) ·
[rudolfzinovev@protonmail.com](mailto:rudolfzinovev@protonmail.com)

## License

[MIT](LICENSE). Dependency licences: [THIRD-PARTY.md](THIRD-PARTY.md).
