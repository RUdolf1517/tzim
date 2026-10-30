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
tzim --capabilities
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
- a cycle is never *recognised* as a cycle: the walk keeps no visited set of `(device, inode)` pairs. What
  stops it in practice is the kernel, which refuses to resolve a path with more than a fixed number of symlink
  hops (ELOOP — 40 on Linux, 32 on macOS). That path is reported `failed: Too many levels of symbolic links`,
  the run exits `1`, and every other file is still processed. The **depth limit of 64 directories** is the
  second guard: it is what stops a tree of real directories deeper than 64 levels
  (`directory nesting deeper than 64 levels; refused`, status `skipped`, exit `0`) and what protects the stack,
  and a link cycle effectively never reaches it. Both were measured rather than assumed — see `T-FS-14` and
  `T-FS-16` in [testing §2](testing.md#2-traceability);
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
| `--memory-budget <BYTES>` | derived from the ceilings: `6 × --max-size + 8 × --max-pixels + --max-uncompressed + 128 MiB` | skip a file whose worst-case cost would exceed this; charged before the file is read, so a file that cannot fit is a line of output instead of the OOM killer. The budget is **per file**, so `--jobs` files can be charged at once: compare `--jobs × --memory-budget` with the memory the machine has — see [safety §7.1](safety.md#71-memory--an-enforced-budget) |
| `--formats <LIST>` | all handlers enabled by this build's default policy (baseline JPEG when `jpeg-v1` is compiled; FITS still needs `--fits-tile`) | only process the named formats (`png`, `gzip`, `zip`, `ico`, `jpeg`, `tiff`, `zlib`, `fits`, `flac`, `pdf` when compiled); it is an allowlist, so `--formats png` excludes JPEG even in the default build |
| `--fits-tile` | off | FITS: rewrite an uncompressed image HDU as a tile-compressed binary table (`GZIP_1` for integers, `GZIP_2` for floats, quantisation off, one tile per row, every other HDU copied verbatim). In an interactive single-file TTY run, the candidate is fully verified before a default-No confirmation; elsewhere this flag is the explicit non-interactive opt-in. The handler is compiled in only with the `fits` cargo feature, so a `default` build answers `unsupported` — see §4.12 of [guarantees](guarantees.md) |
| `--strip-metadata` | off | remove a fixed list of embedded metadata (text, comments, timestamps, Exif); not lossless — see [guarantees §2.7](guarantees.md#27-g4--metadata-removed-on-request) |
| `--nested` | off | also optimize supported files stored inside archives |
| `--max-depth <N>` | `3` | how deep `--nested` may recurse (`0` is rejected) |
| `--zopfli` | off | use the Zopfli encoder; much slower, smaller output |
| `--follow-symlinks` | off | resolve and follow symbolic links |
| `--allow-hardlinks` | off | process files that have more than one hard link |
| `--allow-root-ownership-change` | off | when running as root, allow replacing files owned by another user (ownership becomes root; see [safety](safety.md#8-privileges)) |
| `--no-fsync` | off | skip `fsync` of the candidate and its directory (faster, less durable) |
| `--` | — | end of options; required for paths beginning with `-` |
| `-i, --info` | — | print what the program is: version, promise, formats, the JPEG verification level of this build, links, licence, contact; exits `0` without touching any path |
| `--list-formats` | — | print the handlers compiled into this build and what each one preserves; the JPEG row says default-enabled only with `jpeg-v1`, otherwise codec-only and opt-in via `--formats jpeg`; exits `0` |
| `--capabilities` | — | print one versioned JSON object describing build features, compiled/default-enabled formats, selection flags, handler opt-ins, transformations and confirmation behavior; read-only and exits `0` |
| `--check-deps` | — | print the external programs needed at run time, which is none, and exit `0` |
| `-h, --help` | — | the full text of §2; exits `0` |
| `-V, --version` | — | one line, `tzim <version>`; exits `0` |

### FITS tile confirmation

`--fits-tile` remains the only confirmation-enabled transformation. For one direct regular-file path on a TTY,
when `--fits-tile` is set, the input is eligible and the ordinary candidate verification succeeds, Tzim asks on
stderr whether to change the `IMAGE` HDU into a tile-compressed `BINTABLE`. The default answer is No; only `y`
or `yes` proceeds to the normal atomic install. No, EOF, a closed input, an invalid answer, or a prompt I/O error
reports `skipped` and leaves the original byte-identical; the staged temporary is removed.

No prompt is shown for non-TTY or JSON output, `--dry-run`, recursive or multi-file/batch runs,
nested work, a filtered-out format, a safety-skipped hard link, an unknown/refused input, or a failed
candidate/verifier. `--quiet` does not turn the confirmation into an implicit Yes; an eligible TTY run
still asks for consent. Those cases never wait for input. `--jobs` does not change the single-file path: the eligible
file is processed synchronously on the main thread even if a larger worker count was requested. In non-TTY
runs `--fits-tile` itself is the explicit opt-in and uses the same verified pipeline. `--formats` remains an
allowlist, not a force switch; it cannot make an unknown or refused FITS file eligible. No other format prompts.

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
| FLAC (when the non-default `flac` feature is compiled) | `VORBIS_COMMENT`, `PICTURE`, `PADDING` | `STREAMINFO`, `SEEKTABLE`, `CUESHEET`, `APPLICATION` |
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
receipt.bin      -      unsupported: zlib: nothing decodes behind the header; either the header was a coincidence or the stream is too short to tell (the file is untouched)
```

Exit `0`, and the file is byte-identical afterwards. A `.zlib` that decoded at least one byte and then broke
is `failed` with exit `1` — that is corruption, and the two cases are never merged.

The `--formats` syntax is unchanged. Under #82, release-default builds include the independent JPEG V1 verifier and enable eligible baseline JPEGs without an explicit flag; `--formats` remains an allowlist, not an addition.

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
{"type":"file","path":"photo.png","format":"png","status":"optimized","reason_code":null,"before":1258291,"after":1065000,"saved":193291,"guarantee":"format_equivalent","duration_ms":412,"note":null,"error":null}
{"type":"file","path":"notes.txt","format":null,"status":"unsupported","reason_code":"unsupported_format","before":41,"after":41,"saved":0,"guarantee":"byte_identical","duration_ms":0,"note":null,"error":null}
{"type":"summary","schema":2,"files":2,"optimized":1,"would_optimize":0,"already_optimal":0,"unsupported":1,"skipped":0,"failed":0,"before":1258332,"after":1065041,"saved":193291,"duration_ms":414,"dry_run":false,"strip_metadata":false}
```

Field rules:

| Field | Type | Notes |
| --- | --- | --- |
| `type` | string | `file` or `summary` |
| `path` | string | lossy UTF-8 rendering of the path, for humans |
| `path_raw_b64` | string, optional | base64 of the raw path bytes; present on Unix only when the path is not valid UTF-8 |
| `format` | string or null | handler id, from `Format::id`: `png`, `gzip`, `zip`, `ico`, `jpeg`, `tiff`, `zlib`, plus `fits`, `flac` and `pdf` when compiled. An APNG is reported as `png`, because the PNG handler is what rewrites it |
| `status` | string | `optimized`, `would_optimize`, `already_optimal`, `unsupported`, `skipped`, `failed` |
| `reason_code` | string or null | stable machine-readable explanation; `null` for `optimized`/`would_optimize`, a stable identifier for other outcomes. `status`, `note` and `error` retain their existing meanings and diagnostics |
| `before`, `after` | integer | file sizes in bytes; equal for every non-optimizing outcome |
| `saved` | integer | `before - after` |
| `guarantee` | string | `format_equivalent` for `optimized` / `would_optimize`; `byte_identical` otherwise |
| `metadata_stripped` | bool | present and `true` only when `--strip-metadata` rewrote this file |
| `duration_ms` | integer | wall time spent on this file |
| `note` | string or null | why the file landed in this status: the refusal reason for `skipped`, the filesystem reason for a skipped replacement |
| `error` | object or null | present **only** with `"status":"failed"`, otherwise `null`: `{"kind": "...", "message": "..."}`. `kind` is one of `io`, `parse`, `verify`, `internal` — never `unsupported`, `limit` or `refused`, because those are not failures: a refusal is a `skipped` status with the reason in `note`, and unsupported content is the `unsupported` status, with the reason in `note` only when a handler overruled the detector |
| `schema` | integer | present in the summary only; identifies the JSON result schema version |
| `dry_run` | boolean | present in the summary only |
| `strip_metadata` | boolean | present in the summary only; mirrors the explicit run flag |

`reason_code` values in result schema 2 are stable identifiers:

| Code | Meaning |
| --- | --- |
| `aborted_by_fail_fast` | this path was not started after the run stopped |
| `above_maximum_size`, `below_minimum_size` | configured file-size admission bound |
| `confirmation_declined` | FITS consent was not granted; original is unchanged |
| `configured_limit_exceeded`, `memory_budget_exceeded`, `directory_depth_exceeded` | an explicit resource or traversal bound refused work |
| `filesystem_error`, `input_parse_error`, `verification_failed`, `internal_error` | corresponding failure class; see the unchanged `error` object for details |
| `format_not_compiled`, `format_not_selected`, `unsupported_format`, `handler_rejected_signature` | no usable selected handler or a handler rejected a weak signature |
| `handler_refused` | a recognized input was refused by handler policy |
| `multiple_hard_links`, `read_only_file`, `ownership_change_denied`, `not_a_regular_file`, `symbolic_link_not_followed`, `invalid_symbolic_link_target` | file/path safety policy |
| `no_smaller_candidate` | no acceptable smaller candidate was produced |
| `path_filtered_by_include_exclude` | excluded by the extension filters |
| `skipped_unclassified`, `failed_unclassified` | safe fallback if an outcome lacks a more specific code |

Codes do not replace the human `note` or detailed `error.message`. Consumers must treat an unknown code as an unknown reason, retain the original `status` and diagnostics, and must not infer admission, consent or success from a code they do not understand. Unknown object fields may be ignored only when the consumer's behavior does not depend on them. A consumer requiring a schema version it does not recognize must reject the stream and fail closed rather than guess. `schema` is on the final summary record; a streaming consumer that must decide before then should buffer records or treat unrecognized additions as non-authorizing.

### Capabilities document

`tzim --capabilities` is an informational command that writes exactly one JSON object, not an NDJSON file stream. It performs no directory traversal, file reads, writes, or confirmation prompts. Its `capabilities_schema_version` is `1`; the `result_schema_version` it describes is `2` (the `schema` value on a `--json` summary record).

The object keeps distinct:

- `build_features`: boolean feature flags that affect public formats or optimization options;
- `compiled_formats`: handlers present in this binary, regardless of run selection;
- `default_enabled_formats`: handlers admitted without a run-time handler opt-in. FITS is deliberately omitted even in a build with the `fits` feature, because `--fits-tile` is separate explicit consent;
- `selection_flags`: `--formats` is an allowlist, not a force flag, and never grants a handler-specific opt-in;
- `handler_opt_ins`: JPEG's independent `jpeg-v1` default-admission gate, codec-only JPEG's explicit `--formats jpeg`, FITS's `--fits-tile`, and the optional FLAC/PDF build features;
- `transformations` and `confirmation_policies`: metadata removal, nested processing, Zopfli, and the FITS visible `IMAGE`-to-`BINTABLE` transform, including the direct-single-file TTY default-No prompt and the explicit non-interactive behavior.

A feature-limited binary reports its own actual feature booleans and format arrays; `--all-features` reports its wider set. A compiled or listed format is not evidence of user consent. Consumers must require the exact capability and schema fields their action depends on; an unknown schema version or unknown opt-in policy must fail closed. Unknown fields can be ignored only when they do not affect that decision.

A representative default release-like build emits this one-line JSON object (shown pretty-printed here):

```json
{
  "type": "capabilities",
  "capabilities_schema_version": 1,
  "result_schema_version": 2,
  "tzim_version": "0.15.0",
  "build_features": {"default": true, "png": true, "gzip": true, "zip": true, "ico": true, "jpeg": true, "jpeg-v1": true, "tiff": true, "zlib": true, "fits": false, "flac": false, "pdf": false, "zopfli": true},
  "compiled_formats": ["png", "gzip", "zip", "ico", "jpeg", "tiff", "zlib"],
  "default_enabled_formats": ["png", "gzip", "zip", "ico", "jpeg", "tiff", "zlib"],
  "selection_flags": [{"flag": "--formats", "semantics": "allowlist", "scope": "compiled_formats", "default_set": "default_enabled_formats", "does_not_grant_handler_opt_ins": true}],
  "handler_opt_ins": [
    {"format": "jpeg", "available": true, "kind": "independent_verifier_default_gate", "build_feature": "jpeg", "default_admission_feature": "jpeg-v1", "default_enabled": true, "codec_only_selection_flag": "--formats jpeg"},
    {"format": "fits", "available": false, "kind": "visible_transform_consent", "build_feature": "fits", "runtime_opt_in_flag": "--fits-tile", "default_enabled": false, "allowlist_is_not_consent": true},
    {"format": "flac", "available": false, "kind": "optional_build_feature", "build_feature": "flac", "feature_in_default_set": false, "enabled_by_default_when_compiled": true, "runtime_opt_in_flag": null},
    {"format": "pdf", "available": false, "kind": "optional_build_feature", "build_feature": "pdf", "feature_in_default_set": false, "enabled_by_default_when_compiled": true, "runtime_opt_in_flag": null}
  ],
  "transformations": [
    {"flag": "--strip-metadata", "available": true, "default": false, "effect": "removes only the documented per-format metadata list"},
    {"flag": "--nested", "available": true, "default": false, "effect": "also considers supported files inside archives"},
    {"flag": "--zopfli", "available": true, "default": false, "effect": "uses the slower, stronger DEFLATE encoder where supported"},
    {"flag": "--fits-tile", "available": false, "default": false, "effect": "converts an eligible FITS IMAGE HDU to a tile-compressed BINTABLE", "confirmation_policy": "fits_tile_confirmation"}
  ],
  "confirmation_policies": [{"id": "fits_tile_confirmation", "flag": "--fits-tile", "available": false, "interactive": {"eligible_for_single_direct_regular_file_only": true, "requires_stdin_and_stderr_tty": true, "prompt_stream": "stderr", "default_answer": "no", "runs_after_candidate_verification": true, "only_yes_commits": true}, "non_interactive": {"prompt": false, "flag_is_explicit_opt_in": true}, "decline": {"status": "skipped", "original_unchanged": true}, "never_prompts_for": ["--json", "--dry-run", "recursive_or_batch_runs", "nested_work", "filtered_or_refused_inputs", "candidate_or_verification_failures"]}],
  "side_effects": {"scans_files": false, "writes_files": false, "prompts": false}
}
```

JSON strings are escaped according to RFC 8259, with `\u00XX` escapes for control characters. Result and
capabilities schema versions evolve independently. Field meanings and enum values are stable within each
schema; additive optional fields may be introduced in a minor release, while a breaking field or meaning change
increments that document's schema version. Object keys are stable. Consumers must fail closed when a schema
version required for their decision is unknown.

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
| jobs | 4 (2 with `--zopfli`) | bounds how many files are in flight; it does **not** bound memory — see [safety §7.1](safety.md#71-memory--an-enforced-budget) |
| memory budget, per file | `6 × max-size + 8 × max-pixels + max-uncompressed + 128 MiB` | bounds the bytes one job may cost, including PDF and FLAC scratch; a file above it is skipped before it is read |
| recursion | off | an explicit flag is safer than a surprise tree walk |
| symlink following | off | avoids surprising writes and infinite walks |
| hard-linked files | skipped | replacement would break the link for other paths |
| `fsync` | on | a replaced file should survive a crash |

Limits produce `skipped`, never `failed`: hitting a configured bound is not an error.

`--jobs` is about parallelism, not about memory: the whole file and its candidate are held in memory, and a
decoded image can cost hundreds of megabytes per side. Bytes are bounded by `--memory-budget`, which is
checked against a computed worst case **before the file is read**: a file above it is `skipped` with a
reason and never reaches `fs::read`. The charge covers the file, the candidate read back for verification,
the decoded buffers on both sides of the comparison, and — for a container — `--max-uncompressed`.

The default is derived from the other ceilings and therefore never skips a file that they admit, which also
means it is not protection against a machine with less memory than the ceilings promise. Say how much you
have:

```console
$ tzim --jobs 4 --memory-budget 2147483648 ~/Photos
```

The budget is per file, so `--jobs` files can be charged at once: compare `--jobs × --memory-budget` with
RAM. `--max-pixels` and `--max-uncompressed` lower the charge itself; the full table is in
[safety §7.1](safety.md#71-memory--an-enforced-budget).

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
⠹ 01:12 [████████▏···]  37/148  6.2/s  eta 00:18
```

The first line is a single file: there is no counter to show, so the spinner and the verb are the whole
message. The second is a batch: the clock, the bar and the position, in the order the eye looks for them,
and then the two numbers that answer "how long" — the rate and, once it can be trusted, the estimate.

- the spinner has ten braille frames and advances every 100 ms;
- the bar is eight cells of eight sub-cells each — 64 positions between empty and full, so a bar over a
  hundred files still moves — drawn only when there is more than one file. A finished run pins it full;
- the rate is taken over the last eight completions, so it follows a change in file size instead of
  averaging the whole run; it appears after the second completion and not before, because one sample is
  not a rate;
- `eta` appears only after **three seconds and two completions**. Under those conditions the number says
  more about the first file than about the run, and a wildly wrong estimate is worse than none;
- the clock is `mm:ss`, counted from the start of the run. Past 99 minutes it grows to three digits rather
  than wrapping, because a run that long is a thing the reader wants to know about;
- when the terminal is too narrow for all of it, `eta` goes first, then the rate: the budget is spent on the
  spinner, the clock, the position and the bar, which is what the eye uses. A file name is never printed —
  it would make the line as wide as the deepest directory and turn a status indicator into noise;
- the line appears only after 150 ms, so a run that finishes quickly does not flash, and it is erased before
  the first result is printed, so the report never shares a row with it. Every frame is padded to the width
  of the previous one, so a shorter frame does not leave debris;
- the width comes from `COLUMNS` when it is set and parses, and from 80 otherwise. Truncation counts
  **display cells**, not bytes, through the same wcwidth table the report uses — a CJK or combining file
  name cannot push the bar off the screen, and the tail of the line is never cut mid-escape;
- the spinner and the bar are coloured by the same decision as the rest of stderr (the table above) and are
  plain when colour is off, so `NO_COLOR=1` produces no escape sequence anywhere;
- it is written to stderr only. stdout carries the report, one result per line, and nothing else.

It is drawn only when **all** of these hold: stderr is a terminal, the run has at least one file, and
`--quiet`, `--json` and `--no-progress` are absent. A line of `\r` frames belongs on a terminal; in a log
file or a CI transcript it is noise that hides the lines around it.

The drawing is `src/progress.rs` — about 250 lines, no dependency. Until v0.13 it was
[`indicatif`](https://crates.io/crates/indicatif) 0.18, which cost 96 KiB, a tenth of the binary, for one
line; [dependencies §2.9](dependencies.md#29-indicatif--added-in-v010-removed-in-v013) records both
decisions, and the removal was on the condition that the line get *better*, which is where the 64-position
bar, the recency-weighted rate and the two ETA gates come from.

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
