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
| `T-JPG-01` | Marker inventory (type, order, payload) preserved |
| `T-JPG-02` | `DQT` and `SOF` payloads byte-identical |
| `T-JPG-03` | Decoded component planes bit-identical (baseline) |
| `T-JPG-04` | Decoded component planes bit-identical (progressive) |
| `T-JPG-05` | Restart-marker intervals preserved |
| `T-JPG-06` | Arithmetic-coded, lossless and 12-bit inputs are refused |
| `T-JPG-07` | Truncated or corrupt inputs are refused |
| `T-JPG-08` | A non-smaller candidate is discarded |

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

### Integration tests

Run the real binary against a temporary directory and assert on exit code, stdout/stderr, and file hashes.

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

### Fuzzing (`cargo-fuzz`, nightly, separate crate)

Targets: PNG chunk walker, gzip header/body splitter, ZIP central-directory reader, and (from v0.4) the JPEG
marker/entropy reader. The properties are: no panic, no unbounded allocation, no integer overflow, and
termination inside a time budget. The seed corpus is derived from the fixtures; a short smoke run (60 s per
target) executes in CI on Linux, and longer runs are performed before a release.

Fuzz artefacts are not committed; the corpus is minimised and stored only if it stays small.

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
| `fuzz-smoke` | ubuntu-latest, nightly | 60 s per target, optional on pull requests, mandatory on `main` |

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
