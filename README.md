<div align="center">

# tzim

**צמצום** — *tsimtsum*: contraction, making space

**Shrink the file, never the information.**

[![ci](https://github.com/RUdolf1517/tzim/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/RUdolf1517/tzim/actions/workflows/ci.yml)
[![release](https://img.shields.io/github/v/release/RUdolf1517/tzim?style=flat-square&color=0b3d91)](https://github.com/RUdolf1517/tzim/releases/latest)
[![binary](https://img.shields.io/badge/static%20binary-1024%20KiB-111111?style=flat-square)](docs/size-budget.md)
[![license](https://img.shields.io/badge/license-MIT-0b3d91?style=flat-square)](LICENSE)

[Install](#install) · [Usage](#usage) · [Formats](#formats) · [Why tzim](#why-tzim) · [Guarantees](#guarantees) · [Docs](#documentation)

</div>

---

`oxipng`, `jpegoptim` and `advzip` make a file smaller. Tzim makes it smaller **or leaves it alone** — and when
it cannot prove the result is the same information, it refuses and says why. Every format has a written
contract, every candidate is checked by an independent decoder against that contract, and a file is replaced
only if the new one is strictly smaller. There is no quality setting, no "visually lossless" mode and no
automatic fallback to a lossy path, because none of them are in the program.

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

Windows, x64 — until the manifest reaches the community source, winget installs it from this repository:

```powershell
git clone https://github.com/RUdolf1517/tzim; cd tzim
winget settings --enable LocalManifestFiles
winget install --manifest .\packaging\winget\0.11.0
```

`LocalManifestFiles` is once, as Administrator. Or take `tzim-x86_64-pc-windows-msvc.zip` from
[Releases](https://github.com/RUdolf1517/tzim/releases/latest) and put `tzim.exe` on `PATH`.

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
| JPEG | v0.4 | Entropy re-encoding with optimal Huffman tables; DCT coefficients and every marker untouched; baseline sequential only, opt-in | G3 |
| TIFF (classic, strips) | v0.7 | Recompress strips; every tag, entry order and geometry preserved; BigTIFF, tiling and LZW refused | G3 |
| zlib (`.zlib`, `.zz`) | v0.9 | Recompress the DEFLATE stream; header and Adler-32 preserved byte for byte | G3 |
| FLAC | — | **Refused after measurement**: a stronger encoder buys 0.4 % on music for ~150–300 KiB of binary ([audit](docs/flac-audit.md)) | — |
| PDF, WebP, AVIF/HEIC, MP4/MKV, MP3, GIF, xz/zstd/7z | — | **Not planned** ([why](docs/formats.md#5-refused-formats)) | — |

A format is listed as supported only when its contract and its tests exist. JPEG is off by default and enabled
by name (`--formats jpeg`), because it is the riskiest handler here and typically saves only 1–3 %.

## Why tzim

| | |
| --- | --- |
| **It refuses instead of guessing** | Progressive and arithmetic-coded JPEG, LZW TIFF, signed ZIPs, preset-dictionary zlib, BigTIFF: each is skipped with a precise reason and exit code 0. None of them is approximated. |
| **It verifies with something other than its own writer** | PNG is decoded with the `png` crate, not with our encoder; gzip and zlib payloads are compared through `flate2`; JPEG is compared as coefficients, a condition that implies pixel identity. |
| **It writes nothing unless it is smaller** | Equal or larger candidate: discarded, file untouched, `already optimal`. |
| **It is one static binary** | No C, no `unsafe` in our code, no shell-outs. Built with `panic = "abort"`, LTO and `opt-level = "s"`; the size is measured by CI on every pull request ([budget](docs/size-budget.md)). |
| **It costs what it costs, measured** | `docs/benchmarks.md` compares the ratio, the wall clock and the memory against `oxipng`, `optipng`, `jpegtran` and `gzip -9` — including the rows where the other tool wins. |

On PNG, `oxipng -o max` compresses better than this tool and the benchmarks say so plainly. What it does not
do is tell you when it declined to touch a file, or keep a per-format contract it can be held to.

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
| [audits](docs/audits/) | Frozen independent audits: what was checked, what was found, and the evidence |
| [CONTRIBUTING](CONTRIBUTING.md) | How to build, test and submit a change |

## The name

Hebrew **צמצום** (*tsimtsum*): contraction, reduction, making space. In Lurianic Kabbalah it names the
withdrawal that makes room for something else to exist. For a file optimizer the meaning is plain: the bytes
contract, the information does not.

Written by AI. Directed by [RUdolf1517](https://github.com/RUdolf1517) ·
[rudolfzinovev@protonmail.com](mailto:rudolfzinovev@protonmail.com)

## License

[MIT](LICENSE). Dependency licences: [THIRD-PARTY.md](THIRD-PARTY.md).
