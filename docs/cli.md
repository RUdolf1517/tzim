# Command-line interface

Related: [guarantees](guarantees.md) · [safety](safety.md) · [formats](formats.md)

The interface is deliberately small. There are no config files, no environment variables of our own, no
daemon, no stdin/stdout streaming, and no colour output. Everything is decided by the arguments and by the
files on disk, so a command that worked yesterday behaves the same today.

```
tzim [OPTIONS] <PATH>...
tzim --list-formats
tzim --info
tzim --check-deps
tzim --help
tzim --version
```

**Paths come first.** There is no verb: `tzim photo.png` optimizes one file, `tzim ./photos -r` optimizes a
tree. A path is never mistaken for a command, so a file called `optimize` needs no special handling beyond
the usual `--` when it starts with a dash.

---

## 1. Input objects

Every `PATH` is a complete input object on its own. A single file never needs to be wrapped in a directory.
Files and directories may be mixed in one invocation, in any order, with duplicates.

| Argument | Behaviour |
| --- | --- |
| regular file | processed |
| directory, without `--recursive` | its **top-level** regular files are processed; subdirectories are ignored |
| directory, with `--recursive` | the whole tree is walked depth-first |
| symbolic link | **skipped** with a message (see `--follow-symlinks`) |
| symlink given directly as an argument | skipped with a message (same rule) |
| non-existent path | reported as a failed argument; other paths are still processed |
| FIFO, socket, device | skipped with a message |
| empty file | skipped as unsupported (no signature) |

Directories themselves are never modified, renamed or deleted. Tzim touches files only.

### Deduplication

A file is processed at most once per run, even if it is reachable through several arguments or several
paths (`.`, `./image.png`, `image.png`, a symlink target):

- Unix: identity is the `(device, inode)` pair;
- Windows: identity is the canonical path, compared case-insensitively.

The first occurrence wins; later occurrences are silently dropped.

### Symlinks

By default Tzim never follows symbolic links, either in arguments or during traversal. A symlinked file or
directory is reported as `skipped` and left alone. Rationale: replacing a symlink target because a link was
enumerated is a surprise, and a symlink loop turns an innocent command into an infinite walk.

With `--follow-symlinks`:

- a symlink to a regular file is resolved and the **target** is optimized;
- symlinked directories are descended;
- cycles are prevented by the visited `(device, inode)` set;
- broken links are reported as failures.

### Ordering

Discovery order is deterministic: arguments are processed in the order given, and each directory's entries
are sorted by raw path bytes. Results are printed in discovery order even when `--jobs` > 1, so output is
reproducible and diffable.

---

## 2. Options

### v0.1 (PNG, gzip)

| Option | Default | Meaning |
| --- | --- | --- |
| `-r, --recursive` | off | descend into directories |
| `-n, --dry-run` | off | do all the work, write nothing, report what would change |
| `-j, --jobs <N>` | `min(cores, 4)`; with `--zopfli` `min(cores, 2)` | maximum number of files processed concurrently; `1` disables parallelism |
| `--json` | off | emit newline-delimited JSON on stdout (see §4) |
| `-q, --quiet` | off | suppress human output; errors still go to stderr |
| `--fail-fast` | off | stop after the first failed file |
| `--include <EXT,...>` | all | only files with these extensions (case-insensitive, repeatable, comma-separated) |
| `--exclude <EXT,...>` | none | skip files with these extensions; wins over `--include` |
| `--min-size <BYTES>` | `0` | skip smaller files |
| `--max-size <BYTES>` | `2147483648` (2 GiB) | skip larger files |
| `--max-pixels <N>` | `100000000` | skip images with more pixels (PNG) |
| `--max-uncompressed <BYTES>` | `8589934592` (8 GiB) | refuse archives that expand beyond this |
| `--max-ratio <N>` | `1000` | refuse archives whose compression ratio exceeds N:1 |
| `--formats <LIST>` | all except `jpeg` | only process these formats (`png`, `gzip`, `zip`, `ico`, `jpeg`); naming `jpeg` is the only way to enable it |
| `--strip-metadata` | off | remove a fixed list of embedded metadata (text, comments, timestamps, Exif); not lossless — see [guarantees §2.7](guarantees.md#27-g4--metadata-removed-on-request) |
| `--nested` | off | also optimize supported files stored inside archives |
| `--max-depth <N>` | `3` | how deep `--nested` may recurse (`0` is rejected) |
| `--zopfli` | off | use the Zopfli encoder; much slower, smaller output |
| `--follow-symlinks` | off | resolve and follow symbolic links |
| `--allow-hardlinks` | off | process files that have more than one hard link |
| `--allow-root-ownership-change` | off | when running as root, allow replacing files owned by another user (ownership becomes root; see [safety](safety.md#8-privileges)) |
| `--no-fsync` | off | skip `fsync` of the candidate and its directory (faster, less durable) |
| `--` | — | end of options; required for paths beginning with `-` |
| `-i, --info` | — | print what the program is: version, promise, formats, links, licence, contact; exits `0` without touching any path |
| `--list-formats` | — | print the handlers compiled into this build and what each one preserves; exits `0` |
| `--check-deps` | — | print the external programs needed at run time, which is none, and exit `0` |
| `-h, --help` | — | the full text of §2; exits `0` |
| `-V, --version` | — | one line, `tzim <version>`; exits `0` |

`--include` and `--exclude` take extension lists (`png,gz`), not glob patterns. Globs are deliberately not
implemented: shell globbing already exists, and a second pattern language is a second set of edge cases.

`--strip-metadata` is the only option that removes information, and it never removes content:

| Format | Removed | Kept |
| --- | --- | --- |
| PNG, APNG | `tEXt`, `zTXt`, `iTXt`, `tIME`, `eXIf` | ICC, `sRGB`, `gAMA`, `cHRM`, `pHYs`, `sBIT`, `bKGD`, `tRNS`, APNG control chunks, unknown chunks |
| gzip | `FNAME`, `FCOMMENT`, `FEXTRA`, `FHCRC`, `MTIME` (zeroed) | `XFL`, `OS`, method, payload, CRC-32, ISIZE |
| ZIP | archive comment, entry comments, DOS timestamps | names, extra fields, attributes, order, method |
| JPEG | `APP1` (Exif/XMP), `APP13` (Photoshop), `COM` | `APP0`, `APP2` (ICC), `APP14`, other `APPn`, tables, coefficients |
| ICO | nothing of its own; embedded PNGs follow the PNG rule | directory, bitmaps |
| TIFF | `ImageDescription`, `DocumentName`, `Make`, `Model`, `Software`, `DateTime`, `HostComputer`, `InkNames`, `Copyright`, IPTC, Photoshop, `UserComment`, `ImageSourceData`, Windows XP tags, GDAL metadata | colour and geometry tags, resolution, `Orientation`, ICC (`34675`), strips |

With the flag the acceptance rule becomes "never larger" rather than "strictly smaller": zeroing a
timestamp removes metadata without removing bytes, and the point of the flag is the metadata. A candidate
that is *larger* is still refused under every flag.

`--formats` is an allowlist, not an addition: `--formats jpeg` processes JPEG and nothing else, and
`--formats png,jpeg` processes both. Formats that are detected but not enabled are reported as `skipped`
with the reason, and the run still exits 0. Nested files inherit the same allowlist, so a PNG inside a ZIP
is left alone unless `png` is enabled.

All options above are frozen from v0.3; the JPEG switch is the only behavioural change in v0.4.

---

## 3. Human output

Per-file line on stdout, then a summary:

```
photo.png        PNG     1.2 MiB -> 1.05 MiB   -15.3%
logo.svgz        GZIP    18.2 KiB -> 16.9 KiB   -7.1%
notes.txt        -       skipped: unsupported format
scan.tif         -       skipped: unsupported format
Summary: 4 files, 2 optimized, 211.3 KiB saved (-13.9%), 2 skipped, 0 failed, 0.8 s
```

Rules:

- sizes are binary units (`KiB`, `MiB`, `GiB`) with one decimal; exact bytes are always available in JSON;
- `->` is used instead of a Unicode arrow so output survives naive terminals;
- alignment is fixed-width per column based on the longest path in the run, capped to keep lines readable;
- files that were not changed are still listed, so a run is never silent about what it skipped;
- `-q` suppresses all of the above; errors and warnings remain on stderr;
- stdout is results only; diagnostics, warnings and errors always go to stderr. Piping stdout to a file
  therefore yields clean, machine-readable data even in human mode.

Statuses shown to humans: `optimized`, `already optimal`, `skipped: <reason>`, `failed: <reason>`.

---

## 4. JSON output

`--json` emits **newline-delimited JSON**: one object per file in discovery order, then exactly one summary
object. Nothing else is written to stdout. Warnings and errors still appear on stderr as human text.

```json
{"type":"file","path":"photo.png","format":"png","status":"optimized","before":1258291,"after":1065000,"saved":193291,"guarantee":"format_equivalent","duration_ms":412,"note":null,"error":null}
{"type":"file","path":"notes.txt","format":null,"status":"skipped","before":41,"after":41,"saved":0,"guarantee":"byte_identical","duration_ms":0,"note":"unsupported format","error":{"kind":"unsupported","message":"unsupported format"}}
{"type":"summary","schema":1,"files":2,"optimized":1,"would_optimize":0,"already_optimal":0,"unsupported":1,"skipped":0,"failed":0,"before":1258332,"after":1065041,"saved":193291,"duration_ms":414,"dry_run":false}
```

Field rules:

| Field | Type | Notes |
| --- | --- | --- |
| `type` | string | `file` or `summary` |
| `path` | string | lossy UTF-8 rendering of the path, for humans |
| `path_raw_b64` | string, optional | base64 of the raw path bytes; present on Unix only when the path is not valid UTF-8 |
| `format` | string or null | handler id: `png`, `gzip`, `zip`, `ico`, `apng`, `jpeg`, `tiff` |
| `status` | string | `optimized`, `would_optimize`, `already_optimal`, `unsupported`, `skipped`, `failed` |
| `before`, `after` | integer | file sizes in bytes; equal for every non-optimizing outcome |
| `saved` | integer | `before - after` |
| `guarantee` | string | `format_equivalent` for `optimized` / `would_optimize`; `byte_identical` otherwise |
| `metadata_stripped` | bool | present and `true` only when `--strip-metadata` rewrote this file |
| `duration_ms` | integer | wall time spent on this file |
| `note` | string or null | why the file landed in this status: the refusal reason for `skipped`, the filesystem reason for a skipped replacement |
| `error` | object or null | `{"kind": "...", "message": "..."}`; `kind` is one of `io`, `parse`, `limit`, `refused`, `verify`, `internal` (a `usage` error exits before any file record exists); `refused` and `limit` accompany a `skipped` status, meaning the input is valid but Tzim declines it |
| `schema` | integer | present in the summary only; bumped on breaking schema changes |
| `dry_run` | boolean | present in the summary only |

JSON strings are escaped according to RFC 8259, with `\u00XX` escapes for control characters. Object keys
and enum values are stable; new optional fields may be added in minor versions.

---

## 5. Exit codes

| Code | Meaning |
| --- | --- |
| `0` | No failures. Includes `already_optimal`, `unsupported` and `skipped` (including refused inputs). |
| `1` | At least one file failed; the rest were processed. |
| `2` | Invalid usage (unknown flag, no paths, conflicting flags). Nothing was processed. |
| `3` | Fatal error: the run could not be completed (unreadable input root, out of memory, stdout closed). |
| `4` | **A verification failure occurred.** The affected file was not replaced; the run is aborted. |
| `130` | Interrupted by `SIGINT`; produced by the shell/OS, not by Tzim. |

Precedence when several conditions apply: `2` (immediate) → `4` → `3` → `1` → `0`.

A verification failure is always treated as a bug. It aborts the run even without `--fail-fast`, logs the
contract clause that failed, and leaves the file untouched. Files that were already replaced earlier in the
run keep their (verified) changes.

---

## 6. Dry run

`--dry-run` performs the **complete** pipeline: detection, candidate generation and verification. Only the
final rename is skipped, and the temporary file is deleted. This makes the reported numbers exact instead of
an estimate, at the cost of doing the real work.

A dry run still creates a candidate in the destination directory (verification and exact sizing require the
real bytes) and removes it afterwards. It never opens an existing file for writing, and it never replaces
anything, so every outcome is still G1.

Statuses in dry-run: `would_optimize` (a strictly smaller, verified candidate exists) or `already_optimal`
(no candidate was smaller).

---

## 7. Defaults and limits

| Limit | Default | Why |
| --- | --- | --- |
| maximum file size | 2 GiB | protects against accidental huge inputs; raising it is one flag |
| maximum pixels | 100 MPix | a decoded RGBA buffer at this size is already ~400 MiB |
| jobs | 4 (2 with `--zopfli`) | bounded memory; Zopfli's compressor state is large |
| recursion | off | an explicit flag is safer than a surprise tree walk |
| symlink following | off | avoids surprising writes and infinite walks |
| hard-linked files | skipped | replacement would break the link for other paths |
| `fsync` | on | a replaced file should survive a crash |

Limits produce `skipped`, never `failed`: hitting a configured bound is not an error.

---

## 8. Examples

```sh
# preview a whole tree, exact numbers, nothing written
tzim ~/Pictures --recursive --dry-run

# aggressive PNG optimization, two jobs
tzim *.png --zopfli --jobs 2

# CI check: never modify, fail if any file was reported failed
tzim assets --recursive --dry-run --json > report.ndjson

# exactly the archive types you care about
tzim . --recursive --include zip,jar,whl

# files with spaces and non-ASCII names need only normal shell quoting
tzim "Photo 2024 (копия).png" "иконки/логотип.svgz"
```

---

## 9. Stability policy

- Existing flags, their defaults and the exit codes are frozen from v0.1.
- New flags are additive and default to the previous behaviour.
- Removing or renaming a flag, or changing an exit code, requires a major version bump.
- The JSON schema is versioned with `schema`; fields may be added, not removed or repurposed.

### One exception, already made

v0.5.0 removed the `optimize` subcommand, which is exactly the kind of change the rules above forbid — the
interface became simpler and it was worth the break once, at 0.x, rather than never.

`tzim optimize <PATH>...` is still accepted: a leading `optimize` is stripped before parsing, so no script
written against 0.1–0.4 breaks. `tzim formats` survives the same way as an alias of `--list-formats`. Both
are compatibility shims, not documented commands, and they will be removed only when a major version makes
that free.
