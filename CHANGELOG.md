# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

**v0.4.0 was the first published release.** Versions 0.1 to 0.3 were development milestones: they exist as
commits, were tested in CI, and were never tagged or packaged. Everything in them is contained in 0.4.0.
Planned releases are described in [docs/roadmap.md](docs/roadmap.md); the packaging policy is in
[docs/releases.md](docs/releases.md).

## [0.6.0] - 2026-10-24 — narrowing the build, and metadata on request

### Added

- **`--strip-metadata`**: removes a fixed, documented list of embedded metadata — PNG text and `tIME`/
  `eXIf`, the gzip name/comment/extra/mtime, ZIP comments and DOS timestamps, JPEG `APP1`/`APP13`/`COM` —
  and nothing else. ICC profiles, colour hints, unknown chunks and every pixel, coefficient, payload and
  name stay. Off by default; the only transformation that is not G3, so it gets its own class, G4
  ([guarantees §2.7](docs/guarantees.md#27-g4--metadata-removed-on-request)).
- **Features per handler**: `jpeg` and `ico` join `png`, `gzip`, `zip` and `zopfli`, so a build can drop
  what it will never see. The default set is unchanged and still ships every handler.
- The fuzz target `strip` covers the removal paths, and CI measures every feature combination
  (`ci` job `binary size`, step `Feature matrix`) instead of only the full build.

### Changed

- **For packagers, not users:** JPEG used to be compiled in unconditionally. A build with
  `--no-default-features --features png,gzip` is now smaller and no longer handles JPEG; `--formats jpeg`
  on such a build is a usage error that names the handlers it does have, instead of a silent no-op.
  `ico = ["png"]`, so the two cannot be split.
- With `--strip-metadata` the acceptance rule becomes "never larger" instead of "strictly smaller":
  zeroing a timestamp removes metadata without removing bytes. A larger candidate is still refused under
  every flag.

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.6.0/` are pinned to the published v0.6.0 artifacts and their
  SHA-256, copied from the release notes after the build.

### Documentation

- [guarantees](docs/guarantees.md) now names the **verification level** of every format: an independent
  decoder (PNG, gzip, ZIP payloads), a separate reader (none yet) or a shared implementation (JPEG
  coefficients, ZIP structure), with what a shared checker cannot catch. The strongest claim in the old
  text — "verification never relies on the code that produced the candidate" — was true only for PNG and
  gzip, and is now replaced by the table.
- [size-budget](docs/size-budget.md#3-feature-matrix) lists the feature set as `Cargo.toml` declares it and
  the measured size of each combination; [dependencies](docs/dependencies.md) says that all four crates are
  behind features.
- [safety](docs/safety.md#3-metadata) and the README now separate **content** metadata (preserved, or
  removed on request) from **filesystem** metadata (mode and times preserved; owner, ACLs and extended
  attributes not), because conflating them is how a user loses an ACL.

## [0.5.0] - 2026-10-24 — the file is the command

### Changed — breaking

- **No more subcommand.** `tzim optimize <PATH>` becomes `tzim <PATH>`; the path is the whole command, and
  files, directories and duplicates may still be mixed in one run. The old spelling keeps working — a
  leading `optimize` is stripped before parsing — and so does `tzim formats`, but neither is documented
  outside this entry. Scripts written against 0.1–0.4 do not need editing.
- **`--help` no longer lists verbs.** The usage block is
  `tzim [OPTIONS] <PATH>...` plus the four information commands, and a new `INFORMATION:` section groups
  them.

### Added

- **`-i` / `--info`**: version, the promise in four lines, the formats in this build, G1/G3, homepage,
  docs, licence and contact address, printed from the same tables `--list-formats` uses and exiting `0`
  without touching a single path.
- **`--list-formats`**: the `formats` verb as a flag, so every non-processing action has the same shape.
- **`--check-deps`**: states that there are no external programs at run time, and what the binary is
  statically linked against. It exists because tools that wrap ffmpeg and friends have taught people to
  ask; the honest answer is short.

### Packaging

- **Intel macOS artifact** (`tzim-x86_64-apple-darwin.tar.gz`). The crate is pure Rust, so it cross-compiles
  on the same Apple silicon runner; `install.sh` and `docs/releases.md` pick it up.
- The release smoke test accepts both `--list-formats` and the older `formats`, so tags before 0.5 still
  pass it.
- `Formula/tzim.rb` and `packaging/winget/0.5.0/` are pinned to the published v0.5.0 artifacts and their
  SHA-256, taken from the release notes after the build. The formula now covers Intel as well as Apple
  silicon macOS, and its caveats and test use `--list-formats`.

### Documentation

- `README.md` rewritten around the new shape: contents table, one section per install channel,
  `tzim --check-deps` output, the full flag list in a `<details>`, the three guarantees as a table, the
  format matrix, and a "what tzim will never do" list. Every example is `tzim <path>`.
- `docs/cli.md` documents the path-first grammar, the information commands and their exit codes.

## [0.4.3] - 2026-10-24 — checksums where they can be read

### Added

- **Homebrew formula** (`Formula/tzim.rb`) for the third-party tap, pinned per platform to the published
  artifact and its SHA-256. It unpacks the release, so the tap builds nothing.
- **winget manifest** (`packaging/winget/0.4.3/`) for Windows x64: a `zip` installer with a portable
  nested `tzim.exe`, nothing written outside the package store.
- The README now leads with the install channels and carries the contact address.

### Changed

- The release workflow writes the `SHA256SUMS` block into the release notes and into a check annotation,
  replacing any previous block instead of appending. The asset alone is not reachable from the release
  page, and "copy the hash into the formula" should not need a download of the file being described.

### Fixed

- v0.4.2's notes were built by the previous workflow and carry no checksum block; the assets and
  `SHA256SUMS` were correct. Nothing else differs.

## [0.4.2] - 2026-10-24 — install from a package manager

### Added

- **`install.sh`**: `curl -fsSL .../main/install.sh | sh` takes the prebuilt binary for the platform into
  `~/.local/bin`, and falls back to `cargo install --git` where no build is published. `TZIM_REPO`,
  `TZIM_VERSION`, `TZIM_INSTALL_DIR` and `TZIM_BRANCH` override its choices.
- **Debian and Fedora packages**: `tzim_amd64.deb` and `tzim.x86_64.rpm`, built by
  `packaging/linux-packages.sh` from the same static musl tarball, with no runtime dependency declared.
- **Homebrew formula** (`Formula/tzim.rb`) for a third-party tap, and a **winget manifest**
  (`packaging/winget/`) for Windows x64.

### Changed

- Release assets are named after the Rust target triple and **carry no version**
  (`tzim-x86_64-unknown-linux-musl.tar.gz` instead of `tzim-v0.4.1-linux-x86_64.tar.gz`), so
  `releases/latest/download/<asset>` is a stable URL for the installers.
- The release workflow now also builds the two Linux packages and folds them into `SHA256SUMS`.

### Fixed

- Nothing user-visible. The binary content is unchanged from 0.4.1.

## [0.4.1] - 2026-10-24 — the evidence behind the claims, no functional change

### Added

- **A real-file corpus in CI** (`corpus.yml`): four pinned upstream archives — libpng 1.6.44,
  libjpeg-turbo 3.1.0, image-rs 0.25.6, Pillow 11.3.0 — each verified by SHA-256 and assembled into 572
  images plus four gzip and ZIP containers built from them. The binary runs over the tree twice; the job
  fails on any `failed` status that is not a known-corrupt upstream fixture, on any `verify` or `internal`
  error, and on any file still being rewritten on the second pass.
- **Fuzz targets** (`fuzz/`, `cargo-fuzz`, nightly): `jpeg_decode` covers the marker parser, the layout
  analysis and the entropy decoder; `roundtrip` covers detection, the handler and verification for every
  format. Sixty seconds per target on pull requests that touch the parsers, longer before a release.
- **Comparative benchmarks** (`bench.yml`) against oxipng 10.2.1, optipng 0.7.8 and jpegtran (libjpeg-turbo
  2.1.5) on copies of the same corpus, reporting wall clock, peak RSS and bytes saved.
- The format handlers are a library now (`src/lib.rs`), so the fuzz targets can reach the parsers. The entry
  points exist only behind the `fuzz-api` feature, which no release build enables.

### Changed

- `docs/benchmarks.md` §3 reports measured results instead of "not measured yet", and §4 shrinks to the two
  gaps that are still real.
- Release, size and benchmark builds use default features. `--all-features` would compile the fuzz entry
  points into the binary that is measured and shipped.
- Measured static binary: 980 KiB (+4 KiB, a linker page boundary, not new behaviour). Still four crates.

### Fixed

- Nothing user-visible. No format handler changed, and no file that was optimized by 0.4.0 is treated
  differently by 0.4.1.

## [0.4.0] - 2026-10-24 — first published release

### Added

- **JPEG (opt-in, `--formats jpeg`).** Entropy-coded data is decoded to DCT coefficients and re-encoded with
  optimal Huffman tables (Annex K.2, lengths limited to 16 bits, all-ones code excluded). No coefficient
  changes, so the decoded image is bit-identical. Every other marker — Exif, ICC, `COM`, `DQT`, `SOF0`,
  `DRI`, the `SOS` header and any bytes after `EOI` — is copied as a raw segment.
- Baseline sequential only: progressive, arithmetic-coded, lossless, hierarchical, 12-bit, SOF1 and
  multi-scan files are refused (`skipped`, exit 0) with a reason. Restart intervals are supported and
  preserved, including the `RSTn` markers inside the scan.
- Verification compares **every DCT coefficient** of every component, plus the ordered marker sequence,
  the frame header, unused Huffman tables and the trailing bytes. Coefficient identity implies pixel
  identity, so no second decoder is involved.
- The Huffman table generator is covered by a 4 000-case property test that validates every table it
  produces (no overfull code space, no duplicates, no all-ones code, every used symbol encodable).
- `--formats <LIST>`: an allowlist over `png`, `gzip`, `zip`, `ico`, `jpeg`. JPEG is off unless named;
  nested children inherit the same allowlist.
- Nine integration tests for JPEG and one for the allowlist.

### Changed

- Documentation: `docs/guarantees.md` §4.7 rewritten as the actual contract, `docs/dependencies.md` §2.6
  records why this codec is hand-written instead of delegated to a crate.
- No new dependencies and no `unsafe`; the dependency count stays at four. Measured static binary size:
  976 KiB (+72 KiB over v0.3).

## [0.3.0] - development milestone, shipped in 0.4.0

### Added

- **APNG.** Every frame is recompressed. The recompressed stream is split back into the same number of
  `fdAT` chunks, each keeping its original sequence number, so `fcTL` payloads and the animation sequence
  stay byte-identical; a frame that cannot fill its chunks is left untouched. Verification decodes and
  compares every frame, including the frame control data.
- **ICO/CUR.** Embedded PNG images are optimized; bitmap entries are copied byte for byte. The directory is
  parsed strictly: overlapping images, images outside the file, and unreferenced trailing data are refused.
  Only the size and offset fields of moved images change.
- **`--nested`** (off by default) with **`--max-depth`** (default 3): supported files stored inside ZIP
  entries are optimized, verified against their own contract, and used only when the entry gets smaller.
  A failing or unprofitable child keeps the original bytes and never fails the parent archive.
- ZIP verification accepts a changed entry CRC and uncompressed size only for a verified nested rewrite;
  this is the single documented exception to the preservation rule, and it is spelled out in
  `docs/guarantees.md` §4.6.
- 9 integration tests for APNG, ICO/CUR, nested archives and the depth limit; new fixture builders for
  APNG frames and ICO files.

### Changed

- Handlers now take bytes rather than paths, so the pipeline reads each file once and nested processing is
  possible without a second read.
- Measured static binary size: 904 KiB (+36 KiB over v0.2, still no new dependencies).

## [0.2.0] - development milestone, shipped in 0.4.0

### Added

- **ZIP family.** Surgical rewriter: local headers, central directory records, extra fields, names,
  comments, flags, attributes and timestamps are copied and only the compressed size and offset fields are
  patched. Only DEFLATE entries are recompressed, per entry, keeping whichever is smaller; stored entries
  stay stored, so ODF/EPUB `mimetype` requirements hold.
- Data descriptors (bit 3) are validated against the central directory and rewritten with the new size.
- Refused, with the file left untouched: signed archives (JAR/APK signing block, `META-INF` signatures,
  signed ODF `documentsignatures.xml`, `_xmlsignatures/`, OPC digital signatures), ZIP64, multi-disk,
  encrypted archives, unknown compression methods and prepended stubs.
- `--max-uncompressed` (default 8 GiB) and `--max-ratio` (default 1000:1) guard against decompression bombs.
- New error class `refused`: a valid input that policy declines is reported as `skipped` and the run exits
  `0`, while corrupt inputs remain `failed` with exit `1`. The JSON `kind` field gained `refused`.
- 5 unit tests and 14 integration tests for ZIP, with a fixture builder covering deflate, stored entries,
  data descriptors, extra fields, comments and non-UTF-8 names.

### Changed

- gzip: an unsupported compression method and multi-member files are now reported as `refused` (skipped,
  exit `0`) instead of failures.
- Measured static binary size: 868 KiB (+36 KiB over v0.1, no new dependencies).

## [0.1.0] - development milestone, shipped in 0.4.0

### Added

- **PNG handler.** IDAT recompression with `zlib -9`, scanline filter re-selection for non-interlaced,
  non-animated images, optional Zopfli. Every non-IDAT chunk is copied byte for byte, including unknown and
  private chunks. Verification re-parses the candidate (all chunk CRCs) and decodes both files row by row
  with the independent `png` crate.
- **gzip handler.** The header (`FEXTRA`, `FNAME`, `FCOMMENT`, `FHCRC`, `MTIME`, `XFL`, `OS`) is preserved
  verbatim; only the DEFLATE body is rewritten and the trailer is copied after proving it matches.
  Multi-member files and trailing data are refused.
- **Safe replacement.** Temporary file created next to the destination with `create_new`, RAII cleanup,
  permission and timestamp preservation, `fsync`, single `rename`, directory `fsync` on Unix, and detection
  of concurrent modification before the swap.
- **CLI.** `optimize`, `formats`, `--help`, `--version`; multiple files, directories, `--recursive`,
  `--dry-run`, `--jobs`, `--json`, `--quiet`, `--fail-fast`, `--include`, `--exclude`, `--min-size`,
  `--max-size`, `--max-pixels`, `--zopfli`, `--follow-symlinks`, `--allow-hardlinks`,
  `--allow-root-ownership-change`, `--no-fsync`. Exit codes `0`–`4`.
- Deterministic output ordering, de-duplication by `(device, inode)`, symlink and hard-link refusal.
- 41 unit tests and 21 integration tests; CI on Linux, macOS and Windows; clippy `-D warnings`; static musl
  build with a measured size of 832 KiB and 14 dependency lines.

## [Unreleased]

### Added

- Project README with the lossless invariant, guarantee classes and format matrix.
- `docs/guarantees.md`: normative contracts for PNG (v0.1), gzip (v0.1), ZIP (v0.2), ICO, APNG, nested
  containers (v0.3) and JPEG (v0.4), plus the list of properties that are not preserved.
- `docs/formats.md`: detection rules, format matrix, admission criteria and refused formats with reasoning.
- `docs/cli.md`: command-line specification, exit codes, human and JSON output.
- `docs/safety.md`: the file replacement algorithm, temporary-file policy, metadata table, atomicity and
  failure modes.
- `docs/dependencies.md`: four direct dependencies (one optional), rejected alternatives, licence and supply
  chain policy.
- `docs/size-budget.md`: provisional budgets, release profile, feature matrix, measurement procedure.
- `docs/testing.md`: invariants, contract-to-test traceability, fixture corpus, fuzzing and CI plan.
- `docs/roadmap.md`: releases v0.1–v0.5 with gates, estimates and risks.
- `CONTRIBUTING.md` and issue templates.
