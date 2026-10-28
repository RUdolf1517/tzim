# Testing

Related: [guarantees](guarantees.md) · [formats](formats.md) · [safety](safety.md)

A file optimizer is trusted or it is worthless. Tests are therefore not coverage theatre: the failure modes
that damage user data are tested explicitly, and where a clause has no check, this document says so instead of
implying one exists. There is no property-testing framework and no failure-injection harness here; what exists
is 101 unit tests, 89 integration tests and 10 packaging tests, each asserting a case someone thought of. A
green suite means those cases hold, not that the contract holds.

---

## 1. Invariants under test

These hold for every format and are tested as properties, not as anecdotes.

| ID | Invariant | How it is tested |
| --- | --- | --- |
| I1 | The original is never modified unless the candidate passes all checks | every failure test reads the original back and compares it byte for byte; there is no per-stage injection, so a stage cannot be forced to fail on demand |
| I2 | A candidate is only used if strictly smaller | fixtures whose best candidate is larger (`already_optimal_zip_is_left_alone`, `tiff_already_deflated_small_file_is_left_alone`, `ico_with_only_bitmaps_is_already_optimal`); a candidate of **exactly** equal size has no test, and currently fails — see B1 in the 0.11.0 audit and issue #26 |
| I3 | No metadata is removed | per-format inventory comparison, including unknown/private chunks and extra fields |
| I4 | No lossy or converting transformation exists in the codebase | review gate; no such handler may exist, so there is nothing to test |
| I5 | Concurrent modification aborts the operation | **no test.** The `(device, inode, length, mtime)` re-check in `replace.rs` is exercised only by its unit test `snapshot_roundtrip_detects_changes`; nothing modifies a file mid-run |
| I6 | The release-default JPEG V1 candidate is checked by an independent decoder | `independent_oracle_rejects_a_symmetric_huffman_mapping_negative_control` uses a DHT-symbol permutation whose same-reader/writer model agrees on coefficients but the independent decoder sees a different pixel image; it runs in default and all-features CI |
| I7 | Replacement is atomic where the platform allows | `guard_removes_uncommitted_file` shows the guard deletes an uncommitted candidate; nothing asserts that `rename` is the last fallible operation, and no test kills a run between `fsync` and `rename` |
| I8 | Unparseable input is refused, never repaired | targeted tests per format — `corrupt_png_is_refused_and_left_untouched`, `truncated_zip_is_refused`, `corrupt_entry_crc_is_refused`, `jpeg_truncated_file_fails_and_is_untouched`, `tiff_truncated_file_fails_and_is_untouched`, `a_corrupt_adler32_is_refused_and_the_file_is_untouched` — not a corpus |
| I9 | Presentation never changes results | `colour_is_absent_in_a_pipe_and_present_when_forced` and `json_is_never_coloured`; the comparison against the previous release's totals is **not** automated — the corpus keeps no baseline |

Idempotence is also a property: running Tzim twice on the same tree must produce zero changes the second
time.

---

## 2. Traceability

Each contract clause has an ID and the tables below state the assertion behind it. The IDs are the checklist
used in review; a format is not done until every row has a test that passes in CI. Where a row has a test, the
row names it. A row whose assertion has **no** test says so in the row itself rather than looking covered: the
table is evidence, not a plan.

Every row now names a test. The nine rows that said nothing in 0.11.0 were closed in two passes: six in
0.12.0 — `--zopfli` never worse than the default (`T-PNG-09`), gzip `FCOMMENT` (`T-GZ-01`) and `FHCRC`
(`T-GZ-02`), ZIP unknown methods and multi-disk archives (`T-ZIP-09`), `--max-uncompressed` (`T-ZIP-12`) and a
directory that runs past the end of an ICO (`T-ICO-04`) — and three in 0.12.1: an interlaced PNG
(`T-PNG-04`, which needed Adam7 in the fixture builder), an inconsistent frame sequence (`T-APNG-05`, which
needed the check to exist at all) and the nested size budget (`T-NEST-04`, removed — see below).

Two of the three needed code, not only a test. `PngSpec` could not write an interlaced file, so
"recompression only" was a claim about a path no fixture reached; it writes Adam7 now, and it applies the
scanline filter it is asked for instead of announcing it in the row's first byte. And the APNG frame sequence
was not validated anywhere in this program: the writer copies `fcTL`/`fdAT` number for number, and the `png`
crate was the only thing that refused a broken sequence — which is not a guard this program may rely on,
because a decode that stops on both sides of a comparison is indistinguishable from the end of the frame loop.
`png::check_frame_sequence` now refuses it before anything is written.

`T-NEST-04` is **deleted**, and the reason belongs where the row was. It asserted that "nested expansion
respects the parent's size budget", and there is no such budget: `zip::check_limits` refuses the archive when
the **sum** of the declared uncompressed sizes exceeds `--max-uncompressed`, before anything is decompressed,
and `zip::inflate_entry` refuses an entry whose data disagrees with its own declared size. A child handed to
`nested_child` is therefore never larger than the limit, and the per-entry check there is defence in depth that
cannot fire. Testing it would have meant calling a private function out of order — a test of the guard, not of
the behaviour. `docs/cli.md` and `guarantees.md` describe the limit that does exist.

### PNG

| ID | Assertion |
| --- | --- |
| `T-PNG-01` | All non-IDAT chunks (including an unknown/private chunk) are byte-identical, in the same order |
| `T-PNG-02` | IHDR payload is byte-identical |
| `T-PNG-03` | Decoded pixels are identical for the images the fixture builder produces: 8-bit RGBA, interlaced or not, with every scanline filter from 0 to 4 (`t_png_03_every_scanline_filter_round_trips`). Other colour types and bit depths are not tested — gray, gray+alpha, RGB, palette at 1/2/4/8 bits and 16-bit are rewritten by the corpus without complaint, but nothing re-decodes them. The filter unit test covers bytes-per-pixel 1 to 8 (`filters_roundtrip_for_all_types`), not only RGBA8's four |
| `T-PNG-04` | Interlaced images: recompression only, pixels identical — `t_png_04_an_interlaced_image_is_recompressed_and_pixels_are_identical` (Adam7 fixture, odd dimensions, every non-IDAT chunk byte-identical) |
| `T-PNG-05` | APNG: `acTL`/`fcTL`/`fdAT` untouched; default image pixels identical |
| `T-PNG-06` | Corrupted inputs (bad CRC, truncated, missing IEND, bytes after IEND) are refused |
| `T-PNG-07` | A candidate that is not smaller is discarded; file unchanged, status `already_optimal` |
| `T-PNG-08` | The output parses strictly and all chunk CRCs are valid |
| `T-PNG-09` | With `--zopfli`, the smallest of all candidates is chosen and verified — `t_png_09_zopfli_is_at_least_as_good_as_the_default` runs both and requires the strong one to be no larger |

### gzip

| ID | Assertion |
| --- | --- |
| `T-GZ-01` | Header preserved with `FEXTRA`, `FNAME`, `FCOMMENT` — `t_gz_01_a_comment_is_preserved` splices a comment in and compares the header byte for byte |
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
| `T-ZIP-09` | ZIP64, multi-disk, encrypted and unknown-method archives are refused — `t_zip_09_an_unknown_compression_method_is_refused` (method 12) and `t_zip_09_a_multi_disk_archive_is_refused` (a non-zero disk number) for the two that had no test |
| `T-ZIP-10` | Truncated archives and inconsistent local headers are refused |
| `T-ZIP-11` | Every entry's decompressed bytes and CRC are identical |
| `T-ZIP-12` | `--max-uncompressed` and `--max-ratio` are enforced (decompression-bomb guard) — `t_zip_12_max_uncompressed_is_enforced`, and the ratio limit in an earlier test |

### ICO / APNG / nested / JPEG

| ID | Assertion |
| --- | --- |
| `T-ICO-01` | Directory header and entry order preserved |
| `T-ICO-02` | Embedded PNGs satisfy the PNG contract and decode identically |
| `T-ICO-03` | BMP/DIB entries are byte-identical |
| `T-ICO-04` | Malformed directories and out-of-range offsets are refused — `t_ico_04_a_directory_that_runs_past_the_file_is_refused`. A declared count outside 1..=256 is never refused by the handler, because detection does not call such a file an ICO at all: it is reported `unsupported`, exit 0 |
| `T-APNG-01` | `acTL` payload preserved |
| `T-APNG-02` | Every `fcTL` preserved, in order |
| `T-APNG-03` | Every frame decodes identically (pixels and frame metadata) |
| `T-APNG-04` | The default image is optimized and verified |
| `T-APNG-05` | Inconsistent frame sequences are refused — `t_apng_05_an_inconsistent_frame_sequence_is_refused` (one `fdAT` renumbered, its CRC repaired so the refusal is about the sequence and not a checksum) |
| `T-NEST-01` | A child that fails verification is stored unmodified |
| `T-NEST-02` | Optimized children satisfy their own contracts |
| `T-NEST-03` | `--max-depth` is enforced |
| `T-JPG-01` | Marker inventory (type, order, payload) preserved; Exif and `COM` payloads survive |
| `T-JPG-02` | `DQT` and `SOF0` payloads byte-identical |
| `T-JPG-03` | Every DCT coefficient identical after the rewrite (stronger than comparing pixels) |
| `T-JPG-04` | Progressive, arithmetic, SOF1 and 12-bit inputs are refused, exit 0, file untouched |
| `T-JPG-05` | `DRI` and the restart markers in the scan are preserved, interval unchanged |
| `T-JPG-06` | The default V1 build optimizes eligible baseline JPEGs; `--formats png` narrows the allowlist and leaves JPEG byte-identical; codec-only builds without V1 keep JPEG opt-in |
| `T-JPG-07` | Truncated or corrupt inputs fail with exit 1 and the original is untouched |
| `T-JPG-08` | A non-smaller candidate is discarded; a second run reports `already optimal` |
| `T-JPG-09` | Bytes after `EOI` are preserved verbatim |
| `T-JPG-10` | The default V1 build's independent oracle rejects a symmetric Huffman-symbol-mapping negative control: a writer/reader that both reorder the DHT symbols would agree on coefficients, but the independent decoder reads the on-disk mapping and rejects the pixel change (`independent_oracle_rejects_a_symmetric_huffman_mapping_negative_control`)

### FITS interactive confirmation (#83)

| ID | Assertion |
| --- | --- |
| `T-FITS-C-01` | A direct single-file TTY invocation is the only prompt-eligible shape; `--jobs 4` remains synchronous, and quiet mode still prompts; JSON, dry-run, recursion, batch, nested, non-TTY and a narrowed `--formats` allowlist suppress the prompt (`only_a_single_direct_tty_fits_opt_in_is_prompt_eligible`) |
| `T-FITS-C-02` | The candidate is already staged and passes the ordinary FITS verifier before the confirmation callback; No leaves the original byte-identical and removes the temporary (`declining_verified_fits_candidate_preserves_original_and_cleans_temp`) |
| `T-FITS-C-03` | Only affirmative confirmation reaches atomic install; the installed file is the same verified candidate (`affirmative_confirmation_commits_only_the_verified_candidate`) |
| `T-FITS-C-04` | Unknown, refused checksum-bearing, corrupt, and safety-skipped hard-linked FITS inputs never reach the confirmation callback; originals stay byte-identical and staged files are cleaned (`unknown_fits_like_content_does_not_reach_confirmation`, `refused_checksummed_fits_does_not_reach_confirmation`, `corrupt_fits_header_does_not_reach_confirmation_and_cleans_temp`, `hardlinked_fits_is_skipped_without_confirmation_by_default`) |
| `T-FITS-C-05` | Only `y`/`yes` consents; empty/default, No, invalid input, EOF, closed stdin and prompt I/O failure all mean No (`src/confirmation.rs` unit tests) |

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
| `T-ZL-09` | a header that passes the container's check with no decodable data behind it is `unsupported`, exit `0`, untouched — and a stream that decoded bytes and then broke is still `failed`, exit `1` (`a_coincidental_zlib_header_is_unsupported_and_never_written`, `a_truncated_zlib_stream_is_still_a_failure`, plus the unit tests in `src/formats/zlib.rs`) |

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
| `T-TIF-07` | BigTIFF, tiles, multiple IFDs, planar `2`, predictors and `FillOrder 2` are refused with a reason, exit 0, file untouched |
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

### Filesystem, replacement and idempotence

These are the rows behind I1, I5 and I7 of [guarantees §3](guarantees.md#3-global-invariants), and behind
[safety](safety.md) §1 and §5. Each one names a test that exists today; the scenarios that have no test are in
the "not covered" table in §3 and in issue #35, not here.

| ID | Assertion | Test |
| --- | --- | --- |
| `T-FS-01` | Paths with spaces and non-ASCII characters are processed correctly | `unicode_and_space_filenames_work` |
| `T-FS-02` | A read-only file is skipped with a reason and is byte-identical afterwards | `read_only_file_is_skipped` |
| `T-FS-03` | One missing path among valid ones fails alone; the rest are processed; exit is `1` | `missing_path_fails_but_others_continue` |
| `T-FS-04` | A symlink to a file is skipped by default and its target is untouched | `symlinks_are_not_followed_by_default` |
| `T-FS-05` | The same file reached through three paths is processed once | `duplicates_are_processed_once` |
| `T-FS-06` | Mode bits and mtime survive the replacement | `permissions_and_mtime_survive_the_replacement` |
| `T-FS-07` | A second run over the same tree changes nothing | `second_run_is_a_no_op` |
| `T-FS-08` | A directory is walked recursively when asked, and only then | `directory_is_processed_recursively` |
| `T-FS-09` | A directory argument without `-r` stays at the top level | `a_directory_argument_without_recursive_stays_at_the_top_level` |
| `T-FS-10` | `--dry-run` writes nothing and leaves no temporary file behind | `dry_run_does_not_modify_anything` |
| `T-FS-11` | A FIFO is skipped as "not a regular file" and is never opened for writing (its mtime is unchanged) | `t_fs_11_a_fifo_is_skipped_and_never_opened` |
| `T-FS-12` | A character device (`/dev/null`) is skipped, not read | `t_fs_12_a_device_node_is_skipped` |
| `T-FS-13` | A full disk is `failed` with the original intact and no `.tzim-*.tmp` left behind. Gated on `TZIM_TEST_FULL_DISK`, which CI points at a 2 MiB tmpfs; without it the test prints that it was skipped | `t_fs_13_a_full_disk_is_a_failure_with_nothing_left_behind` |
| `T-FS-14` | A symlink cycle under `--follow-symlinks` terminates and is reported: the kernel's symlink-hop limit (ELOOP) stops it long before the 64-level guard, the path is `failed`, exit `1`, and the rest of the run is unaffected | `t_fs_14_a_symlink_cycle_terminates_and_is_reported` |
| `T-FS-15` | A directory the process cannot write to is `failed` with the original intact; skipped-with-a-reason if the process is root, because 0555 would not bind | `t_fs_15_a_read_only_directory_is_a_failure_and_the_file_survives` |
| `T-FS-16` | A tree deeper than 64 levels is refused at the limit while the files above it are still processed | `t_fs_16_a_tree_deeper_than_sixty_four_levels_is_refused` |

### `--strip-metadata`

One row per handler that has the flag's behaviour tested. The tag, chunk and field lists themselves are
normative in [guarantees §2.7](guarantees.md#27-g4--metadata-removed-on-request); these rows are about the
observable result.

| ID | Assertion | Test |
| --- | --- | --- |
| `T-STRIP-01` | PNG: `tEXt` and `tIME` removed, `eXIf` removed, pixels identical, an unknown private chunk kept | `strip_metadata_removes_png_text_and_exif_but_keeps_the_content` |
| `T-STRIP-02` | gzip: header reduced to the stripped form, payload identical after decoding | `strip_metadata_clears_the_gzip_header_and_keeps_the_payload` |
| `T-STRIP-03` | JPEG: `APP1` and `COM` removed, `APP0` kept, coefficients unchanged | `strip_metadata_drops_jpeg_exif_and_comment` |
| `T-STRIP-04` | ZIP: entry and archive comments removed, extra fields and payload byte-identical | `strip_metadata_drops_zip_comments_and_keeps_extra_fields` |
| `T-STRIP-05` | The removal is reported — `metadata stripped` in text, `metadata_stripped` in the record, `strip_metadata` in the summary — and `--dry-run` writes nothing | `strip_metadata_is_reported_and_dry_run_writes_nothing` |
| `T-STRIP-06` | Without the flag nothing is removed, whatever the encoder does | `metadata_is_preserved_when_the_flag_is_absent` |
| `T-STRIP-07` | TIFF: the textual tags removed, `Orientation`, resolution and strips kept | `tiff_strip_metadata_removes_only_textual_tags` |
| `T-STRIP-08` | zlib: nothing to remove, the flag is accepted, and no line claims otherwise | `strip_metadata_has_nothing_to_remove_from_zlib` |

---

## 3. Test levels

### Unit tests

- PNG scanline filters: encode/decode round-trip for the five filter types at every bytes-per-pixel value the
  format allows, 1 to 8 (`filters_roundtrip_for_all_types`), plus a refilter test that rebuilds a filtered
  stream from pixels. The integration side runs all five filters through the binary
  (`t_png_03_every_scanline_filter_round_trips`).
- Argument parser: every flag, combinations, `--`, conflicting flags, missing values.
- Size formatting, and column alignment for ASCII paths. Alignment is measured in display cells, and a test
  pins the table for CJK and combining characters (`display_width_counts_cells_not_characters`).
- JSON escaping: control characters, quotes, backslashes, non-UTF-8 paths.
- gzip header parse/serialise round-trip, including all flags.
- ZIP structure reader/writer: offsets, descriptors, EOCD, comments.
- Dedup and traversal policy: one integration test covers the same file reached through three paths. The
  traversal order is asserted there, not in a unit test.
- Colour detection (`src/style.rs`): a pure function of a probe struct, so `NO_COLOR`, `CLICOLOR`,
  `CLICOLOR_FORCE`, `TERM=dumb`, a pipe and a non-ANSI console are all covered without a terminal, and the
  three-way `--color` flag is tested against each of them.
- Progress line (`src/progress.rs`): the line is drawn here again since v0.13, so the drawing is tested —
  the bar's 64 sub-cell positions, the rate over the last eight completions (and its refusal to invent one
  from a single sample), the ETA's two gates (three seconds and two completions), the `mm:ss` clock past the
  hour, clipping by display width against the wcwidth table, the width from `COLUMNS` with the 80-column
  fallback, and the rule that a disabled, empty or non-terminal run creates no line at all. The escape
  stripping is tested too: the pad-to-previous-width rule must count cells, not bytes.
- Memory budget (`src/pipeline.rs`): `worst_case_bytes` is asserted term by term — a file with no format, a
  PNG with and without dimensions in the header, a lying `IHDR` clamped by `--max-pixels`, a JPEG, a TIFF,
  a ZIP charged its decompression ceiling, gzip charged nothing, and a `u64::MAX` length that must not
  overflow. `derived_memory_budget` is asserted to admit every format at the limits it is derived from.
- The probe (`src/detect.rs`): a PNG's dimensions are read from the header the detector already loads, and
  everything else — a JPEG, a truncated `IHDR`, an empty file — reports `pixels: None` rather than a guess.

### Integration tests

Run the real binary against a temporary directory and assert on exit code, stdout/stderr, and a byte-for-byte
comparison of the original with what is on disk afterwards. "Hash the file" in this document means that
comparison; nothing in the suite uses a cryptographic hash.

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

Fixtures are **built at run time, not committed**. `tests/common/mod.rs` contains a writer per format and a
`Workspace` helper that creates a temporary directory per test; nothing binary is in the repository, so a
reviewer can read exactly what a test feeds the binary. Corrupted inputs are produced by flipping documented
bytes of a valid fixture, so a failure is reproducible rather than a random blob.

What each builder can produce today:

| Builder | Covers | Does not cover |
| --- | --- | --- |
| `build_png` / `PngSpec` | 8-bit RGBA, non-interlaced; either scanline filter; compression level; arbitrary extra chunks (including unknown ones and `tEXt`); an APNG with `acTL` and frames | other color types, bit depths other than 8, interlaced images, palette with `tRNS` |
| `build_gzip` | payload of any size; optional `FNAME` and `FEXTRA`; `XFL`; a deliberately non-zero MTIME | `FCOMMENT` and `FHCRC` are spliced into the built header by the two tests that need them, not produced by the builder; multi-member streams and trailing garbage are built by hand in the refusal tests |
| `build_zip` / `ZipEntrySpec` | `store` and `deflate` per entry; level; data descriptors with and without signature; local and central extra fields; entry and archive comments; non-UTF-8 names | ZIP64, encryption, prepended stubs, signed JAR/APK/OOXML |
| `build_ico` | ICO and CUR directors with PNG or bitmap payloads of any size | — |
| `build_jpeg` | baseline and greyscale; restart intervals; trailing bytes; an Exif `APP1` payload | progressive, CMYK, arithmetic coding, 12-bit |
| `build_tiff` / `TiffSpec` | both byte orders; `none`/`deflate`; strips and rows per strip; planar, predictor and `FillOrder` settings; extra ASCII and SHORT tags; an optional tiled layout for refusal tests | LZW, JPEG-in-TIFF, multi-page, BigTIFF |

A format whose row above is thinner than the contract needs is a fixture gap, not a contract gap: the
handler may well support it, but nothing in this repository proves it does.

### Filesystem and environment tests

Covered today, each by an integration test that asserts the original file is byte-identical afterwards:

| Scenario | Expected | Test |
| --- | --- | --- |
| path with spaces and Unicode | optimized correctly | `unicode_and_space_filenames_work` |
| read-only file | `skipped`, original intact | `read_only_file_is_skipped` |
| missing path among valid ones | that path fails, others processed, exit `1` | `missing_path_fails_but_others_continue` |
| symlink to a file | `skipped` by default | `symlinks_are_not_followed_by_default` |
| same file via three paths | processed once | `duplicates_are_processed_once` |
| permissions and mtime | preserved across the replacement | `permissions_and_mtime_survive_the_replacement` |
| second run on the optimized tree | zero changes | `second_run_is_a_no_op` and the per-format equivalents |
| hard-linked file | `skipped` with a reason naming `--allow-hardlinks`; with the flag the link is broken on purpose and the other name keeps the old bytes | `a_hard_linked_file_is_skipped_unless_allowed` |
| Unix socket | `skipped` as not a regular file, never opened for writing, still there afterwards | `a_socket_is_skipped_and_never_opened_for_writing` |
| path that is not valid UTF-8 (Linux only) | optimized through the raw bytes; the JSON record carries `path_raw_b64`. APFS and Windows refuse to create such a name, so the fixture cannot exist there | `a_path_that_is_not_valid_utf8_is_optimized` |

**Not covered yet.** These are promises in [safety](safety.md) or consequences of it that no test exercises;
they are tracked as work in issue #35, not silently assumed:

| Scenario | Why it matters |
| --- | --- |
| file modified during processing | `failed`, original intact (the I5 path) |
| `SIGKILL` during processing | original intact; a `.tzim-*.tmp` may remain |
| empty file, 1-byte file | `unsupported` |

The rows that left this table in 0.12.1 are `T-FS-11` … `T-FS-16` above: a FIFO, a device node, a full disk
(behind an env var), a symlink cycle, a read-only directory and a 70-level tree. Each one was also the reason
its row was listed here, so the list is now the three scenarios that need a signal or a clock, not a
filesystem.

### Failure injection

**Not implemented.** There is no `test-hooks` feature and no `TZIM_TEST_*` variable in this build; nothing
here claims otherwise. What exists instead is structural: the candidate is a separate file owned by a guard,
the rename is the only operation that changes the visible state, and the guard removes the candidate on every
early return. The nearest evidence is therefore indirect — `guard_removes_uncommitted_file` in `src/replace.rs`
and the integration tests that leave a file untouched when a run fails — rather than a hook that forces a
failure at each stage.

Adding real hooks is a task of its own (it means a feature that must never reach a release build); until then
this section describes an intention, not a mechanism.

### Property tests

**`proptest` is not a dependency** of this project, and the tool never claims otherwise. Two tests cover part
of what property testing would: a hand-rolled PRNG test for the JPEG entropy codec (`src/formats/jpeg.rs`) and
the PNG filter round-trip across every bytes-per-pixel value. Neither is generated from a shrinking strategy,
so a failure is reported as a fixed input rather than a minimal one.

Properties worth turning into real property tests, in order of value: "an accepted candidate always passes
`verify`" (see #43), "the output is never larger than the input", and idempotence of a second run.

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

The fourth property — **the set of refused files is unchanged** — is gated since v0.13 (#36). The script
writes every file it did not optimize to `refusals.txt` as `<path><TAB><reason>` and compares the count per
reason with the committed `.github/corpus-refusals.txt`: a reason whose count changed fails the run, and a
reason that disappeared entirely is the case the row exists for — a construct that stopped being refused and
started being rewritten. A reason that is not in the baseline is a warning plus the line to paste, so a new
refusal is visible without a release being blocked on a table nobody has regenerated. The full list is in the
`<details>` block of the job's report and in the `corpus-run` artifact, which is what makes a changed count
actionable rather than mysterious.

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

The build enables **all seven** handlers (`png, gzip, zip, ico, jpeg, tiff, zlib`), so a detected TIFF or zlib
stream reaches its real parser instead of the "format not compiled" arm — `tiff` and `zlib` were missing from
`fuzz/Cargo.toml` until v0.13, which meant the two newest parsers never saw fuzz input at all (#36). Seeds are
the pngsuite PNGs and the libjpeg-turbo test JPEGs, Pillow's TIFF fixtures (copied into `roundtrip`, `strip`
and a `tiff` corpus), and eight generated zlib streams — four compression levels over text and over a PNG —
copied into `roundtrip` and `strip`. `strip` had no seeds at all before v0.13.

The properties are: no panic, no unbounded allocation, no integer overflow, termination inside a time budget,
and — for `roundtrip` and `strip` — that any candidate reported as accepted is no larger than the input and
passes verification. A 60 s-per-target smoke run is part of CI (`fuzz.yml`); longer runs are performed before a
release.

Fuzz artefacts are not committed; `fuzz/corpus` and `fuzz/artifacts` are ignored.

### The third-party decoder job

`verify` is the strongest check in the program and it has a structural blind spot: for JPEG, and for the ZIP
container, the code on both sides of the comparison lives in this repository. A defect symmetric in the writer
and the reader round-trips "successfully" ([guarantees §2.6](guarantees.md#26-verification-independence)).
`.github/workflows/third-party.yml` is the answer that costs the binary nothing:

| Family | Decoder | Compared |
| --- | --- | --- |
| PNG, JPEG, TIFF, ICO | Pillow (pinned `11.3.0`, libjpeg-turbo and libtiff versions printed) | mode, dimensions, transparency and every decoded sample, frame by frame |
| gzip, zlib | `gzip` / `zlib` from the Python standard library | the decompressed byte sequence |
| ZIP | `zipfile` from the standard library | every entry's name, method and decompressed bytes, in order, plus `testzip()` |

The job fetches the same pinned corpus as §3, keeps a copy of the originals, runs tzim over the tree, and then
compares each `optimized` file with its original. It is triggered by any change under `src/formats/`, weekly,
and on dispatch.

The split earns its keep: on its first run the job caught a real writer defect rather than a corpus quirk. Five
Pillow test images — `tiff_gray_2_4_bpp_hopper{2,4,2I,4I}R.tif` and `16bit.r.tif` — were read fine by Pillow,
rewritten by tzim, and then refused by Pillow with `decoder error -2`. Every one of them is `FillOrder 2`
(reversed bit order) and uncompressed, and libtiff reverses the bits of a *compressed* strip before inflating
it, so tzim's Deflate rewrite had been turning readable files into unreadable ones. The fix was a refusal
([guarantees §4.8](guarantees.md#48-tiff-classic-baseline-strips--v07)) — the file is left alone and the reason
is printed — plus `tiff_reversed_bit_order_is_skipped`, which fails on the writer as it was.

Three outcomes per file, the same three `verify` has had since v0.13: **both decode and agree** is covered;
**the original does not decode** is *no verdict* — a handful of corpus TIFFs and one APNG fixture are files
Pillow cannot read and tzim can, and an oracle with no opinion is not a verdict, so they are counted and named
in the report rather than failing the job; **the original decodes and the candidate does not, or the two
differ** is a defect. That split is not a loophole: it is exactly the rule `jpeg-v1` follows inside `verify`,
and the numbers (compared / no verdict / mismatches) are printed on every run so a growing "no verdict" count
is visible.

Two rules make it a gate rather than a report:

- **it proves it can fail.** `--self-test` mutates one accepted candidate — one changed pixel for an image,
  one flipped bit in a compressed payload — and requires the comparison to reject it. A comparison that
  silently stopped comparing fails the job instead of passing it, which is the failure mode of every
  "it opens" check ever written.
- **it is reproducible.** The library versions are printed and Pillow is pinned, because a verdict nobody can
  reproduce is not a verdict.

A mismatch fails the job and uploads both the report and the run's JSON, with the offending files named: this
is the one class of defect a human has to look at. What it does not cover: the `--strip-metadata` path (the
comparison is about content, and stripping is content-preserving by contract but not by this decoder's
knowledge), and the memory budget of §3's whole-file model.

---

---

## 4. Continuous integration

| Job | Platforms | Contents |
| --- | --- | --- |
| `test` | ubuntu-latest, macos-latest, windows-latest | two configurations on every platform: `cargo test --all-features --locked` and the plain `cargo test --locked` a release ships. Until v0.14 the second one was never run anywhere, so the `--info` sentence every published binary prints was a branch no CI run executed ([#68](https://github.com/Rudolf1517/tzim/issues/68)) |
| `minimal` | ubuntu-latest | `--no-default-features --features png`: every test target must **compile** (`--no-run`), then the library's unit tests, `verification` and `packaging` run. The 65 integration tests that assert what gzip, ZIP, ICO, JPEG, TIFF, zlib or `--zopfli` do cannot hold in a build without those handlers, and they are not filtered out by name — a name filter would silently run fewer tests as the suite grows |
| `fits oracle (astropy)` | ubuntu-latest | builds with `--features fits` and reads every candidate back with Astropy's own `astropy.io.fits` GZIP_1/GZIP_2 implementation (Python `gzip` plus Astropy's shuffle, not CFITSIO `funpack`): integer arrays compare by bytes and floats by FITS storage bytes, with negative controls. The separate `fits-heasarc` workflow runs CFITSIO `fitsverify`/`funpack -C`; both are external, run-scoped checks rather than decoders embedded in the binary |
| `msrv` | ubuntu-latest | `cargo build --all-features --locked` on Rust 1.88.0, the floor in `Cargo.toml` |
| `clippy` | ubuntu-latest | `cargo clippy --all-targets --all-features --locked -- -D warnings` |
| `fmt` | ubuntu-latest | on pull requests, `cargo fmt --all -- --check` only; on non-PR events, formats the tree and commits changes (§4.2) |
| `deny` | ubuntu-latest | `cargo deny check` — licences, advisories, duplicate versions, sources |
| `audit` | ubuntu-latest | `cargo audit` against the RustSec database |
| `size` | ubuntu-latest | `x86_64-unknown-linux-musl` build that `file` reports as `static-pie linked` (no interpreter, no libc), size gate and per-feature matrix (see [size budget](size-budget.md#6-ci-gate)) |
| `package` | ubuntu-latest | `cargo package --locked`, then a build from the packaged tarball — the crate as crates.io would receive it |
| `installer` | ubuntu-latest, macos-latest | runs `install.sh` against the published release, then proves a doctored archive is refused (§4.2) |
| `corpus` | ubuntu-latest | downloads the pinned corpus (PNG, JPEG, TIFF, gzip, ZIP and, since v0.13, zlib), runs the binary over it twice, gates on §3 including the refused-set baseline |
| `third-party` | ubuntu-latest | Pillow, stdlib `zlib` and `zipfile` compare the originals with the candidates (§3.3), on pull requests that touch `src/formats/` and weekly |
| `fuzz` | ubuntu-latest, nightly | 60 s per target on pull requests that touch `src/` or `fuzz/`; weekly on `main` |
| `bench` | ubuntu-latest | comparative benchmarks, by `bench` label, weekly, or on dispatch |
| `lockfile` | ubuntu-latest | regenerates `Cargo.lock` and commits it to the branch (§4.1) |

`audit` overlaps the advisory half of `deny`, and both stay: `deny` is the one that also knows about licences
and duplicate versions, `audit` is the RustSec-native check that speaks the advisory database directly. Two
tools reading the same database with different rules is the point, not duplication.

### 4.1 Packaging is tested, not assumed

The files that are shipped but never compiled are the ones that rot quietly: a flag added to the parser and
missing from the man page, a release target added to the workflow and missing from the installer. Three
mechanisms cover them.

**Drift assertions** (`tests/packaging.rs`, part of `cargo test`). Every long option in `--help` must appear in
`man/tzim.1` and in all three completions; the man page's `.TH` line must carry the crate version; every
target in the release matrix must be fetchable by `install.sh` (Windows excepted: it ships a zip, not a shell
installer); the Linux packaging script must ship the man page and the completions for both architectures; the
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
