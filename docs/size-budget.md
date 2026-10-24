# Binary size budget

Related: [dependencies](dependencies.md) · [roadmap](roadmap.md)

A small binary is a project requirement, not a nice-to-have. But "small" is only meaningful when it is
measured, so this document defines the budget, the build profile, and the exact procedure used to check it.

> **Current status:** measured by CI on every pull request (`ci` job `binary size`).
> **Measured: 1 049 440 bytes (1 024 KiB)** for `x86_64-unknown-linux-musl`, `--release`, default features
> (`png,gzip,zip,ico,jpeg,tiff,zopfli`), v0.7.0. Per-feature measurements are in §3.
> Details: [benchmarks.md](benchmarks.md#1-binary-size--measured).

---

## 1. Budget

These are **upper bounds** for the stripped release binary, one row per build CI actually performs. A row
that CI does not build does not belong here: an unchecked budget is decoration.

| Profile | Features | Measured | Budget (+15 %) |
| --- | --- | --- | --- |
| `png` | `png` | 756 KiB | 870 KiB |
| `core` | `png,gzip` | 764 KiB | 880 KiB |
| `+zip` | `png,gzip,zip` | 812 KiB | 935 KiB |
| `images` | `png,gzip,zip,ico` | 828 KiB | 955 KiB |
| `+jpeg` | `png,gzip,zip,ico,jpeg` | 892 KiB | 1 030 KiB |
| `+tiff` | `png,gzip,zip,ico,jpeg,tiff` | 924 KiB | 1 063 KiB |
| `full` | `png,gzip,zip,ico,jpeg,tiff,zopfli` | 1 024 KiB | 1 178 KiB |

`full` is gated on every pull request: the job fails when the static binary exceeds the budget. The other
rows are reported by the same job (§3) and are the numbers the next tightening uses. The policy is
*measured + 15 %*, so a change that costs more than a sixth of the current size has to be justified in the
pull request.

Reference point only (not a target): the official oxipng 10.2.1 Linux release archive is ~493 KiB
compressed, and Leanify advertises "under 1 MB" for a C++ tool covering many formats. We are in the same
order of magnitude, with a stricter contract.

---

## 2. Release profile

Exactly what `Cargo.toml` contains today:

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
```

Notes:

- `panic = "abort"` means `Drop` guards do not run on a panic. The original file is still intact
  (replacement happens last), but a temporary file may remain. This is documented in
  [safety](safety.md#5-failure-modes).
- `strip` is **not** set, so symbols stay in the binary. That is deliberate: a panic message with a symbol
  name is worth more to whoever has to report the bug than the bytes it costs, and the budget is set with
  them included. If a future release needs the space, `strip = "symbols"` is the first thing to try, and the
  experiment in §5 records the decision either way.
- `debug` and `incremental` are Cargo's defaults for a release profile (off and off); they are not spelled
  out because writing them down would suggest a measurement that never happened.
- LTO is "fat" because the tree is tiny; thin LTO was not worth the extra measurement complexity.
- No `opt-level = "s"` or `"z"` for the whole crate: it slows the DEFLATE and PNG hot paths, and there is no
  measurement in this repository showing the size win would pay for it.

---

## 3. Feature matrix

One feature per handler family, exactly as `Cargo.toml` declares them. `default` is what the published
binaries are built with; everything else is a way to build a smaller tool for a narrower job.

```toml
default = ["png", "gzip", "zip", "ico", "jpeg", "tiff", "zopfli"]
png     = ["dep:png", "dep:flate2", "dep:crc32fast"]
gzip    = ["dep:flate2", "dep:crc32fast"]
zip     = ["dep:flate2", "dep:crc32fast"]
ico     = ["png"]
jpeg    = []
tiff    = ["dep:flate2"]
zopfli  = ["dep:zopfli"]
fuzz-api = []          # never in a release: it only exposes the parsers to fuzz/
```

`ico` is a thin layer over `png` (an icon is a directory plus embedded PNGs), `jpeg` gates this
repository's own codec, so it adds code without adding a crate, and `tiff` reuses `flate2`, which PNG, gzip
and ZIP already bring in. Turning a feature off removes the handler
from `--list-formats`, makes `--formats <name>` a usage error instead of a silent no-op, and reports files
of that format as `skipped: this build has no <format> handler`.

Measured on `x86_64-unknown-linux-musl`, release profile, cold `target/`, one build per row
(`ci` job `binary size`, step `Feature matrix`):

| Features | Bytes | KiB |
| --- | --- | --- |
| `png` | 774 944 | 756 |
| `png,gzip` | 783 136 | 764 |
| `png,gzip,zip` | 832 288 | 812 |
| `png,gzip,zip,ico` | 848 672 | 828 |
| `png,gzip,zip,ico,jpeg` | 914 208 | 892 |
| `png,gzip,zip,ico,jpeg,tiff` | 946 976 | 924 |
| `png,gzip,zip,ico,jpeg,tiff,zopfli` | 1 049 440 | 1 024 |

Each feature's cost is visible by subtracting the row above it: `gzip` +8 KiB, `zip` +49 KiB, `ico`
+16 KiB, the JPEG codec +65 KiB, the TIFF handler +32 KiB, Zopfli +98 KiB.

The deltas are what a user saves by narrowing the build, not estimates: `png,gzip` is the smallest useful
optimizer (PNG and `.gz`), `png,gzip,zip,ico` is the full lossless set without JPEG, `+tiff` adds the one
format that is on by default, and the last row is the published binary.

`jpeg` is the only feature that is *also* off at runtime by default: its handler requires an explicit
`--formats jpeg`, because the expected gain is small and the verification is the weakest in the project
([guarantees §2.6](guarantees.md#26-verification-independence)).

Build examples:

```sh
# smallest useful build
cargo build --release --no-default-features --features png,gzip

# everything except JPEG and the optional encoder
cargo build --release --no-default-features --features png,gzip,zip,ico

# the published binary
cargo build --release
```

## 4. Measurement procedure

Performed on Linux x86_64, with a cold `target/` directory, for each profile in §1.

```sh
# 1. size of the artifact, static musl build
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl --no-default-features --features png,gzip
ls -l target/x86_64-unknown-linux-musl/release/tzim

# 2. section breakdown
llvm-size -A target/x86_64-unknown-linux-musl/release/tzim | sort -k2 -n

# 3. per-crate attribution
cargo install cargo-bloat --locked
cargo bloat --release --target x86_64-unknown-linux-musl \
            --no-default-features --features png,gzip -n 40 --crates

# 4. confirm it is really static and C-free
file target/x86_64-unknown-linux-musl/release/tzim     # "statically linked"
ldd  target/x86_64-unknown-linux-musl/release/tzim     # "not a dynamic executable"

# 5. dependency count
cargo tree --edges normal --no-dev --prefix none | sort -u | wc -l

# 6. build time (clean build, informational)
cargo clean && /usr/bin/time -v cargo build --release --target x86_64-unknown-linux-musl \
            --no-default-features --features png,gzip
```

What CI records, and what therefore appears in `docs/benchmarks.md`:

- the artifact size in bytes for the shipping profile, and the size of every feature combination in §3;
- the number of `cargo tree` lines, as a dependency-count tripwire;
- the `llvm-size` section table for the shipping binary, when the tool is present;
- the commit the run was built from (the job summary names the workflow run, not just a hash).

What it does **not** record, and which no report here claims: `cargo-bloat` attribution and clean build time.
Both are useful and neither runs today; a number that this project has not measured does not appear in a
table that looks measured.

---

## 5. Options considered

**None of the rows below is a measurement taken in this repository.** They are recorded decisions with the
reasoning that led to them, kept so the next person does not have to re-litigate them from memory. A row
with a number attached would say so; none has one.

| Experiment | Expected effect | Decision rule |
| --- | --- | --- |
| `opt-level = "z"` globally | −5…15 % size, slower codecs | keep only if the size win exceeds 10 % and runtime cost is under 20 % |
| `opt-level = "z"` for the CLI/reporting modules only | small | per-package override if measurable |
| `strip = "debuginfo"` vs `"symbols"` vs none | small | **decided: none** — symbols are kept so a panic can name a function; revisit only if the budget gets tight |
| `panic = "abort"` (already on) | tens of KiB | stays |
| `-Z build-std=std,panic_abort -Z build-std-features=panic_immediate_abort` (nightly) | 20–30 % smaller | **not default**; offered only in a documented "minimal" build, because it needs nightly and weakens reproducibility |
| `flate2` `zlib-rs` backend instead of `miniz_oxide` | faster, possibly larger, adds `unsafe` | rejected by default; re-evaluate only if decode speed becomes the bottleneck |
| `zopfli` default-features off (already) | avoids `log`, `simd-adler32`, gzip/zlib wrappers | stays |
| `png` default-features off (already) | avoids `unstable`, `zlib-rs` | stays |
| UPX | −50…70 % on disk | **never**: false positives in antivirus, decompression at startup, additional attack surface, and it hides the real size instead of reducing it |
| dynamic linking against system zlib | smaller artifact | **never**: breaks static cross-platform builds and reproducibility |

---

## 6. CI gate

The `binary size` job in `.github/workflows/ci.yml` runs on every pull request — there is no path filter,
because a handler change can move the size just as a `Cargo.toml` change can:

1. builds the shipping profile (default features) for `x86_64-unknown-linux-musl`;
2. fails when `stat -c %s` exceeds the `full` budget in §1;
3. builds every feature combination in §3 in the same job and prints the table, so a pull request cannot
   claim a size saving without the number;
4. publishes the whole report as a PR comment.

The budget lives in the workflow (one `budget=` line next to the check) and in §1. When a release changes
the size, both are updated in the same pull request: a document that disagrees with the gate is worse than
no document.

---

## 7. Non-negotiables

The budget never justifies:

- skipping verification (that would double the saving in time, not in size — and destroy the point of the
  project);
- removing checksum validation;
- dropping the independent decoder;
- using `unsafe` to shave bytes;
- linking against a system C library to make the artifact smaller on one platform while breaking another.

If the budget and correctness ever conflict, correctness wins and the budget is raised after a written
justification in this document.
