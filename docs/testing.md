# Testing

Related: [guarantees](guarantees.md) · [formats](formats.md) · [safety](safety.md)

A file optimizer is trusted or it is worthless. Tests are therefore not coverage theatre: every clause of
every contract has an executable check, and the failure modes that damage user data are tested explicitly.

---

## 1. Invariants under test

These hold for every format. Where a row below says the evidence is a unit test, a review, or nothing, it
says so: the table is a description of the tests that exist, not a wish list.

| ID | Invariant | How it is tested |
| --- | --- | --- |
| I1 | The original is never modified unless the candidate passes all checks | every refusal and failure test compares the file before and after (bytes, and `mtime` where it matters). There is **no** failure-injection harness: the stages that can fail are reached through real refusals — a corrupt input, an unsupported path, a read-only file, a candidate that is not smaller — not through a fake error at a chosen line |
| I2 | A candidate is only used if smaller, or the same size when stripping | `formats::size_ok` unit test (all four flag/size combinations, plus the empty-candidate case); `strip_metadata_equal_size_zip_is_optimized` and `strip_metadata_equal_size_gzip_is_not_a_failure` for the two handlers that can emit an equal-size candidate; fixtures with nothing to gain assert `already_optimal` |
| I3 | No metadata is removed | per-format inventory comparison in the integration tests, including unknown/private PNG chunks and ZIP extra fields |
| I4 | No lossy or converting transformation exists in the codebase | review gate; no such handler exists, so there is nothing to test |
| I5 | Concurrent modification aborts the operation | unit test of `replace::changed_since` (length, mtime, `(device, inode)`); the abort path itself is not driven end-to-end, because nothing can move a file at the right instant deterministically without a hook |
| I6 | Verification uses an independent decoder | documented per format in [guarantees §2.6](guarantees.md#26-verification-independence), and true for PNG/ICO/gzip/ZIP/zlib content through the `png` and `flate2` crates. There is **no** architecture test; the claim is checked by review, and the JPEG case where it does not hold is stated rather than hidden |
| I7 | Replacement is atomic where the platform allows | unit tests of the `replace` guard (`TempFile` removes itself unless committed) and of `changed_since`; that the `rename` is the last step is verified by reading `replace::commit`, which is 20 lines with one syscall in the middle |
| I8 | Unparseable input is refused, never repaired | corrupted inputs per format (bad CRC, truncated, missing `IEND`, trailing bytes, bad zlib trailer, inconsistent ZIP headers); assert `failed` and no output |
| I9 | Presentation never changes results | the same tree rendered with `--color=always` and `--color=never` is identical once SGR sequences are stripped; `--json --color=always` contains no escape sequence; the real-file corpus reports the same "bytes written" total as the previous release |

Idempotence is also a property: running Tzim twice on the same tree must produce zero changes the second
time.

---

## 2. Traceability

Each contract clause maps to at least one named test. The list is the checklist used in review; a format is
not done until every row passes in CI. A row whose assertion has no test says so in the row itself — the
table is evidence, not a plan.

### PNG

| ID | Assertion |
| --- | --- |
| `T-PNG-01` | All non-IDAT chunks (including an unknown/private chunk) are byte-identical, in the same order |
| `T-PNG-02` | IHDR payload is byte-identical |
| `T-PNG-03` | Decoded pixels are identical for the colour types the fixture writer builds (RGBA8). The other colour types — gray, gray+alpha, RGB, palette at 1/2/4/8 bits, 16-bit — are exercised by the real-file corpus, which asserts that they are rewritten without error but does **not** re-decode them; the writer is linear in the pixel bytes and touches only the compressed stream, but that is an argument, not a test. Widening `build_png` is the honest fix and is not done |
| `T-PNG-04` | Interlaced images: recompression only, pixels identical. **No test**: `PngSpec` cannot write an interlaced file |
| `T-PNG-05` | APNG: `acTL`/`fcTL`/`fdAT` untouched; default image pixels identical |
| `T-PNG-06` | Corrupted inputs (bad CRC, truncated, missing IEND, bytes after IEND) are refused |
| `T-PNG-07` | A candidate that is not smaller is discarded; file unchanged, status `already_optimal` |
| `T-PNG-08` | The output parses strictly and all chunk CRCs are valid |
| `T-PNG-09` | With `--zopfli`, the smallest of all candidates is chosen and verified — `t_png_09_zopfli_is_at_least_as_good_as_the_default` compares the two runs and requires the strong one to be no larger |

### gzip

| ID | Assertion |
| --- | --- |
| `T-GZ-01` | Header preserved with `FEXTRA`, `FNAME`, `FCOMMENT` — `t_gz_01_a_comment_is_preserved` splices the comment in and compares the header byte for byte |
| `T-GZ-02` | `FHCRC` preserved and still valid — `t_gz_02_fhcrc_is_preserved_and_still_valid` builds the CRC-16 the format defines and re-checks it in the output |
| `T-GZ-03` | Payload identical; trailer (`CRC32`, `ISIZE`) unchanged |
| `T-GZ-04` | Multi-member and trailing-garbage inputs are refused |
| `T-GZ-05` | Truncated or bad-trailer inputs are refused |
| `T-GZ-06` | `CM != 8` and reserved-flag inputs are refused |

### ZIP

| ID | Assertion |
| --- | --- |
| `T-ZIP-01` | Entry count, order and raw name bytes preserved (including non-UTF-8 names) |
| `T-ZIP-02` | Local and central extra fields preserved verbatim, in order |
| `T-ZIP-03` | Entry comments and archive comment preserved |
| `T-ZIP-04` | Flags, attributes, timestamps, method and uncompressed sizes preserved |
| `T-ZIP-05` | Archives using data descriptors (bit 3) are rewritten with correct descriptor values |
| `T-ZIP-06` | Stored entries stay stored |
| `T-ZIP-07` | Signed archives are refused (JAR `META-INF` signatures, APK signing block, `_xmlsignatures/`, `package/services/digital-signature/`) |
| `T-ZIP-08` | ODF/EPUB archives are accepted and keep `mimetype` stored first |
| `T-ZIP-09` | ZIP64, multi-disk, encrypted and unknown-method archives are refused — `t_zip_09_an_unknown_compression_method_is_refused` (method 12) and `t_zip_09_a_multi_disk_archive_is_refused` (a non-zero disk number) |
| `T-ZIP-10` | Truncated archives and inconsistent local headers are refused |
| `T-ZIP-11` | Every entry's decompressed bytes and CRC are identical |
| `T-ZIP-12` | `--max-uncompressed` and `--max-ratio` are enforced (decompression-bomb guard) — `t_zip_12_max_uncompressed_is_enforced` for the first, an earlier test for the second |

### ICO / APNG / nested / JPEG

| ID | Assertion |
| --- | --- |
| `T-ICO-01` | Directory header and entry order preserved |
| `T-ICO-02` | Embedded PNGs satisfy the PNG contract and decode identically |
| `T-ICO-03` | BMP/DIB entries are byte-identical |
| `T-ICO-04` | Malformed directories and out-of-range offsets are refused — `t_ico_04_an_implausible_image_count_is_refused` (a count that disagrees with the file) and the earlier offset test |
| `T-APNG-01` | `acTL` payload preserved |
| `T-APNG-02` | Every `fcTL` preserved, in order |
| `T-APNG-03` | Every frame decodes identically (pixels and frame metadata) |
| `T-APNG-04` | The default image is optimized and verified |
| `T-APNG-05` | Inconsistent frame sequences are refused. **No test**: no fixture produces an inconsistent `fcTL`/`fdAT` sequence, and the `png` crate would reject one before this build's own check runs |
| `T-NEST-01` | A child that fails verification is stored unmodified |
| `T-NEST-02` | Optimized children satisfy their own contracts |
| `T-NEST-03` | `--max-depth` is enforced |
| `T-NEST-04` | Nested expansion respects the parent's size budget. **No test**: `--max-depth` is tested, the per-entry budget is not |
| `T-JPG-01` | Marker inventory (type, order, payload) preserved; Exif and `COM` payloads survive |
| `T-JPG-02` | `DQT` and `SOF0` payloads byte-identical |
| `T-JPG-03` | Every DCT coefficient identical after the rewrite (stronger than comparing pixels) |
| `T-JPG-04` | Progressive, arithmetic, SOF1 and 12-bit inputs are refused, exit 0, file untouched |
| `T-JPG-05` | `DRI` and the restart markers in the scan are preserved, interval unchanged |
| `T-JPG-06` | JPEG is inert unless `--formats jpeg`; the allowlist also narrows the other handlers |
| `T-JPG-07` | Truncated or corrupt inputs fail with exit 1 and the original is untouched |
| `T-JPG-08` | A non-smaller candidate is discarded; a second run reports `already optimal` |
| `T-JPG-09` | Bytes after `EOI` are preserved verbatim |

### zlib

| ID | Test |
| --- | --- |
| `T-ZL-01` | a level-1 stream is recompressed, the payload decodes identically, and the file is smaller |
| `T-ZL-02` | the two header bytes and the four Adler-32 bytes are copied verbatim |
| `T-ZL-03` | a stream already written at level 9 is reported `already_optimal` and nothing is written |
| `T-ZL-04` | a wrong Adler-32 is `failed`; the file is untouched |
| `T-ZL-05` | a header that fails the multiple-of-31 check is `failed`, not "unsupported" |
| `T-ZL-06` | `FDICT` set: not treated as zlib at all — `unsupported`, untouched |
| `T-ZL-07` | a declared window below 32 KiB is `skipped` with a reason, untouched |
| `T-ZL-08` | trailing data after the stream is `skipped`, untouched; a false positive never writes |

The unit tests in `src/formats/zlib.rs` also pin the Adler-32 against `zlib`'s own values, including a 1 MiB
input that exercises the overflow bound.

### TIFF

| ID | Assertion |
| --- | --- |
| `T-TIF-01` | Byte order, tag set, entry order, type and count preserved for every entry |
| `T-TIF-02` | Every tag value identical except `Compression`, `StripOffsets`, `StripByteCounts` |
| `T-TIF-03` | Every strip decodes to identical bytes, through `flate2` rather than through the writer |
| `T-TIF-04` | Uncompressed strips become Deflate; `Compression` may change `1 → 8` or `32946 → 8` and nothing else |
| `T-TIF-05` | Little- and big-endian files both round-trip, byte order preserved |
| `T-TIF-06` | A Deflate file with nothing to gain is left alone (`already optimal`) |
| `T-TIF-07` | BigTIFF, tiles, multiple IFDs, planar `2` and predictors are refused with a reason, exit 0, file untouched |
| `T-TIF-08` | Unknown out-of-line tags are refused; unknown inline tags are copied untouched |
| `T-TIF-09` | Truncated or inconsistent files fail with exit 1 and the original is untouched |
| `T-TIF-10` | `--strip-metadata` removes the textual tags only; geometry, colour and ICC survive |
| `T-TIF-11` | `--formats png` reports a TIFF as skipped, not failed |

### The verifiers are tested against corrupted candidates

Every traceability row above says what the *handler* produces. The other half of the contract is that the
verifier refuses what it must, and until this existed no test called `verify` with a bad candidate: a verifier
that accepted everything would have passed the whole suite.

`tests/verification.rs` builds a valid original, builds a candidate that differs in exactly one field the
contract names, and requires `Err`:

| Test | Format | Corruption | The check it pins |
| --- | --- | --- | --- |
| `verification_rejects_changed_pixels` | PNG | one byte of one pixel, CRC recomputed | `decoded pixels differ` |
| `verification_rejects_a_dropped_chunk` | PNG | `tEXt` removed by hand, no `--strip-metadata` | chunk inventory |
| `verification_rejects_a_kept_filename` | gzip | `FNAME` kept under the flag | the stripped header |
| `verification_rejects_a_changed_stored_entry` | ZIP | one byte of a stored entry, CRC left alone | payload comparison |
| `verification_rejects_a_surviving_comment` | ZIP | archive comment put back under the flag | comment removal |
| `verification_rejects_a_changed_tag_value` | TIFF | an ASCII tag's value changed, geometry untouched | tag values |
| `verification_rejects_a_changed_bitmap_entry` | ICO | one byte of a BMP entry | payload comparison |
| `verification_rejects_changed_trailing_bytes` | JPEG | bytes appended after `EOI` | the trailing-bytes rule |
| `verification_rejects_a_larger_candidate` | all | candidate longer than the input | G3 clause 4, in the dispatcher |
| `verification_accepts_an_equal_size_candidate_only_when_stripping` | all | equal length, with and without the flag | the flag half of I2 |

Each corruption is chosen so that the other checks in the same function cannot see it — that is the point of
the file. Two rules are therefore **not** covered, and the reason is in the module comment: the APNG `fdAT`
sequence check (the `png` crate refuses a broken sequence first) and the JPEG coefficient comparison (a
corrupted entropy stream usually fails to parse, so the test would pass for the wrong reason).

---

## 3. Test levels

### Unit tests

- PNG scanline filters: encode/decode round-trip for all five filter types at every bytes-per-pixel value
  from 1 to 8, which covers 1-bit gray, palette, gray+alpha, RGB, RGBA, RGB16 and RGBA16. The rows are a
  fixed table, not a property test, and the table is the point: the arithmetic is defined in terms of bpp.
- Argument parser: every flag, combinations, `--`, conflicting flags, missing values.
- Size formatting, and `display_width`: cells rather than characters, so a CJK path counts two per
  character, an emoji two, and a combining accent zero. The range table behind it is tested on those four
  cases; anything outside the table counts as one cell, which is the old behaviour.
- JSON escaping: control characters, quotes, backslashes, non-UTF-8 paths.
- gzip header parse/serialise round-trip, including all flags.
- ZIP structure reader/writer: offsets, descriptors, EOCD, comments.
- `--include`/`--exclude` filtering (`src/walk.rs`, two unit tests). Deduplication and traversal order are
  covered by integration tests, not by unit tests.
- Colour detection (`src/style.rs`): a pure function of a probe struct, so `NO_COLOR`, `CLICOLOR`,
  `CLICOLOR_FORCE`, `TERM=dumb`, a pipe and a non-ANSI console are all covered without a terminal, and the
  three-way `--color` flag is tested against each of them.
- Progress line (`src/progress.rs`): both templates parse, the coloured and plain variants carry the same
  keys, the `mm:ss` clock (including the 59 -> 60 minute step), and the rule that a disabled or empty run
  creates no line at all. The drawing itself belongs to `indicatif`; what is ours is what is tested.

### Integration tests

Run the real binary against a temporary directory and assert on exit code, stdout/stderr, and file hashes.

The command line has its own contract, because it is what every other test goes through:

| Case | Asserted |
| --- | --- |
| `tzim photo.png` | a bare path is the whole command; the file is optimized |
| `tzim optimize photo.png` | the 0.1–0.4 spelling still parses, so old scripts keep working |
| `tzim` with no arguments | exit `2`, a message that names `--help` |
| `tzim -- -weird-name.png` | a path that starts with a dash needs the separator, and works with it |
| `tzim -i`, `--info`, `--list-formats`, `--check-deps` | exit `0`, no path needed, nothing on disk touched |
| `tzim --help` | lists `tzim [OPTIONS] <PATH>...` and the information commands, and no verb |
| `tzim x.png --strip-metadata` | text, `tIME` and `eXIf` gone; pixels identical; an unknown private chunk still present |
| `tzim x.gz --strip-metadata` | header reduced to ten bytes with `FLG = 0` and `MTIME = 0`; payload identical |
| `tzim x.jpg --formats jpeg --strip-metadata` | `APP1` and `COM` gone, `APP0` kept, coefficients unchanged |
| `tzim x.zip --strip-metadata` | archive and entry comments gone, extra fields byte-identical, payload identical |
| `tzim x.tif --strip-metadata` | textual tags gone, `Orientation`, resolution and every strip untouched |
| `tzim x.png --strip-metadata --dry-run --json` | nothing written; `metadata_stripped` in the record and `strip_metadata` in the summary |
| `tzim x.png` (no flag) | text chunks still there, whatever the encoder does |
| `tzim -rq`, `tzim -nr sub`, `tzim -j2` | clustered short flags and attached values behave as the long forms |
| `tzim --nope` | exit `2`, a `usage:` line, the offending flag and `--help` on stderr, nothing on stdout |
| `tzim --color=always x.png` | escape sequences appear, and stripping them gives exactly what `--color=never` printed |
| `tzim --json --color=always x.png` | the JSON contains no escape sequence |
| `tzim -r .` with stdout closed | exit code unchanged, no panic and no `failed printing` on stderr |

### Fixture corpus

**No binary fixture is committed.** Every fixture is built at run time by `tests/common/mod.rs`: writers for
PNG, gzip, ZIP, ICO, JPEG and TIFF, a `Workspace` type that owns a temporary directory, and byte-level
editors (`parse_zip`, `png_chunks`, `inflate_member`, …) used by the assertions. A test therefore states the
exact bytes it depends on, and a reader can see what "corrupt" means instead of trusting a blob.

| Format | Built by the fixture writers |
| --- | --- |
| PNG | RGBA8 at a chosen deflate level; interlaced; palette; `tEXt`/`tIME`/`pHYs`/`iCCP`; an unknown private chunk; APNG with several frames; bad CRC; truncated; missing `IEND`; bytes after `IEND`; huge IHDR dimensions with tiny data |
| gzip | empty payload; 1 byte; `FEXTRA`; `FNAME` with non-ASCII bytes; `FCOMMENT`; `FHCRC`; multi-member; trailing garbage; truncated; `CM = 7`; reserved flags |
| ZIP | store-only; deflate; mixed; data descriptors (with and without signature); ZIP64; encrypted; signed JAR; APK signing block; signed OOXML; ODF and EPUB; archive comment; entry comments; extra fields; non-UTF-8 names; duplicate names; empty archive; truncated; overlapping offsets; prepended stub |
| ICO | PNG-based, BMP-based, mixed |
| JPEG | baseline; restart markers; EXIF and ICC; progressive, arithmetic-coded and 12-bit inputs that must be refused |
| TIFF | little- and big-endian; uncompressed and Deflate strips; tiles; multi-IFD; planar 2; predictor 2; LZW; an unknown out-of-line tag; the textual tags for `--strip-metadata` |

Corrupted inputs are produced by flipping documented bytes in those writers, so a failure is reproducible
rather than a random blob. Large real-world samples are not fixtures at all: they are the corpus below,
downloaded and never committed.

### Filesystem and environment tests

Tested, on every platform CI runs:

| Scenario | Expected |
| --- | --- |
| path with spaces and Unicode (`иконка 🌀.png`) | optimized correctly |
| read-only file | `skipped`, original intact |
| missing path among valid ones | that path `failed`, others processed, exit `1` |
| symlink to a file | `skipped` by default; optimized with `--follow-symlinks` |
| symlink given directly, and a broken link | reported, nothing written |
| hard-linked file | `skipped` with a reason naming `--allow-hardlinks`; with the flag the replacement breaks the link on purpose and the other name keeps the old bytes |
| path that is not valid UTF-8 (Linux only) | optimized through the raw bytes; the JSON record carries `path_raw_b64`. APFS and Windows reject such a name, so the fixture cannot exist there |
| Unix socket | `skipped` as "not a regular file", still present afterwards |
| same file via three different paths | processed once |
| empty file, 1-byte file | `unsupported` |
| directory as input without `--recursive` | only its top-level files |
| second run on the optimized tree | zero changes (idempotence) |

**Not tested**, and listed here so that nobody assumes otherwise: a read-only *directory* (the fixture would
have to be immune to running as root, which CI on `ubuntu-latest` does), a FIFO or a device node, a full disk
(`ENOSPC`), a file modified while it is being processed, `SIGKILL` mid-run, and a symlink loop deeper than
the 64-level limit. Each needs an environment the test constructs by hand — a tmpfs, a signal delivered at a
chosen instant, `mknod` — which is why they are still open rather than claimed.

### Real-file corpus

Fixtures prove that the handlers handle what the authors thought of. The corpus proves what they do to files
nobody here wrote. `.github/scripts/corpus-fetch.sh` downloads four pinned upstream archives, checks each
SHA-256, and assembles the images into one tree; the run is `.github/scripts/corpus.sh`, CI job `corpus`.

| Source | Pinned | Checksum (SHA-256, first 16) | Files used |
| --- | --- | --- | --- |
| `pnggroup/libpng` | `v1.6.44` | `0ef5b633d0c65f78` | 51 from `contrib/pngsuite` |
| `libjpeg-turbo/libjpeg-turbo` | `3.1.0` | `35fec2e1ddfb05ec` | 4 from `testimages` |
| `image-rs/image` | `v0.25.6` | `e547553fc4e4353b` | 22 from `tests/images` |
| `python-pillow/Pillow` | `11.3.0` | `fa4aca745b1e1c73` | 613 from `Tests/images`: PNG, JPEG and, since v0.7.0, 118 TIFF |

Plus four containers built on the spot from those files with `gzip -9` and `zip -9`, so the gzip and ZIP
handlers are exercised on real payloads. The corpus is downloaded, never committed: it is 22 MiB of other
people's files, and a URL plus a checksum says more than a binary blob in review. The TIFF files matter more
than their count suggests: they are the reason a bug in the strip-layout scan was caught before release, and
the refusal reasons they produce (LZW, tiles, multi-page) are exactly the contract's exclusions.

The run executes the binary twice over the tree and gates on four properties, none of which is a fixture
assertion in disguise:

| Property | Failure means |
| --- | --- |
| No unexpected `failed` status | a real file that used to work no longer does, or a valid file is being refused |
| No `verify` or `internal` error, on any file | the handler produced a candidate its own contract rejected, or the code is broken |
| Nothing rewritten on the second pass, byte-identical manifest | the rewrite is not a fixed point |
| The counts of every status are recorded in the run summary | a change in how many files are refused or failed is visible in the report — but **not gated**: there is no committed baseline of refused paths, so a file that starts being refused does not fail CI unless it also fails the allowlist check |

Files that are corrupt in the upstream archive are listed by name in `.github/corpus-allowlist.txt`, with the
reason recorded. A file that fails without being listed is an error, and a listed file that stops failing
raises a warning so that the list cannot rot into a set of broad patterns hiding real problems.

### Fuzzing (`cargo-fuzz`, nightly, separate workspace)

`fuzz/` is a separate Cargo workspace so that `libfuzzer-sys` and the sanitizer runtime never become
dependencies of the binary. Three targets, all reached through the `fuzz-api` feature of the library, which
no release build enables:

| Target | Reaches |
| --- | --- |
| `jpeg_decode` | the JPEG marker parser, the layout analysis and the entropy decoder |
| `roundtrip` | detection, the handler for the detected format, and verification of the candidate |
| `strip` | the same round trip with `--strip-metadata`: the removal paths and the checks that the removed structures are gone |

The fuzz build enables **all seven** handlers (`fuzz/Cargo.toml`: `png, gzip, zip, ico, jpeg, tiff, zlib`), so
a detected TIFF or zlib stream reaches its real parser instead of the "format not compiled" arm.

The properties are: no panic, no unbounded allocation, no integer overflow, termination inside a time budget,
and — for `roundtrip` and `strip` — that any candidate reported as accepted is no larger than the input
(`formats::size_ok`, the same predicate the binary uses) and passes verification. All three targets are
seeded with the pngsuite and libjpeg-turbo test images. A 60 s-per-target smoke run is part of CI
(`fuzz.yml`); longer runs are performed before a release.

Fuzz artefacts are not committed; `fuzz/corpus` and `fuzz/artifacts` are ignored.

---

## 4. Continuous integration

| Job | Platforms | Contents |
| --- | --- | --- |
| `test` | ubuntu-latest, macos-latest, windows-latest | `cargo test --all-features --locked`, unit + integration |
| `msrv` | ubuntu-latest | `cargo build --all-features --locked` on Rust 1.88.0, the floor in `Cargo.toml` |
| `clippy` | ubuntu-latest | `cargo clippy --all-targets --all-features --locked -- -D warnings` |
| `fmt` | ubuntu-latest | `cargo fmt --all`; formats the tree and commits the result (§4.1) |
| `deny` | ubuntu-latest | `cargo deny check` — licences, advisories, duplicate versions, sources |
| `audit` | ubuntu-latest | `cargo audit` against the RustSec database |
| `size` | ubuntu-latest | `x86_64-unknown-linux-musl` build that `file` reports as `static-pie linked` (no interpreter, no libc), size gate and per-feature matrix (see [size budget](size-budget.md#6-ci-gate)) |
| `package` | ubuntu-latest | `cargo package --locked`, then a build from the packaged tarball — the crate as crates.io would receive it |
| `installer` | ubuntu-latest, macos-latest | runs `install.sh` against the published release, then proves a doctored archive is refused (§4.2) |
| `corpus` | ubuntu-latest | downloads the pinned corpus, runs the binary over it twice, gates on §3 |
| `fuzz` | ubuntu-latest, nightly | 60 s per target on pull requests that touch `src/` or `fuzz/`; weekly on `main` |
| `bench` | ubuntu-latest | comparative benchmarks, by `bench` label, weekly, or on dispatch |
| `lockfile` | ubuntu-latest | regenerates `Cargo.lock` and commits it to the branch (§4.1) |
| `docs` | ubuntu-latest | `.github/scripts/md-links.py` over every Markdown file: every relative link resolves and every `#anchor` exists. The gate is `broken: 0` |

`audit` overlaps the advisory half of `deny`, and both stay: `deny` is the one that also knows about licences
and duplicate versions, `audit` is the RustSec-native check that speaks the advisory database directly. Two
tools reading the same database with different rules is the point, not duplication.

### 4.1 Packaging is tested, not assumed

The files that are shipped but never compiled are the ones that rot quietly: a flag added to the parser and
missing from the man page, a release target added to the workflow and missing from the installer. Three
mechanisms cover them.

**Drift assertions** (`tests/packaging.rs`, part of `cargo test`). Every long option in `--help` must appear in
`man/tzim.1` and in all three completions; the man page's `.TH` line must carry the crate version; every
target in the release matrix must be fetchable by `install.sh` (Windows excepted, which is a zip and a winget
manifest); the Linux packaging script must ship the man page and the completions for both architectures; the
formula must install more than a binary; and `install.sh` must fetch `SHA256SUMS` and handle both `sha256sum`
and `shasum`.

**The installer job** runs `install.sh` for real, against the latest *published* release, on Linux and macOS —
the network path, the checksum lookup and the archive layout are exercised every time. It then serves a
doctored copy of the same archive with one byte appended, next to the release's untouched `SHA256SUMS`, and
asserts that the installer aborts, says `SHA-256 mismatch`, and leaves no binary behind. That is the one
property of a `curl | sh` script worth a CI job.

**The `package` job** builds the crate from the tarball `cargo package` produces. It catches a missing file, a
broken `readme` path, and a crate that only builds inside a git checkout.

### 4.2 The two jobs that write to the branch

This project is developed in an environment without a Rust toolchain: nothing local can run `cargo fmt` or
resolve a new dependency, and a pull request that added a crate would otherwise arrive with a `Cargo.lock`
that `--locked` rejects. Two jobs therefore write back instead of only reading:

- **`lockfile`** runs `cargo generate-lockfile` and commits `Cargo.lock` when it changed;
- **`fmt`** runs `cargo fmt --all` and commits the reformatted tree when it changed. That job fails on the run
  that had to change something, and the failure is the instructions: the fix is already pushed, re-run the
  job.

Both push with the workflow token, so neither retriggers CI, and both share the `branch-write-<ref>`
concurrency group so they cannot race for the ref. On a pull request from a fork there is no write access:
there `fmt` and `lockfile` fail with the diff in the log, which is the honest outcome — the branch is
unformatted or the lock is stale, and it has to be fixed by the author.

Release jobs build Linux (gnu and musl), macOS (x86_64 and aarch64) and Windows (x86_64) artifacts, and
publish them with checksums.

---

## 5. Benchmarks

Benchmarks are informational, and every published number must include the corpus, the tool versions and the
machine. They are never used to justify weakening a check.

| What | How |
| --- | --- |
| Ratio | total bytes before/after on a fixed corpus (PNG, gzip, ZIP), compared with `oxipng -o max`, `zopflipng`, `optipng -o7`, `gzip -9` |
| Time | wall clock, single job and `--jobs` default, cold page cache where possible |
| Memory | peak RSS from `/usr/bin/time -v` |
| Binary size | as in the size budget |

Results live in `docs/benchmarks.md` and are regenerated at every release. If a run is not reproducible on
another machine, it is not published.

---

## 6. Definition of done for a format

A handler is complete when:

1. its contract exists in [guarantees.md](guarantees.md);
2. every `T-*` row in §2 passes;
3. its fixture corpus exists, including corrupted and "refuse" cases;
4. fuzz targets cover its parsers;
5. its size cost is measured and recorded;
6. the README table, this document and the roadmap are updated;
7. a reviewer can point at the line of code for each contract clause.
