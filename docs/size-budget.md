# Binary size budget

Related: [dependencies](dependencies.md) · [roadmap](roadmap.md)

A small binary is a project requirement, not a nice-to-have. But "small" is only meaningful when it is
measured, so this document defines the budget, the build profile, and the exact procedure used to check it.

> **Current status:** measured by CI on every pull request (`ci` job `binary size`).
> **Measured: 942 944 bytes (920 KiB)** for `x86_64-unknown-linux-musl`, `--release`, default features
> (`png,gzip,zip,ico,jpeg,tiff,zopfli`), v0.8.0. Per-feature measurements are in §3; the profile that got
> here is in §2, and what it cost in speed is in §5.
> Details: [benchmarks.md](benchmarks.md#1-binary-size--measured).

---

## 1. Budget

These are **upper bounds** for the stripped release binary, one row per build CI actually performs. A row
that CI does not build does not belong here: an unchecked budget is decoration.

| Profile | Features | Measured | Budget (+15 %) |
| --- | --- | --- | --- |
| `png` | `png` | 708 KiB | 814 KiB |
| `core` | `png,gzip` | 712 KiB | 818 KiB |
| `+zip` | `png,gzip,zip` | 752 KiB | 864 KiB |
| `images` | `png,gzip,zip,ico` | 764 KiB | 878 KiB |
| `+jpeg` | `png,gzip,zip,ico,jpeg` | 816 KiB | 938 KiB |
| `+tiff` | `png,gzip,zip,ico,jpeg,tiff` | 844 KiB | 970 KiB |
| `full` | `png,gzip,zip,ico,jpeg,tiff,zopfli` | 920 KiB | 1 059 KiB |

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
opt-level = "s"
lto = "fat"
codegen-units = 1
panic = "abort"
strip = "symbols"
incremental = false

[profile.release.package."*"]
opt-level = 3
```

Notes:

- `opt-level = "s"` applies to **this crate**; everything it depends on — `miniz_oxide`, `fdeflate`, `png`,
  `zopfli` — is built at `3`. The time is in the codecs, the bytes are mostly here: the same corpus runs
  3.6 % slower this way instead of 11 %, and the binary is 104 KiB smaller (§5 has all three measurements).
- `panic = "abort"` means `Drop` guards do not run on a panic. The original file is still intact
  (replacement happens last), but a temporary file may remain. This is documented in
  [safety](safety.md#5-failure-modes).
- `strip = "symbols"` removes the symbol table. `strip = true` measured byte-identical (§5): there is no
  debug information to remove in the first place. A panic in the released binary therefore prints addresses;
  the message and the exit code are unaffected.
- `lto = "fat"` and `codegen-units = 1` are what make cross-crate inlining possible; thin LTO measured
  106 KiB *larger* (§5), so the extra link time buys size rather than costing it.
- `debug` and `incremental` are Cargo's defaults for a release profile (off and off); they are not spelled
  out because writing them down would suggest a measurement that never happened.

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
| `png` | 725 792 | 708 |
| `png,gzip` | 729 888 | 712 |
| `png,gzip,zip` | 770 848 | 752 |
| `png,gzip,zip,ico` | 783 136 | 764 |
| `png,gzip,zip,ico,jpeg` | 836 384 | 816 |
| `png,gzip,zip,ico,jpeg,tiff` | 865 056 | 844 |
| `png,gzip,zip,ico,jpeg,tiff,zopfli` | 942 944 | 920 |

Each feature's cost is visible by subtracting the row above it: `gzip` +4 KiB, `zip` +40 KiB, `ico`
+12 KiB, the JPEG codec +52 KiB, the TIFF handler +28 KiB, Zopfli +76 KiB. The same deltas measured at
`opt-level = 3` were 8, 49, 16, 65, 32 and 98 KiB, which is the profile change made visible in each
handler: the codecs shrink least, because they are already compiled for speed.

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

### 5.1 The measurements behind the current profile

One CI job built all of these from the same commit — v0.7.1's source, before the v0.8.0 interface work added
its 20 KiB to every row — and ran the same corpus through each (694 real files, `--recursive`, one job, warm
cache, two timed runs per build). "Bytes written" is what the corpus run
produced: it is identical in every row, which is the only result that matters — a compiler flag cannot
change a zlib stream, so a difference here would have been a bug.

| Build | Bytes | KiB | PNG corpus | JPEG corpus | Bytes written |
| --- | --- | --- | --- | --- | --- |
| `opt-level = 3` (previous) | 1 028 960 | 1 004 | 9 050 ms | 304 ms | 379 177 |
| `opt-level = 3` (repeat) | 1 028 960 | 1 004 | 9 063 ms | 308 ms | 379 177 |
| `opt-level = "s"` (whole tree) | 901 984 | 880 | 10 073 ms | 321 ms | 379 177 |
| `opt-level = "s"` (repeat) | 901 984 | 880 | 10 073 ms | 329 ms | 379 177 |
| **`"s"` + deps `3` (chosen)** | **922 464** | **900** | **9 390 ms** | **311 ms** | 379 177 |
| `"s"` + deps `3` (repeat) | 922 464 | 900 | 9 379 ms | 314 ms | 379 177 |

Reading of the table: shrinking the whole tree buys 124 KiB and costs 11 % on real PNGs; shrinking only
this crate buys 104 KiB and costs 3.6 % (≈60 ms per 1 MiB corpus, JPEG +2 %), because the time is spent
inside the codecs. The remaining 20 KiB would have been paid for with three times the slowdown, so the
mixed profile is the trade this project takes. Both repeats agree to within 15 ms, so these are not
noise-limited numbers.

### 5.2 Earlier trials in the same pass

Measured the same way but before the `core::fmt` change described in §5.3, so each figure is roughly
20 KiB above what the same variant would measure today. They are kept because they are the reason the
chosen combination is what it is; none of them is a claim about the shipped binary.

| Experiment | Result | Decision |
| --- | --- | --- |
| `opt-level = 2` | 1 020 768 (−8 KiB vs `3`) | pointless: 8 KiB for a full optimisation level |
| `opt-level = "z"` | 926 560 | rejected: smaller than `3`, larger than `"s"`, slower than both |
| `lto = "thin"` | 1 008 480 at `opt=s` | rejected: 106 KiB larger than fat LTO |
| `lto = false` | 1 131 368 at `opt=s` | rejected |
| `codegen-units = 16` | 922 464 at `opt=s` | rejected: no smaller than the mixed profile, and gives up cross-crate inlining |
| `strip = true` instead of `strip = "symbols"` | 1 049 440, identical | no effect: there is no debug information to strip |
| `-Wl,--gc-sections` | 1 049 440, identical | no effect: LTO already removes what is unreachable |
| `-C force-unwind-tables=no` | identical at both `opt=3` and `opt=s` | no effect, and a non-standard flag is not worth zero bytes |

### 5.3 What is left, and why it is not taken

`cargo bloat`-style attribution of the 1 049 440-byte binary put 165 KiB in `gimli`/`addr2line`/`unwind`
symbols, which looks like free money. It is not: a hello-world Rust binary with `panic = "abort"` and
`strip = "symbols"` carries 127 `gimli` symbols and 107 unwind symbols of its own, so that stack is `std`'s
and no dead-code pass over this repository can remove it. `panic = "abort"` is already on; the only way
past it is `-Z build-std` on nightly, which §5.4 rejects.

The other size lever was `core::fmt`, at 17.5 KiB: formatting a `u64` with `{}` pulls in the general
formatter. `src/report.rs` now computes sizes, percentages and durations with integer arithmetic and its
own two-decimal rounding, which removes that path from the binary without changing a single byte of output
format. That is the shape of every remaining win here: code that is compiled but whose general case is
never reached.

### 5.4 Options rejected without measurement in this repository

Recorded decisions with the reasoning that led to them, kept so the next person does not have to
re-litigate them from memory. A row with a number attached would say so; none has one.

| Experiment | Expected effect | Decision rule |
| --- | --- | --- |
| `-Z build-std=std,panic_abort -Z build-std-features=panic_immediate_abort` (nightly) | 20–30 % smaller | **not default**; it needs nightly, weakens reproducibility, and would change the toolchain every user needs to match a release |
| `flate2` `zlib-rs` backend instead of `miniz_oxide` | faster, possibly larger, adds `unsafe` | rejected by default; re-evaluate only if decode speed becomes the bottleneck |
| `zopfli` default-features off (already) | avoids `log`, `simd-adler32`, gzip/zlib wrappers | stays |
| `png` default-features off (already) | avoids `unstable`, `zlib-rs` | stays |
| UPX | −50…70 % on disk | **never**: false positives in antivirus, decompression at startup, additional attack surface, and it hides the real size instead of reducing it |
| dynamic linking against system zlib | smaller artifact | **never**: breaks static cross-platform builds and reproducibility |

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
