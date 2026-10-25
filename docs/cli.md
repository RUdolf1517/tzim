# Command-line interface

Related: [guarantees](guarantees.md) · [safety](safety.md) · [formats](formats.md)

The interface is deliberately small. There are no config files, no environment variables of our own, no
daemon and no stdin/stdout streaming. Everything that decides *what happens to a file* is an argument or is
on disk, so a command that worked yesterday behaves the same today. Two things decide only how the report
*looks* — whether colour is on and whether the progress line is drawn — and both are off unless a terminal
is attached (see [§10](#10-colour-and-progress)).

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
- cycles are stopped by a **depth limit of 64 directories**: the walk keeps no visited set of
  `(device, inode)` pairs, so a loop is not recognised as a loop — it runs out of depth and the subtree below
  it is reported as refused (`directory nesting deeper than 64 levels; refused`, status `skipped`, exit 0).
  The limit exists for exactly this reason, and it is also what protects the stack;
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
| `--color <WHEN>` | `auto` | `auto`, `always` or `never`; see [§10](#10-colour-and-progress) |
| `--no-color` | — | same as `--color=never` |
| `--no-progress` | — | never draw the progress line, even on a terminal |
| `--fail-fast` | off | stop after the first failed file |
| `--include <EXT,...>` | all | only files with these extensions (case-insensitive, repeatable, comma-separated) |
| `--exclude <EXT,...>` | none | skip files with these extensions; wins over `--include` |
| `--min-size <BYTES>` | `0` | skip smaller files |
| `--max-size <BYTES>` | `2147483648` (2 GiB) | skip larger files |
| `--max-pixels <N>` | `100000000` | skip images with more pixels (PNG and JPEG; TIFF is bounded by `--max-size` and by the single strip it decodes at a time) |
| `--max-uncompressed <BYTES>` | `8589934592` (8 GiB) | refuse archives that expand beyond this |
| `--max-ratio <N>` | `1000` | refuse archives whose compression ratio exceeds N:1 |
| `--formats <LIST>` | all except `jpeg` | only process these formats (`png`, `gzip`, `zip`, `ico`, `jpeg`, `tiff`, `zlib`); naming `jpeg` is the only way to enable it |
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
| zlib | nothing: no name, no comment, no timestamp in the header | the header, the trailer, the payload |
| FLAC (if ever implemented) | `VORBIS_COMMENT`, `PICTURE`, `PADDING` | `STREAMINFO`, `SEEKTABLE`, `CUESHEET`, `APPLICATION` |
| TIFF | `DocumentName` (269), `ImageDescription` (270), `Make` (271), `Model` (272), `Software` (305), `DateTime` (306), `Artist` (315), `HostComputer` (316), `InkNames` (333), `Copyright` (33432), IPTC, Photoshop, `UserComment`, `ImageSourceData`, Windows XP tags, GDAL metadata (42112) | colour and geometry tags, resolution, `Orientation`, ICC (`34675`), strips, GDAL nodata (`42113`), unknown tags |

With the flag the acceptance rule becomes "never larger" rather than "strictly smaller": zeroing a
timestamp removes metadata without removing bytes, and the point of the flag is the metadata. A candidate
that is *larger* is still refused under every flag.

`--formats` is an allowlist, not an addition: `--formats jpeg` processes JPEG and nothing else, and
`--formats png,jpeg` processes both. Nested files inherit the same allowlist, so a PNG inside a ZIP is left
alone unless `png` is enabled.

### `--formats` is a policy, not a filter

A filter would decide *whether a path is looked at*. A policy decides *what may happen to it*, and the
difference is visible in the report and in the exit code. Exactly one of three words describes every input,
and they are never interchangeable:

| Word | Meaning | Exit |
| --- | --- | --- |
| `unsupported` | there is no handler for this content at all — a text file, a format this build does not carry, or a handler that proved the signature a coincidence (a zlib header, see below) | `0` |
| `skipped` | a handler exists, and the run declines: `--formats` does not enable it, a policy refused the input (a signed archive, ZIP64, a progressive JPEG), or a limit was reached | `0` |
| `failed` | the content claims to be a supported format and is corrupt, or the filesystem refused the operation | `1` |

So `--formats gzip` over a directory of PNGs reports every PNG as `skipped: PNG is not enabled for this
run (use --formats; enabled: gzip)` and touches nothing. It is not `unsupported` (this build has a PNG
handler) and it is not `failed` (nothing is wrong with the file).

The one way `unsupported` carries a note is a **weak signature proven wrong by the handler**. zlib is the
only format detected by a check rather than a magic number: 1 random two-byte pair in 1984 satisfies it, so a
tree of 2000 unrelated files has a ~63 % chance of containing one. Such a file decodes to nothing, and the
line reads

```
receipt.bin      -      unsupported: zlib: the header passes the container's check but nothing decodes; this is not a zlib stream (the file is untouched)
```

Exit `0`, and the file is byte-identical afterwards. A `.zlib` that decoded at least one byte and then broke
is `failed` with exit `1` — that is corruption, and the two cases are never merged.

All options above are frozen from v0.3; the JPEG switch is the only behavioural change in v0.4.

---

## 3. Human output

Per-file line on stdout, then a summary:

```
photo.png        PNG    1.20 MiB -> 1.05 MiB  -12.5%
logo.svgz        GZIP   18.20 KiB -> 16.90 KiB  -7.1%
notes.txt        -      unsupported
scan.tif         TIFF   already optimal
Summary: 4 files, 2 optimized, 211.30 KiB saved, 1 already optimal, 1 unsupported, 0 skipped, 0 failed, 0.80 s
```

Rules:

- sizes are binary units (`KiB`, `MiB`, `GiB`) with two decimals — the arithmetic is integer, so the digits
  are exact rather than merely rounded; exact bytes are always available in JSON;
- `->` is used instead of a Unicode arrow so output survives naive terminals;
- alignment is fixed-width per column based on the longest path in the run, capped to keep lines readable;
- files that were not changed are still listed, so a run is never silent about what it skipped;
- `-q` suppresses all of the above; errors and warnings remain on stderr;
- stdout is results only; diagnostics, warnings and errors always go to stderr. Piping stdout to a file
  therefore yields clean, machine-readable data even in human mode;
- the summary is the last line, because that is where the eye goes after a long run.

Statuses shown to humans: `optimized`, `would optimize` (dry run), `already optimal`, `unsupported`,
`skipped: <reason>`, `failed: <reason>`. Only the last two carry a reason, plus the `unsupported` case where
a handler overruled the detector (see the table in §2); the others are the whole message, and a note that
merely repeated the status word (`unsupported: unsupported format`) was removed.

In a terminal, colour marks the outcome and nothing else: the status word (green for a rewrite, dim for
"nothing to do", yellow for a skip, red for a failure), the saving percentage, and the saved total in the
summary. The format column is dimmed, and `(dry run)` / `(metadata stripped)` are dimmed suffixes. Nothing
else is coloured — headings, tables and `--help` stay plain, because reference text is pasted into issues
and piped into files.

## 4. JSON output

`--json` emits **newline-delimited JSON**: one object per file in discovery order, then exactly one summary
object. Nothing else is written to stdout. Warnings and errors still appear on stderr as human text.

```json
{"type":"file","path":"photo.png","format":"png","status":"optimized","before":1258291,"after":1065000,"saved":193291,"guarantee":"format_equivalent","duration_ms":412,"note":null,"error":null}
{"type":"file","path":"notes.txt","format":null,"status":"unsupported","before":41,"after":41,"saved":0,"guarantee":"byte_identical","duration_ms":0,"note":null,"error":null}
{"type":"summary","schema":1,"files":2,"optimized":1,"would_optimize":0,"already_optimal":0,"unsupported":1,"skipped":0,"failed":0,"before":1258332,"after":1065041,"saved":193291,"duration_ms":414,"dry_run":false}
```

Field rules:

| Field | Type | Notes |
| --- | --- | --- |
| `type` | string | `file` or `summary` |
| `path` | string | lossy UTF-8 rendering of the path, for humans |
| `path_raw_b64` | string, optional | base64 of the raw path bytes; present on Unix only when the path is not valid UTF-8 |
| `format` | string or null | handler id, from `Format::id`: `png`, `gzip`, `zip`, `ico`, `jpeg`, `tiff`, `zlib`. An APNG is reported as `png`, because the PNG handler is what rewrites it |
| `status` | string | `optimized`, `would_optimize`, `already_optimal`, `unsupported`, `skipped`, `failed` |
| `before`, `after` | integer | file sizes in bytes; equal for every non-optimizing outcome |
| `saved` | integer | `before - after` |
| `guarantee` | string | `format_equivalent` for `optimized` / `would_optimize`; `byte_identical` otherwise |
| `metadata_stripped` | bool | present and `true` only when `--strip-metadata` rewrote this file |
| `duration_ms` | integer | wall time spent on this file |
| `note` | string or null | why the file landed in this status: the refusal reason for `skipped`, the filesystem reason for a skipped replacement |
| `error` | object or null | present **only** with `"status":"failed"`, otherwise `null`: `{"kind": "...", "message": "..."}`. `kind` is one of `io`, `parse`, `verify`, `internal` — never `unsupported`, `limit` or `refused`, because those are not failures: a refusal is a `skipped` status with the reason in `note`, and unsupported content is the `unsupported` status, with the reason in `note` only when a handler overruled the detector |
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

A **closed stdout** is not a failure and does not change the result. When the reader of a pipe goes away —
`tzim . -r | head -1` — the write fails with `EPIPE`; Tzim says nothing further and exits with the code the
run had already earned (`0`, `1` or `4`), because losing the report should not turn a failed file into a
success. Any other write error on stdout (a full disk, a broken redirect) is a fatal error and exits `3`.

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

Statuses in dry-run: `would_optimize` (a smaller verified candidate exists — or an equal-size one under
`--strip-metadata`) or `already_optimal` (nothing to gain).

---

## 7. Defaults and limits

| Limit | Default | Why |
| --- | --- | --- |
| maximum file size | 2 GiB | protects against accidental huge inputs; raising it is one flag |
| maximum pixels | 100 MPix | a decoded RGBA buffer at this size is already ~400 MiB |
| jobs | 4 (2 with `--zopfli`) | bounds how many files are in flight; it does **not** bound memory — see [safety §7.1](safety.md#71-memory--an-upper-bound-not-a-budget) |
| recursion | off | an explicit flag is safer than a surprise tree walk |
| symlink following | off | avoids surprising writes and infinite walks |
| hard-linked files | skipped | replacement would break the link for other paths |
| `fsync` | on | a replaced file should survive a crash |

Limits produce `skipped`, never `failed`: hitting a configured bound is not an error.

`--jobs` is about parallelism, not about memory: the whole file and its candidate are held in memory, and a
decoded image can cost hundreds of megabytes per side. The real ceiling and the numbers behind it are in
[safety §7.1](safety.md#71-memory--an-upper-bound-not-a-budget); `--max-pixels` and `--max-size` are the two
flags that actually reduce it.

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
- Human output may gain colour, alignment or a progress line in a minor version; machine output (`--json`)
  may not change shape without a `schema` bump.
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

---

## 10. Colour and progress

Both are *presentation*: they change how the report looks, never what happens to a file. Both are off unless
a terminal is attached, and both have a flag that forces the decision.

### Colour

The report is coloured when its stream is an interactive terminal. `stdout` and `stderr` are probed
separately: `tzim . > report.txt` writes plain text to the file and still colours the errors it prints on
screen, which is the behaviour that makes a redirected report readable and a terminal diagnosis possible at
the same time.

| Source | Effect |
| --- | --- |
| `--color=always`, `--color=never` | the decision, in both directions; wins over everything below |
| `--no-color` | same as `--color=never` |
| `NO_COLOR` (set and non-empty) | off — [no-color.org](https://no-color.org/) |
| `CLICOLOR_FORCE` (set, not `0`) | on, even through a pipe — what CI job logs are for |
| `CLICOLOR=0` | off |
| `TERM=dumb` | off |
| not a terminal | off |

`--json` is never coloured, whatever the flag says: escape sequences inside a JSON string would be a bug in
the producer, not a feature for the consumer.

On Windows the console only interprets ANSI sequences on Windows 10 and later, and only when the program
asks it to — which would mean either `unsafe` or a dependency. Colour is therefore offered only in terminals
that advertise it (`WT_SESSION`, `TERM`, `ANSICON`, `TERM_PROGRAM`), and `--color=always` overrides that for
somebody who knows their terminal. Printing `[32m` into an old `cmd.exe` is worse than no colour.

What is coloured, and with what:

| Element | Attribute | Why |
| --- | --- | --- |
| `optimized` / `would optimize` | green | the run did something |
| the saving percentage | green, dim when nothing moved | the number people look for |
| `already optimal` | dim | nothing to do, nothing to worry about |
| `skipped` / `unsupported` | yellow / dim | not an error |
| `failed` | red | the only thing that is one |
| saved total, failed count in the summary | green / red | the two numbers the summary exists for |
| the format column | dim | context, not a result |
| `tzim:` prefix of a diagnostic | red | errors are on stderr and are rare |

Only SGR codes `1`, `2`, `31`, `32`, `33` and a `0` reset are used: no 256-colour, no truecolor. A terminal
that cannot render the palette should get clean text, not approximations of it. `--help`, `--info`,
`--list-formats` and `--check-deps` are never coloured — they are reference text that gets pasted into issues
and piped into files.

### Progress

Runs of a few hundred files can take minutes, and a program that prints nothing for a minute looks broken.
While the work is in progress Tzim draws one line on **stderr**. The shape follows the job:

```
⠋ converting…
⠋ 00:01 [████░░░░] 3/12
```

The first line is a single file: there is no counter to show, so the spinner and the verb are the whole
message. The second is a batch: the clock, the bar and the position, in the order the eye looks for them.

- the spinner has ten braille frames and advances every 100 ms;
- the bar is eight cells — `█` done, `░` remaining — and it is drawn only when there is more than one file;
- the clock is `mm:ss`, counted from the start of the run. Past 99 minutes it grows to three digits rather
  than wrapping, because a run that long is a thing the reader wants to know about;
- the line appears only after 150 ms, so a run that finishes quickly does not flash, and it is erased before
  the first result is printed, so the report never shares a row with it;
- it contains no path: a path makes the line as wide as the deepest directory and turns a status indicator
  into noise. The position, the clock and the bar are the information;
- the spinner and the bar are coloured by the same decision as the rest of stderr (the table above) and are
  plain when colour is off, so `NO_COLOR=1` produces no escape sequence anywhere;
- it is written to stderr only. stdout carries the report, one result per line, and nothing else.

It is drawn only when **all** of these hold: stderr is a terminal, the run has at least one file, and
`--quiet`, `--json` and `--no-progress` are absent. A line of `\r` frames belongs on a terminal; in a log
file or a CI transcript it is noise that hides the lines around it.

The drawing is [`indicatif`](https://crates.io/crates/indicatif) 0.18. It is the one dependency in this
project that is not behind a feature flag: the templates and the `mm:ss` clock are ours, but keeping a line
redrawn from several worker threads consistent with the terminal, at the right width, is not a problem worth
solving twice. What it costs in bytes is in [size-budget §3](size-budget.md#3-feature-matrix), and why it was
admitted is in [dependencies §2.9](dependencies.md#29-indicatif--the-progress-line).

The progress line is not a measurement. `Summary:` is.

---

## 11. Conformance with CLI conventions

Measured against [Command Line Interface Guidelines](https://clig.dev/) and the POSIX utility syntax
guidelines, from the behaviour of the binary rather than from intent. Each row is a convention, whether this
tool follows it, and what was done about it.

### 11.1 Followed

| Convention | Where |
| --- | --- |
| `--help` and `--version` exist, go to stdout, exit `0` | `-h/--help`, `-V/--version` |
| usage errors go to stderr, print `usage:` and exit `2` | [§5](#5-exit-codes) |
| distinct, documented exit codes; success is `0` | [§5](#5-exit-codes) |
| results on stdout, diagnostics on stderr; either can be redirected alone | [§3](#3-human-output) |
| machine-readable output is a separate mode with its own schema | `--json`, [§4](#4-json-output) |
| options may appear before, between or after paths; `--` ends parsing | [§2](#2-options) |
| `--opt=value` and `--opt value` are both accepted | [§2](#2-options) |
| no interactive prompts, no pager, no config file, no hidden state | everywhere |
| a single action needs no subcommand | paths are the command, [§1](#1-input-objects) |
| long and short forms for everything used often (`-r`, `-n`, `-q`, `-j`) | [§2](#2-options) |
| colour and animation only on a terminal, `NO_COLOR` honoured, `--no-color` offered | [§10](#10-colour-and-progress) |
| `SIGINT` works: no signal handlers, no half-written files | [safety](safety.md#5-failure-modes) |
| errors name the thing that failed and the flag that fixes it | `--allow-hardlinks`, `--follow-symlinks` |

### 11.2 Fixed in v0.8.0 — four real violations

These were found by writing this section, and every one of them was a user-visible defect:

| Problem | What it looked like | Fix |
| --- | --- | --- |
| **A closed pipe aborted the process.** Rust ignores `SIGPIPE`, so `println!` failed with `EPIPE` and panicked; with `panic = "abort"` that is an abort, a core-shaped message and exit `134`. | `tzim . -r \| head -1` printed `thread 'main' panicked at ... failed printing to stdout` | the report writes to a sink and returns `io::Result`; `EPIPE` ends the report quietly with the run's own exit code ([§5](#5-exit-codes)) |
| **No short-flag clustering.** Every getopt-based tool accepts `-rq`; this one answered `unknown option`. | `tzim -rq .` failed | clusters expand, and `-j4` is accepted as `-j 4` |
| **No usage line on a usage error.** The message named the bad flag and then said "try `tzim --help`", leaving the shape of the command to be guessed. | `tzim: unknown option \`--colour\`` | the same error now prints `usage: tzim [OPTIONS] <PATH>...` |
| **No way to control colour or animation** — the tool could not be made to colour a log or to stop animating, and ignored `NO_COLOR`. | nothing to type | `--color`, `--no-color`, `--no-progress`, [§10](#10-colour-and-progress) |

A fifth was found and fixed in the documentation rather than the code: §3 showed output with one decimal and
a percentage inside the summary line, neither of which the binary has ever printed. The text now matches the
binary, and the binary was left alone — its format is frozen and two decimals are more precise.

### 11.3 Deliberate deviations

Documented rather than planned for removal. Each is a place where the convention and the guarantee pull in
different directions.

| Deviation | Why |
| --- | --- |
| **`-` is not stdin.** A path of `-` is a file literally named `-`. | The tool replaces files in place and verifies against the original on disk; a stream has no original to compare with, so a stdin mode could not offer the guarantee that defines the program. |
| **`optimize` and `formats` as first words** shadow files with those names (frozen, removed at 1.0). | Compatibility with 0.1–0.4. `tzim -- optimize` already works for the file. |
| **No "did you mean" suggestions.** | A table of every flag costs bytes in a binary whose size is a requirement, and a guess is wrong often enough to be worse than silence. The usage line is one line away. |
| **`--jobs 0` is rejected** rather than meaning "choose for me". | Zero means "decide for me" only if it is documented as such; an explicit number, including `1`, is unambiguous. |
| **No `--progress=always`.** | The only reason to want progress in a pipe is to read it later, and what is worth reading later is the summary. |
| **`tzim` with no arguments prints the usage error, not the help.** | A missing path is a mistake, not a request for documentation; the error carries the usage line and the hint. |
| **No shell completions or man page are shipped.** | They are another surface to keep in step with the flags; `--help` is generated from no source and cannot drift. If they are added, they will be generated from the parser in the same commit. |
