# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

**v0.4.0 was the first published release.** Versions 0.1 to 0.3 were development milestones: they exist as
commits, were tested in CI, and were never tagged or packaged. Everything in them is contained in 0.4.0.
Planned releases are described in [docs/roadmap.md](docs/roadmap.md); the packaging policy is in
[docs/releases.md](docs/releases.md).

## [Unreleased]

Documentation only: no handler changed, no file is treated differently, and the binary is the same. What
changed is that several documents stop claiming more than the code does.

### Fixed

- **`docs/safety.md` §1 no longer describes a snapshot "from the open handle".** The snapshot comes from
  `lstat`, and a file is opened twice — once to detect the format, once to read it — not once. §2 now gives
  the real temporary-file name (`.tzim-<pid>-<nanos>-<attempt>.tmp`) and says that a successful run renames
  the candidate instead of deleting it, and §8 describes the ownership rule as it is: a file owned by another
  user is skipped with a reason, on Unix only, and nothing is "warned" about root.
- **`docs/guarantees.md` G1 is no longer "verified by a content hash".** No hash exists in the program; G1
  holds because nothing is written on those paths, and the only comparison made is the re-check of
  `(device, inode, length, mtime)` before replacement. G3 clause 4 now attributes the size rule to the place
  that enforces it — the handler and the pipeline — instead of implying that `verify()` compares sizes.
- **The TIFF `--strip-metadata` list now exists.** `guarantees.md` §2.7 pointed at §4.8 for a list that was
  not there; §4.8 now carries both lists it referred to: the tags whose out-of-line values may be moved, and
  the exact tags the flag removes. `docs/cli.md` gains `Artist` (315) and states that GDAL nodata (`42113`) is
  kept.
- **`guarantees.md` §2.6 has a TIFF row.** The independence table listed six formats and silently omitted the
  seventh, and the prose called JPEG "the weakest of the five". TIFF's row states what is true: independent
  for strips that were uncompressed, a shared inflater for strips that were already Deflate, and no external
  checksum to anchor the original side.
- **`docs/cli.md` matches the output.** The human example shows `unsupported: unsupported format` (the status,
  then the note), the JSON example shows `"status":"unsupported"` with `"error":null`, the format ids are the
  ones `Format::id()` emits (no `apng`, `zlib` included), `--max-pixels` is marked as applying to PNG and
  JPEG, and the symlink section says what really protects a walk: a 64-level depth limit, not a visited set of
  directories.
- **`docs/testing.md` stops promising mechanisms that do not exist.** The failure-injection hooks
  (`TZIM_TEST_*`), the `proptest` suite, the committed `tests/fixtures/` corpus and the "refused set"
  baseline are gone; the fixture section describes the builders in `tests/common/mod.rs` and what each one
  actually covers, and the filesystem table is split into what a test covers and what it does not. The fuzz
  section says that `tiff` and `zlib` are not in the fuzz build and that the `strip` target has no seeds.
- **Eight broken internal links and anchors are fixed** (`CHANGELOG.md`, `docs/flac-audit.md`,
  `docs/roadmap.md`), including one that the 0.11.0 release notes introduced themselves, and `CONTRIBUTING.md`
  no longer names a `png_reader` fuzz target or a `tests/fixtures/generate/` directory that do not exist.
- **`docs/roadmap.md` marks the v0.1 numbers as history** instead of letting "41 unit and 21 integration
  tests" and "832 KiB" read as the current state.

## [0.11.0] - 2026-10-25 — distribution

No handler changed and no file is treated differently. This release is about the three minutes between
`curl` and a working tool: what the installer checks, what the packages contain, and which architectures
exist at all.

### Added

- **A Linux ARM64 binary** (`aarch64-unknown-linux-musl`), cross-linked on an x86_64 runner with the
  toolchain's own `rust-lld` and the self-contained musl libc that ships with `rustc`, so no cross C
  toolchain is involved. ARM64 servers and single-board computers now install a binary instead of compiling
  one, and `tzim_arm64.deb` / `tzim.aarch64.rpm` exist next to the x86_64 packages.
- **A manual page** (`man/tzim.1`) and **completion for bash, zsh and fish** (`completions/`), installed by
  the `.deb`, the `.rpm` and the Homebrew formula and shipped inside the POSIX archives. `man tzim` and tab
  completion now work after a package install.
- **`TZIM_BASE_URL`** in `install.sh`, for a mirror. The checksum check applies there too.
- **A `package` CI job**: `cargo package --locked`, then a build from the packaged tarball — the crate as
  crates.io would receive it, so `cargo install tzim` is one `cargo publish` away rather than a rewrite.
- **An `installer` CI job**: runs `install.sh` against the latest published release on Linux and macOS, then
  serves a doctored archive with one byte appended next to the release's real `SHA256SUMS` and asserts that
  the install aborts, says `SHA-256 mismatch`, and leaves nothing behind.

### Changed

- **The winget manifest is checked against the release, not just against a schema.** `winget validate` reads
  the manifest as a document; `packaging/winget/verify.sh` downloads the archive it names and compares the
  hash, which is the half that decides whether an install works. A new `winget` CI job on Windows runs both,
  and `docs/releases.md` §5 has the exact steps for submitting to `microsoft/winget-pkgs`, since that pull
  request is against a repository this project does not own.
- **`install.sh` verifies what it downloads.** The archive is checked against `SHA256SUMS` before it is
  unpacked; a mismatch prints both hashes and exits instead of falling back to a source build that would hide
  it. The installed binary is run once before success is reported, and with `TZIM_VERSION` set the reported
  version has to match. A `curl | sh` installer that trusts the network more than the project's own checksums
  is a strange thing to ship next to this program.
- **The release archives carry the man page and the completions** next to the binary, so the formula, the
  packages and the repository install the same text.
- **`install.sh` covers Linux ARM64 and Intel macOS**, and names every platform the release matrix builds.
- **`tests/packaging.rs`** fails when an option in `--help` is missing from the man page or any completion,
  when the man page's version drifts from the crate, or when a release target is not fetchable by the
  installer.
- The `.deb` and `.rpm` description now lists the formats the binary actually has: it had been stuck at the
  v0.4 set, without TIFF and without zlib.

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.11.0/` are pinned to the published v0.11.0 artifacts and their
  SHA-256, copied from the release notes after the build. The formula gained the two platforms this release
  made possible: Intel macOS and Linux ARM64.

### Size

- **+0 bytes.** 1 049 440 bytes (1 024 KiB), the same number as v0.10.0: this release adds packaging, a
  manual page and an architecture that is built from the same source. One feature row moved by 4 KiB because
  the linker crossed a page boundary, which is recorded in [size-budget §3](docs/size-budget.md#3-feature-matrix)
  rather than smoothed over.

### Fixed

- **`--strip-metadata` was accepted by the parser and missing from `--help`** since v0.6. It is documented
  now, and the completions and the man page list it.

## [0.10.0] - 2026-10-25 — the claims around the code

No handler changed and no file is treated differently: the corpus writes the same total, and every contract
and refusal is as it was. This release is about the sentences around the code that had stopped being true —
in CI, in the benchmarks, in the README, and on the progress line.

### Added

- **`indicatif` 0.18 draws the progress line**, at the user's request and for the first time since v0.1 with a
  new dependency. One file gets `⠋ converting…`; a batch gets `⠋ 00:01 [████░░░░] 3/12` — the clock, an
  eight-cell bar and the position. Still stderr only, still nothing in a pipe, still absent with `--quiet`,
  `--json` and `--no-progress`, and still erased before the report prints. The templates, the `mm:ss` clock
  and the 150 ms delay stay in `src/progress.rs`; the animation thread and the terminal handling do not.
  [cli §10](docs/cli.md#10-colour-and-progress) has the shapes,
  [dependencies §2.9](docs/dependencies.md#29-indicatif--the-progress-line) argues the crate, its eight new
  lock entries and the `unsafe` surface that arrives with them.
- **`msrv`, `fmt`, `deny` and `audit` jobs.** `docs/testing.md` §4 has listed eleven CI jobs since v0.1 and
  the workflow contained three. The missing gates exist now: 1.88.0 compilation, `cargo fmt`, `cargo deny`
  against a new `deny.toml` that carries the licence allow-list, and `cargo audit` against RustSec.
  [`THIRD-PARTY.md`](THIRD-PARTY.md) is the notice file §5 of the dependency policy already referred to.
- **TIFF and zlib rows in the comparative benchmarks**, with a note in the table itself that neither has a
  reference tool that recompresses like this one does.

### Changed

- **Every CI build runs with `--locked`**, which [dependencies §6](docs/dependencies.md#6-supply-chain) has
  been promising since the first release. `Cargo.lock` is written by the `lockfile` job, whose `git push`
  could never have worked — `actions/checkout` leaves a detached HEAD. It now checks out the branch, pushes
  with an explicit refspec, runs on every branch, and shares a concurrency group with `fmt`.
- **`rustfmt` runs over this tree for the first time.** The environment the project is written in cannot run
  it, so the `fmt` job formats and commits, and fails on the run that had to change something. Two jobs now
  write to the branch; [testing §4.1](docs/testing.md#42-the-two-jobs-that-write-to-the-branch) says why and
  what happens on a fork.
- **The comparative benchmarks are this release's.** §3.2 of [benchmarks](docs/benchmarks.md) carried v0.4.0
  numbers taken before the `-Os`/`-O3` profile split, with a paragraph explaining why they were not re-taken.
  They are re-taken, and the run id is recorded next to them.
- **The README fits on one screen**: badges, the promise, the before/after, the format table, and the pitch —
  contract and refusal — where a stranger reads it first. The depth stays in `docs/`.

### Size

- **+96 KiB** (928 → 1 024 KiB), and none of it is a handler. `indicatif` and `console` cost 112 KiB at the
  wildcard `opt-level = 3` and 96 KiB at `"s"`, which is what §2 of the size budget now says; the zlib handler
  meanwhile shrank into a page boundary and now measures 0 KiB. The budget follows the documented rule
  (measured + 15 %): 1 069 → 1 179 KiB. Numbers in
  [benchmarks §1](docs/benchmarks.md#1-binary-size--measured), per-feature table in
  [size-budget §3](docs/size-budget.md#3-feature-matrix).

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.10.0/` are pinned to the published v0.10.0 artifacts and their
  SHA-256, copied from the release notes after the build.
- The release build runs with `--locked` too: the published bytes come from the dependency set that was
  reviewed, not from whatever resolves on the day of the release.

### Fixed

- **The `musl` check accepted only the older `file` wording.** The musl target now links position-independent
  and `file` reports `static-pie linked`; the gate called a perfectly static binary dynamic.

## [0.9.0] - 2026-10-25 — zlib, and a format queue that closes

### Added

- **zlib (RFC 1950)** — the DEFLATE stream is recompressed, the two header bytes and the four Adler-32 bytes
  are copied verbatim, and the contract is [guarantees §4.10](docs/guarantees.md#410-zlib-rfc-1950--v09).
  No new dependency: `flate2` was already in the tree and the checksum is twelve lines of arithmetic.
  Refused with a reason: `CM ≠ 8`, a declared window below 32 KiB (our encoder may exceed it), a preset
  dictionary (the payload cannot be decoded without bytes that are not in the file), and trailing data.

### Changed

- **FLAC is refused after measurement**, not deferred. `libFLAC` at every level on three deterministic
  signals: a stronger encoder buys **0.38 %** on musical material, 1.6 % on a pure tone, and **0 %** on files
  already written at `flac -8`; the encoder plus an independent decoder would cost roughly **150–300 KiB** of
  a 920 KiB binary. The contract stays on record, the code does not get written, and
  [docs/flac-audit.md §7](docs/flac-audit.md#7-decision-refused-and-the-measurement-behind-it) has the table.
- **Raw DEFLATE is refused**, not deferred: a bare stream has no header, no trailer and no signature, so
  recognising one would mean guessing.
- The format queue in [docs/roadmap.md](docs/roadmap.md) is now empty, and every remaining candidate is
  recorded with the reason it is not there.

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.9.0/` are pinned to the published v0.9.0 artifacts and their
  SHA-256, copied from the release notes after the build.

### Size

- **+8 KiB** (920 → 928 KiB), of which 4 KiB is the zlib handler itself — the cheapest format in the table —
  and the rest is a page boundary. The budget follows the documented rule (measured + 15 %): 1 059 → 1 069 KiB.

## [0.8.0] - 2026-10-25 — the terminal, and a contract for FLAC

No output byte changed and no file is treated differently: this release is the interface around the same
work. The corpus still writes 379 177 bytes.

### Added

- **Colour**, on a terminal only: the status word, the saving percentage and the two summary numbers.
  Controlled by `--color=auto|always|never` and `--no-color`, and following `NO_COLOR`, `CLICOLOR=0`,
  `CLICOLOR_FORCE` and `TERM=dumb`. `--json` is never coloured. Windows terminals that cannot interpret
  ANSI sequences are detected and left alone.
- **A progress line** on stderr for runs that take longer than a moment: ASCII frames, the file counter,
  the clock and the running total. Drawn only when stderr is a terminal; `--no-progress` turns it off.
- **The FLAC contract** ([guarantees §4.9](docs/guarantees.md#49-flac--refused-after-measurement)),
  written before any code as every contract here is, with the audit behind it in
  [docs/flac-audit.md](docs/flac-audit.md). No FLAC support is claimed: the handler and its tests do not
  exist, and the implementation is blocked on three crates that have not been approved.
- [cli §10](docs/cli.md#10-colour-and-progress) and [cli §11](docs/cli.md#11-conformance-with-cli-conventions):
  the colour rules, and an audit against the CLI guidelines that found four defects — all fixed below.

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.8.0/` are pinned to the published v0.8.0 artifacts and their
  SHA-256, copied from the release notes after the build.

### Size

- The binary grows by **20 480 bytes** (900 → 920 KiB) in every build: colour, the progress thread, buffered
  reporting and `IsTerminal`. Handlers are unchanged, and the round of size work in 0.7.1 is what pays for it.
  The budget in [size-budget §1](docs/size-budget.md#1-budget) is re-measured, not widened by hand.

### Fixed

- **A closed pipe no longer aborts the process.** `tzim . -r | head -1` used to fail with `EPIPE` inside
  `println!` and, under `panic = "abort"`, kill the process with an unhelpful message. The report now writes
  to a sink, and a closed stdout ends it quietly with the exit code the run had earned.
- **Clustered short flags and attached values** (`-rq`, `-nr`, `-j4`) are accepted, as every getopt-based
  tool accepts them; before they were rejected as unknown options.
- **Usage errors print the usage line**, not only a hint to read `--help`.
- **`--help` matches the binary**: the `--formats` list now names `tiff`, and the colour, progress and
  command-line convention sections are part of it.

### Documentation

- The example in [cli §3](docs/cli.md#3-human-output) showed one decimal and a percentage in the summary,
  neither of which the program has ever printed. The text now matches the binary.
- [dependencies §2.7–2.8](docs/dependencies.md#27-flac-the-crates-it-would-have-needed)
  record the FLAC crates that are not added and why colour and progress are not dependencies.

## [0.7.1] - 2026-10-25 — the same binary, 124 KiB lighter

No format, flag, guarantee or output byte changed. The corpus writes 379 177 bytes in
both 0.7.0 and 0.7.1 builds.

### Changed

- **Release profile**: this crate is compiled with `opt-level = "s"`, its dependencies — DEFLATE, PNG,
  Zopfli, where the time is spent — stay at `3`. Measured on 694 real files: −104 KiB for +3.6 % runtime.
  Optimising the whole tree instead would have saved 124 KiB and cost 11 %. Numbers and rejected variants
  (thin LTO, `opt-level = "z"`, `codegen-units = 16`, `gc-sections`, `force-unwind-tables=no`) are recorded in
  [size budget §5](docs/size-budget.md#5-options-considered).
- **`src/report.rs`** formats sizes, percentages and durations with integer arithmetic instead of the
  general `core::fmt` path: −20 KiB, byte-identical output, with boundary tests on the unit thresholds.
- **Size budget** tightened from 1 178 KiB to 1 036 KiB, so CI refuses growth 142 KiB earlier.

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.7.1/` are pinned to the published v0.7.1 artifacts and their
  SHA-256, copied from the release notes after the build.

## [0.7.0] - 2026-10-25 — TIFF, and a rebuild that can prove itself

### Added

- **TIFF**: classic TIFF (`II`/`MM`, magic `42`) with strips. Uncompressed strips are compressed with
  Deflate; Deflate strips are re-compressed and keep whichever of the two results is smaller. The contract
  is [guarantees §4.8](docs/guarantees.md#48-tiff-classic-baseline-strips--v07); it was written before the
  handler, as every format contract is.
- The file is **rebuilt** rather than patched — a TIFF stores absolute offsets, so a smaller strip moves
  everything after it. Verification then compares what the rebuild could break: every entry as a resolved
  *value* (not as bytes at a fixed position), the tag set, the entry order, the byte order, the strip count,
  and every strip decoded through `flate2` and compared byte for byte. `Compression` is the only value that
  may change, and only across the two transitions that stay lossless.
- `tiff` joins the feature list. It reuses `flate2`, so it adds **no dependency** — only code.

### Changed

- Refusals are part of the format's value, so they are specific: BigTIFF, tiled layouts, more than one IFD,
  separate planes, predictors, `SubIFDs`/EXIF/GPS sub-IFDs, and unknown *out-of-line* tags (which may be
  offsets) are skipped with a reason and the file is left alone. LZW is refused instead of being written by
  an encoder this repository would have to invent and could not verify. Unknown tags that fit inside the
  entry are copied untouched.

### Packaging

- `Formula/tzim.rb` and `packaging/winget/0.7.0/` are pinned to the published v0.7.0 artifacts and their
  SHA-256, copied from the release notes after the build — the order that never publishes a hash for a file
  that does not exist yet.

### Documentation

- [formats](docs/formats.md#2-format-matrix) moves TIFF from *research* to *implemented* in the same commit
  as the tests, and [roadmap](docs/roadmap.md) records what shipped; the research queue now starts at v0.8.
- The honest gain is stated next to the claim: 5–30 % on uncompressed scans, 0–5 % on already-Deflate files,
  and nothing at all for LZW — which is why the handler refuses it rather than pretending.

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
