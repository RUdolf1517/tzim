# Tzim

**Shrink the file, never the information.**

Tzim is a small, cross-platform command-line tool that makes files smaller using only *provably lossless*
transformations. It processes individual files, many files, or whole directory trees, and it refuses to
perform any operation whose safety it cannot verify.

The name comes from the Hebrew צמצום (*tsimtsum*) — contraction, reduction, making space.

---

## Status

**v0.4 implemented.** PNG, APNG, gzip, the ZIP family, ICO/CUR and — opt-in — baseline JPEG are supported
and verified, including optional optimization of files stored inside archives (`--nested`). The code is
tested on Linux, macOS and Windows in CI. Everything else in the table below is *planned*, with the version
in which it is expected.

- Static Linux binary: **976 KiB** (measured, all features, musl). See [docs/benchmarks.md](docs/benchmarks.md).
- 56 unit and 54 integration tests, clippy clean with `-D warnings`.
- No release artifacts have been published yet; build from source (below).

See [docs/roadmap.md](docs/roadmap.md) for the version plan and [docs/formats.md](docs/formats.md) for the
admission criteria a format must meet before it may be listed as supported.

---

## The invariant

> **Tzim never performs an irreversible or lossy transformation.**

This is not a default that can be turned off with a flag. It is a structural property of the design:

- No quality reduction, no resampling, no bit-depth reduction, no frame dropping.
- **No metadata, comment, attachment or vendor-chunk removal** — not even "safe" metadata. Other tools
  strip it; Tzim does not.
- No format conversion (a PNG never becomes a WebP, a TIFF never becomes a PNG).
- No switching to a lossy algorithm when lossless optimization yields nothing.
- If the result is not strictly smaller than the input, the input is left untouched and reported as
  `already_optimal`.
- If optimization, verification or the write itself fails at any point, the original file is left
  byte-for-byte unchanged.

## What "lossless" means here

Tzim does not use one vague notion of losslessness. Every format declares one guarantee class, and the
class is verified mechanically before a file is replaced:

| Class | Meaning | Used by |
| --- | --- | --- |
| **G1 — byte-identical** | The output file is identical to the input. | No-op outcomes only: `already_optimal`, `unsupported`, `skipped`, all failures, and every `--dry-run` run. |
| **G2 — exactly reversible** | The transformation is reversible and the original file can be reconstructed bit-for-bit. | Not used by Tzim. It requires a container format and a second decoding step (the Precomp approach), which conflicts with "output is a normal file of the same type". |
| **G3 — format-equivalent** | Bytes differ, but every piece of content and every significant property is preserved according to an explicit per-format contract. | All real optimizations in Tzim. |

Each G3 contract states exactly what is preserved, what is allowed to change, and how the result is
verified. See [docs/guarantees.md](docs/guarantees.md).

---

## Formats

| Format | Version | Status | Transformation | Guarantee |
| --- | --- | --- | --- | --- |
| PNG | v0.1 | **implemented** | Recompress IDAT; re-select scanline filters; optional Zopfli | G3 |
| gzip (`.gz`, `.tgz`, `.svgz`) | v0.1 | **implemented** | Recompress the DEFLATE body; header preserved verbatim | G3 |
| ZIP and ZIP-based formats (`.zip`, `.jar`, `.apk`, `.docx/.xlsx/.pptx`, `.epub`, `.odt/.ods/.odp`, `.cbz`, `.whl`, `.nupkg`) | v0.2 | **implemented** | Recompress DEFLATE entries; names, extra fields, comments, order and attributes preserved; signed, ZIP64 and encrypted archives are refused | G3 |
| ICO / CUR | v0.3 | **implemented** | Optimize embedded PNG images; bitmap entries copied verbatim | G3 |
| APNG | v0.3 | **implemented** | Recompress every frame, keeping chunk count and sequence numbers | G3 |
| Nested containers | v0.3 | **implemented**, opt-in | Optimize supported files inside ZIP archives (`--nested`) | G3 |
| JPEG | v0.4 | **implemented**, opt-in (`--formats jpeg`) | Entropy re-encoding with optimal Huffman tables; DCT coefficients and every marker untouched; baseline sequential only | G3 |
| TIFF | v0.5+ | research | Recompress Deflate/LZW strips without changing tags | not specified yet |
| FLAC | v0.5+ | research | Re-encode losslessly, preserving all metadata blocks | not specified yet |
| PDF, WebP lossless, AVIF/HEIC, MP4/MKV, MP3, GIF, xz/zstd/7z | — | **not planned** | — | — |

JPEG is opt-in because it carries the highest risk in the project and typically saves only 1–3 %. Progressive,
arithmetic-coded, lossless, hierarchical and 12-bit JPEGs are refused with a precise reason rather than
approximated; see [the contract](docs/guarantees.md#47-jpeg--v04-opt-in).

Formats that are refused, and the reasoning, are listed in [docs/formats.md](docs/formats.md#5-refused-formats).
In short: for PDF and modern image/video codecs there is no way to guarantee and verify preservation with
the reliability this project requires, and for text formats the only available savings come from
minification, which destroys information.

---

## Installation

```sh
# from source (requires Rust 1.88 or newer)
cargo install --path .

# smallest build: PNG and gzip only, no Zopfli encoder
cargo build --release --no-default-features --features png,gzip

# static Linux binary, matching the measured 832 KiB artifact
cargo build --release --target x86_64-unknown-linux-musl
```

The tool builds as a single static binary with no C toolchain and no runtime dependencies. Prebuilt
binaries for Linux, macOS and Windows will be published with the first tagged release.

## Usage

```sh
# one file
tzim optimize image.png

# several files
tzim optimize image.png archive.zip document.pdf

# a directory (top level only)
tzim optimize ./files

# a directory tree
tzim optimize ./files --recursive

# preview without writing anything
tzim optimize ./files --recursive --dry-run

# best possible compression, much slower
tzim optimize image.png --zopfli

# machine-readable output
tzim optimize ./files --recursive --json
```

Every path is a full input object: a single file never needs to be wrapped in a directory. Files,
directories and duplicates may be mixed freely in one invocation.

Full CLI reference: [docs/cli.md](docs/cli.md).

### Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success. Includes files that were already optimal, skipped, unsupported or refused. |
| `1` | One or more files failed; other files were still processed. |
| `2` | Invalid usage; nothing was processed. |
| `3` | Fatal error; the run could not be completed. |
| `4` | **Verification failure** — a candidate did not satisfy its contract. The original file was not replaced. This indicates a bug and aborts the run. |

---

## What Tzim will never do

These are permanent non-goals, not unimplemented features:

- Remove EXIF, ICC profiles, comments, thumbnails, vendor chunks, ZIP extra fields or archive comments.
- Re-encode images, audio or video in a lossy way, or "visually losslessly".
- Convert a file to another format.
- Minify HTML, CSS, JavaScript, SVG or XML (that is exactly the kind of data removal this project forbids).
- Rewrite a file in place, truncate it, or write a larger result.
- Replace a file that changed on disk while it was being processed.
- Claim support for a format before its contract exists and its tests pass.

---

## Documentation

| Document | Contents |
| --- | --- |
| [docs/guarantees.md](docs/guarantees.md) | Guarantee classes, per-format contracts, verification methods |
| [docs/formats.md](docs/formats.md) | Format matrix, admission criteria, refused formats |
| [docs/cli.md](docs/cli.md) | Command-line interface, exit codes, JSON schema |
| [docs/safety.md](docs/safety.md) | File replacement algorithm, atomicity, metadata, failure modes |
| [docs/dependencies.md](docs/dependencies.md) | Every dependency and why it exists |
| [docs/size-budget.md](docs/size-budget.md) | Binary size budget, feature matrix, measurement procedure |
| [docs/testing.md](docs/testing.md) | Test strategy, fixture corpus, fuzzing, CI |
| [docs/benchmarks.md](docs/benchmarks.md) | Measured binary size, tests, and what is still unmeasured |
| [docs/roadmap.md](docs/roadmap.md) | Versions, gates, complexity and risks |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How to build, test and submit changes |

## License

MIT. See [LICENSE](LICENSE). All dependencies must be compatible with it; see
[docs/dependencies.md](docs/dependencies.md#5-licence-policy).
