# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The v0.1 code is complete and tested in CI, but no release artifacts have been published yet. Planned
releases are described in [docs/roadmap.md](docs/roadmap.md).

## [0.2.0] - unreleased

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

## [0.1.0] - unreleased

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
