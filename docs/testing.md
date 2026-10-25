# Testing

Related: [guarantees](guarantees.md) · [formats](formats.md) · [safety](safety.md)

A file optimizer is trusted or it is worthless. Tests are therefore not coverage theatre: every clause of
every contract has an executable check, and the failure modes that damage user data are tested explicitly.

---

## 1. Invariants under test

These hold for every format and are tested as properties, not as anecdotes.

| ID | Invariant | How it is tested |
| --- | --- | --- |
| I1 | The original is never modified unless the candidate passes all checks | inject failure at each pipeline stage; hash the original before and after |
| I2 | A candidate is only used if strictly smaller | fixtures whose best candidate is larger or equal; assert `already_optimal` and an unchanged file |
| I3 | No metadata is removed | per-format inventory comparison, including unknown/private chunks and extra fields |
| I4 | No lossy or converting transformation exists in the codebase | review gate; no such handler may exist, so there is nothing to test |
| I5 | Concurrent modification aborts the operation | test hook modifies the file mid-run; assert `failed` and an unchanged file |
| I6 | Verification uses an independent decoder | architecture test: the writer modules do not appear in the verification path |
| I7 | Replacement is atomic where the platform allows | instrumented test in `replace`: the rename is the last operation; failure before it leaves the original intact |
| I8 | Unparseable input is refused, never repaired | corpus of truncated/corrupted files; assert `failed` and no output |
| I9 | Presentation never changes results | the same file rendered with `--color=always` and `--color=never` is identical once SGR sequences are stripped; `--json --color=always` contains no escape sequence; the real-file corpus reports the same "bytes written" total as the previous release |

Idempotence is also a property: running Tzim twice on the same tree must produce zero changes the second
time.

---

## 2. Traceability

Each contract clause maps to at least one named test. The list is the checklist used in review; a format is
not done until every row passes in CI.

### PNG

| ID | Assertion |
| --- | --- |
| `T-PNG-01` | All non-IDAT chunks (including an unknown/private chunk) are byte-identical, in the same order |
| `T-PNG-02` | IHDR payload is byte-identical |
| `T-PNG-03` | Decoded pixels are identical for every color type × bit depth (gray, gray+alpha, RGB, RGBA, palette at 1/2/4/8 bits, 8 and 16 bit) |
| `T-PNG-04` | Interlaced images: recompression only, pixels identical |
| `T-PNG-05` | APNG: `acTL`/`fcTL`/`fdAT` untouched; default image pixels identical |
| `T-PNG-06` | Corrupted inputs (bad CRC, truncated, missing IEND, bytes after IEND) are refused |
| `T-PNG-07` | A candidate that is not smaller is discarded; file unchanged, status `already_optimal` |
| `T-PNG-08` | The output parses strictly and all chunk CRCs are valid |
| `T-PNG-09` | With `--zopfli`, the smallest of all candidates is chosen and verified |

### gzip

| ID | Assertion |
| --- | --- |
| `T-GZ-01` | Header preserved with `FEXTRA`, `FNAME`, `FCOMMENT` |
| `T-GZ-02` | `FHCRC` preserved and still valid |
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
| `T-ZIP-09` | ZIP64, multi-disk, encrypted and unknown-method archives are refused |
| `T-ZIP-10` | Truncated archives and inconsistent local headers are refused |
| `T-ZIP-11` | Every entry's decompressed bytes and CRC are identical |
| `T-ZIP-12` | `--max-uncompressed` and `--max-ratio` are enforced (decompression-bomb guard) |

### ICO / APNG / nested / JPEG

| ID | Assertion |
| --- | --- |
| `T-ICO-01` | Directory header and entry order preserved |
| `T-ICO-02` | Embedded PNGs satisfy the PNG contract and decode identically |
| `T-ICO-03` | BMP/DIB entries are byte-identical |
| `T-ICO-04` | Malformed directories and out-of-range offsets are refused |
| `T-APNG-01` | `acTL` payload preserved |
| `T-APNG-02` | Every `fcTL` preserved, in order |
| `T-APNG-03` | Every frame decodes identically (pixels and frame metadata) |
| `T-APNG-04` | The default image is optimized and verified |
| `T-APNG-05` | Inconsistent frame sequences are refused |
| `T-NEST-01` | A child that fails verification is stored unmodified |
| `T-NEST-02` | Optimized children satisfy their own contracts |
| `T-NEST-03` | `--max-depth` is enforced |
| `T-NEST-04` | Nested expansion respects the parent's size budget |
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

---

## 3. Test levels

### Unit tests

- PNG scanline filters: encode/decode round-trip for all five filter types, all bytes-per-pixel values,
  including bpp 1, 2, 3, 4, 6, 8 and 16-bit samples; property test with random rows.
- Argument parser: every flag, combinations, `--`, conflicting flags, missing values.
- Size formatting and column alignment (including Unicode and wide characters).
- JSON escaping: control characters, quotes, backslashes, non-UTF-8 paths.
- gzip header parse/serialise round-trip, including all flags.
- ZIP structure reader/writer: offsets, descriptors, EOCD, comments.
- Dedup and traversal policy.
- Colour detection (`src/style.rs`): a pure function of a probe struct, so `NO_COLOR`, `CLICOLOR`,
  `CLICOLOR_FORCE`, `TERM=dumb`, a pipe and a non-ANSI console are all covered without a terminal, and the
  three-way `--color` flag is tested against each of them.
- Progress line text (`src/progress.rs`): frame rotation, wrapping, the saved total appearing only when
  there is one, and a hard limit on the length of the line so it cannot wrap in a narrow terminal.

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

Committed under `tests/fixtures/`, generated by scripts in `tests/fixtures/generate/`, with provenance,
licence and a one-line description in `tests/fixtures/README.md`. Fixtures are deliberately small; large
real-world samples are generated, not committed.

| Format | Cover |
| --- | --- |
| PNG | every color type and bit depth; interlaced; APNG with several frames; palette with `tRNS`; `iCCP`, `eXIf`, `tEXt`, `tIME`, `pHYs`, `sBIT`; an unknown private chunk; zero-length `tEXt`; data after IEND; bad CRC; truncated; huge dimensions in IHDR with tiny data |
| gzip | empty payload; 1 byte; 1 MiB of text; `FEXTRA`; `FNAME` with non-ASCII bytes; `FCOMMENT`; `FHCRC`; multi-member; trailing garbage; truncated; `CM = 7`; reserved flags |
| ZIP | store-only; deflate; mixed; data descriptors; ZIP64; encrypted; signed JAR; APK with signing block; signed OOXML (`_xmlsignatures/`); signed VSIX; ODF and EPUB; archive comment; entry comments; NTFS/extended timestamp extra fields; non-UTF-8 names; duplicate names; empty archive; empty entry; symlink entry; truncated; overlapping offsets; prepended stub |
| ICO | PNG-based, BMP-based, mixed, 256×256 |
| JPEG | baseline; progressive; restart markers; CMYK; EXIF+ICC; arithmetic-coded (refused); 12-bit (refused); truncated |

Corrupted fixtures are produced by a generator that flips bytes at documented offsets, so the failure is
reproducible rather than a random blob.

### Filesystem and environment tests

| Scenario | Expected |
| --- | --- |
| path with spaces and Unicode (`иконка 🌀.png`) | optimized correctly |
| path that is not valid UTF-8 (Unix) | optimized; JSON contains `path_raw_b64` |
| read-only directory | `failed`, original intact |
| read-only file | `skipped`, original intact |
| missing path among valid ones | that path `failed`, others processed, exit `1` |
| symlink to a file | `skipped` by default; optimized with `--follow-symlinks` |
| symlink loop in a tree | terminates, reports, exit `0` or `1` |
| hard-linked file | `skipped` by default; processed with `--allow-hardlinks` |
| same file via three different paths | processed once |
| FIFO / socket / device | skipped, never opened for writing |
| disk full (small tmpfs) | `failed`, original intact, no temporary file left |
| file modified during processing | `failed`, original intact |
| `SIGKILL` during processing | original intact; a `.tzim-*.tmp` may remain (asserted, documented) |
| second run on the optimized tree | zero changes (idempotence) |
| empty file, 1-byte file | `unsupported` |
| very deep directory tree | traversal terminates within limits |

### Failure injection

A `test-hooks` feature (never enabled in release builds) provides:

- `TZIM_TEST_FAIL_AFTER_WRITE` — fail after the candidate is written;
- `TZIM_TEST_FAIL_BEFORE_RENAME` — fail after verification;
- `TZIM_TEST_CORRUPT_CANDIDATE` — produce a candidate that violates the contract;
- `TZIM_TEST_SLOW_WRITE` — widen the race window for the concurrent-modification test.

Each hook has a test asserting the outcome, the original hash, exit code and temporary-file cleanup.

### Property tests (`proptest`)

- filter round-trip: any row, any bpp, any filter type;
- "output is never larger than input", over random valid PNGs and gzip files;
- "verification always passes for an accepted candidate";
- idempotence: `optimize(optimize(x)) == optimize(x)` in terms of size and status;
- parser robustness: arbitrary bytes never panic, only produce `Err`.

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
| The set of refused files is unchanged | a construct that used to be supported silently stopped being supported |

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

The properties are: no panic, no unbounded allocation, no integer overflow, termination inside a time budget,
and — for `roundtrip` and `strip` — that any candidate reported as accepted is no larger than the input and
passes verification. The seed corpus is the pngsuite and libjpeg-turbo test images. A 60 s-per-target smoke run is
part of CI (`fuzz.yml`); longer runs are performed before a release.

Fuzz artefacts are not committed; `fuzz/corpus` and `fuzz/artifacts` are ignored.

---

## 4. Continuous integration

| Job | Platforms | Contents |
| --- | --- | --- |
| `test` | ubuntu-latest, macos-latest, windows-latest | `cargo test --all-features`, unit + integration |
| `msrv` | ubuntu-latest | build with Rust 1.88 |
| `clippy` | ubuntu-latest | `cargo clippy --all-targets -- -D warnings` |
| `fmt` | ubuntu-latest | `cargo fmt --check` |
| `deny` | ubuntu-latest | `cargo deny check` (licences, advisories, duplicates) |
| `audit` | ubuntu-latest | `cargo audit` |
| `musl` | ubuntu-latest | static `x86_64-unknown-linux-musl` build; `file` must report "statically linked" |
| `size` | ubuntu-latest | binary-size gate (see [size budget](size-budget.md#6-ci-gate)) |
| `corpus` | ubuntu-latest | downloads the pinned corpus, runs the binary over it twice, gates on §3 |
| `fuzz` | ubuntu-latest, nightly | 60 s per target on pull requests that touch `src/` or `fuzz/`; weekly on `main` |
| `bench` | ubuntu-latest | comparative benchmarks, by `bench` label, weekly, or on dispatch |

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
