<div align="center">

# tzim

**צמצום** — *tsimtsum*: contraction, making space

Shrink the file, never the information.  
One CLI for provably lossless optimization — PNG · APNG · gzip · ZIP · ICO/CUR · JPEG.

[Install](#install) · [Usage](#usage) · [Formats](#formats) · [Guarantees](#guarantees) · [The name](#the-name)

[![license](https://img.shields.io/badge/license-MIT-0b3d91?style=flat-square)](LICENSE)
[![platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-111111?style=flat-square)](#install)
[![rust](https://img.shields.io/badge/rust-1.88%2B-dea584?style=flat-square)](https://www.rust-lang.org/)

</div>

---

```bash
tzim optimize photo.png
tzim optimize ./photos -r
tzim optimize backup.zip --nested
tzim optimize scan.jpg --formats jpeg
tzim optimize . -r --dry-run
```

```
14 files  48.2 MB → 43.7 MB  (-9.3 %)   2 already optimal
```

The file gets smaller or it is left alone. There is no third outcome.

## Install

**Binary** (no Rust) into `~/.local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/RUdolf1517/tzim/main/install.sh | sh
```

On a platform without a published build it falls back to `cargo install --git` instead of guessing a
binary.

**Homebrew** (no Rust). Third-party tap — Homebrew 6+ [tap trust](https://docs.brew.sh/Tap-Trust).

```bash
brew tap RUdolf1517/tzim https://github.com/RUdolf1517/tzim
brew install RUdolf1517/tzim/tzim
```

`RUdolf1517/tzim/tzim` is `user/tap/formula`. A trailing slash (`brew install RUdolf1517/tzim/`) names
no formula and fails with *No available formula or cask with the name "rudolf1517/tzim/"*.

Only two platforms have a published build: macOS on Apple silicon and Linux x86_64. On an Intel Mac or
ARM Linux the formula has no URL, and `brew install` cannot help — use the script above or
`cargo install --git` there.

After that, short name:

```bash
brew trust --formula RUdolf1517/tzim/tzim
brew install tzim
```

**apt** (Debian/Ubuntu x86_64, no Rust):

```bash
curl -fsSL -O https://github.com/RUdolf1517/tzim/releases/latest/download/tzim_amd64.deb
sudo apt install ./tzim_amd64.deb
```

**dnf** (Fedora/RHEL x86_64, no Rust):

```bash
curl -fsSL -O https://github.com/RUdolf1517/tzim/releases/latest/download/tzim.x86_64.rpm
sudo dnf install ./tzim.x86_64.rpm
```

Both packages contain the same statically linked binary and declare no dependency at all.

**winget** (Windows x64). Until the manifest is in the community source, install it from this GitHub repo —
winget downloads `tzim-x86_64-pc-windows-msvc.zip` from Releases:

```powershell
git clone https://github.com/RUdolf1517/tzim
cd tzim
winget settings --enable LocalManifestFiles
winget install --manifest .\packaging\winget\0.4.3
```

`LocalManifestFiles` is once, as Administrator. After microsoft/winget-pkgs accepts the manifest:

```powershell
winget install -e --id RUdolf1517.tzim
```

Zip only (no winget): [Releases](https://github.com/RUdolf1517/tzim/releases/latest) →
`tzim-x86_64-pc-windows-msvc.zip`, put `tzim.exe` somewhere on `PATH`.

**Cargo** ([Rust 1.88+](https://rustup.rs)):

```bash
cargo install --git https://github.com/RUdolf1517/tzim
```

macOS (Apple Silicon), Linux (x86_64, static musl), Windows x64. ARM Linux and Intel macOS build from
source in one command.

<details>
<summary>From a checkout</summary>

```bash
git clone https://github.com/RUdolf1517/tzim
cd tzim
cargo build --release            # target/release/tzim

# smallest build: PNG and gzip only, no Zopfli encoder
cargo build --release --no-default-features --features png,gzip

# static Linux binary, matching the published artifact
cargo build --release --target x86_64-unknown-linux-musl
```

</details>

### Dependencies

None. No C toolchain, no runtime, no configuration file, no daemon. The Linux binary is statically linked
against musl and runs on any kernel from 3.2; the whole program links four Rust crates and contains no
`unsafe` code. It opens no sockets, checks for no updates and sends no telemetry.

## Usage

```bash
tzim optimize image.png                  # one file, no wrapping directory
tzim optimize a.png b.zip c.jpg          # several, mixed kinds in one run
tzim optimize ./files                    # a directory, top level only
tzim optimize ./files -r                 # the whole tree
tzim optimize ./files -r --dry-run       # everything except the final rename
tzim optimize image.png --zopfli         # best ratio, 10-100x slower
tzim optimize ./files -r --json          # one JSON object per file
```

Every path is a full input object: a single file never needs to be wrapped in a directory, and files,
directories and duplicates can be mixed freely in one invocation.

### Scope

```bash
tzim formats                              # what this build supports
tzim optimize photo.jpg --formats jpeg    # JPEG is opt-in
tzim optimize project.zip --nested        # also optimize supported files inside
tzim optimize ./src --include png --exclude lock
```

JPEG is off by default and enabled by name, because it is the riskiest handler here and typically saves
only 1–3 %.

### Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success. Includes files that were already optimal, skipped, unsupported or refused. |
| `1` | One or more files failed; the others were still processed. |
| `2` | Invalid usage; nothing was processed. |
| `3` | Fatal error; the run could not be completed. |
| `4` | **Verification failure** — a candidate did not satisfy its contract. The original was not replaced, and this means a bug. |

<details>
<summary>Flags</summary>

```
tzim optimize [OPTIONS] <PATH>...

  -r, --recursive            descend into directories
  -n, --dry-run              do all the work, write nothing
  -j, --jobs <N>             files processed at once (default: min(cores, 4))
      --json                 newline-delimited JSON on stdout
  -q, --quiet                suppress per-file output
      --fail-fast            stop after the first failed file
      --include <EXT,...>    only files with these extensions
      --exclude <EXT,...>    skip files with these extensions
      --min-size / --max-size           size window (default: 0 .. 2 GiB)
      --max-pixels <N>                  skip larger images
      --max-uncompressed / --max-ratio  decompression-bomb guards for ZIP
      --nested               optimize supported files inside archives
      --max-depth <N>        nesting limit (default: 3)
      --formats <LIST>       png, gzip, zip, ico, jpeg
      --zopfli               use the slower, stronger encoder
      --follow-symlinks      resolve and follow symbolic links
      --allow-hardlinks      process files with more than one hard link
      --no-fsync             skip fsync (faster, less durable)
```

Full reference: [docs/cli.md](docs/cli.md).

</details>

## Guarantees

Tzim does not use one vague notion of losslessness. Every handler declares a guarantee class, and the class
is checked mechanically before a file is replaced.

| Class | Meaning | Used by |
| --- | --- | --- |
| **G1 — byte-identical** | The output is identical to the input. | `already_optimal`, `unsupported`, `skipped`, every failure, every `--dry-run`. |
| **G2 — exactly reversible** | The original can be reconstructed bit-for-bit. | Not used. It needs a container and a second decoding step, which conflicts with "the output is a normal file of the same type". |
| **G3 — format-equivalent** | Bytes differ; every piece of content and every significant property is preserved by an explicit per-format contract, and verified. | All real optimizations. |

> **Tzim never performs an irreversible or lossy transformation.**

- No quality, resolution, bit-depth or frame reduction, and no "visually lossless".
- **No metadata, comment, attachment or vendor-chunk removal** — not even the metadata other tools call
  safe. EXIF, ICC, `COM`, ZIP extra fields and archive comments survive byte for byte.
- No format conversion.
- No fallback to a lossy path when lossless yields nothing.
- If the result is not strictly smaller, the file is untouched and reported as `already_optimal`.
- If optimization, verification or the write fails, the original stays byte-for-byte unchanged.

## Formats

| Format | Since | Status | Transformation | Guarantee |
| --- | --- | --- | --- | --- |
| PNG | v0.1 | **implemented** | Recompress IDAT; re-select scanline filters; optional Zopfli | G3 |
| gzip (`.gz`, `.tgz`, `.svgz`) | v0.1 | **implemented** | Recompress the DEFLATE body; header preserved verbatim | G3 |
| ZIP family (`.zip`, `.jar`, `.apk`, `.docx`, `.epub`, `.whl`, …) | v0.2 | **implemented** | Recompress DEFLATE entries; names, extra fields, comments, order and attributes preserved; signed, ZIP64 and encrypted archives refused | G3 |
| ICO / CUR | v0.3 | **implemented** | Optimize embedded PNGs; bitmap entries copied verbatim | G3 |
| APNG | v0.3 | **implemented** | Recompress every frame, keeping chunk count and sequence numbers | G3 |
| Nested containers | v0.3 | **implemented**, opt-in | Optimize supported files inside ZIP archives (`--nested`) | G3 |
| JPEG | v0.4 | **implemented**, opt-in | Entropy re-encoding with optimal Huffman tables; DCT coefficients and every marker untouched; baseline sequential only | G3 |
| TIFF, FLAC | v0.5+ | research | not specified yet — no contract, no claim | — |
| PDF, WebP, AVIF/HEIC, MP4/MKV, MP3, GIF, xz/zstd/7z | — | **not planned** | — | — |

Progressive, arithmetic-coded, lossless, hierarchical and 12-bit JPEGs are refused with a precise reason
rather than approximated. Why the other formats are refused is in
[docs/formats.md](docs/formats.md#5-refused-formats).

## What tzim will never do

- Remove EXIF, ICC profiles, comments, thumbnails, vendor chunks, ZIP extra fields or archive comments.
- Re-encode images, audio or video in a lossy way.
- Convert a file to another format.
- Minify HTML, CSS, JavaScript, SVG or XML.
- Rewrite a file in place, truncate it, or write a larger result.
- Replace a file that changed on disk while it was being processed.
- Claim support for a format before its contract exists and its tests pass.

## Documentation

| Document | Contents |
| --- | --- |
| [docs/guarantees.md](docs/guarantees.md) | Guarantee classes, per-format contracts, verification |
| [docs/formats.md](docs/formats.md) | Format matrix, admission criteria, refused formats |
| [docs/cli.md](docs/cli.md) | Command-line interface, exit codes, JSON schema |
| [docs/safety.md](docs/safety.md) | Replacement algorithm, atomicity, failure modes |
| [docs/dependencies.md](docs/dependencies.md) | Every dependency and why it exists |
| [docs/size-budget.md](docs/size-budget.md) | Binary size budget and measurement |
| [docs/testing.md](docs/testing.md) | Test strategy, real-file corpus, fuzzing, CI |
| [docs/benchmarks.md](docs/benchmarks.md) | Measured size, tests, corpus run, comparisons |
| [docs/releases.md](docs/releases.md) | Artifacts, install channels, release process |
| [docs/roadmap.md](docs/roadmap.md) | Versions, gates, risks |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How to build, test and submit changes |

The static Linux binary measures **980 KiB**. The real-file corpus run saves **4.19 %** over 576 files from
four pinned upstream projects and changes nothing on a second pass. Both numbers, and the comparisons
against oxipng, optipng and jpegtran, are in [docs/benchmarks.md](docs/benchmarks.md).

## The name

Hebrew **צמצום** (*tsimtsum*): contraction, reduction, making space.

In Lurianic Kabbalah it names the withdrawal that makes room for something else to exist. For a file
optimizer the meaning is plain: the bytes contract, the information does not.

```bash
tzim --version
tzim formats
```

Written by AI. Directed by [RUdolf1517](https://github.com/RUdolf1517).  
[rudolfzinovev@protonmail.com](mailto:rudolfzinovev@protonmail.com)

## License

[MIT](LICENSE). See [docs/dependencies.md](docs/dependencies.md#5-licence-policy) for the dependency
licence policy.
