# Binary size budget

Related: [dependencies](dependencies.md) · [roadmap](roadmap.md)

A small binary is a project requirement, not a nice-to-have. But "small" is only meaningful when it is
measured, so this document defines the budget, the build profile, and the exact procedure used to check it.

> **Current status:** measured by CI on every pull request (`ci` job `binary size`).
> **Measured: 979 808 bytes (956 KiB)** for `x86_64-unknown-linux-musl`, `--release`, default features
> (`png,gzip,zip,ico,jpeg,tiff,zlib,zopfli`) — **69 632 bytes (68 KiB) smaller than v0.12.1**, on the run
> that also measured the feature matrix below. Per-feature measurements are in §3; the profile that got here
> is in §2, and what it cost in speed is in §5.
> **The progress line gave that back.** `indicatif` and `console` were 96 KiB of the v0.12.1 binary; v0.13
> draws the line in `src/progress.rs` and costs 0, which is what paid for everything else in this release.
> **`jpeg-v1` is not in the shipped default**: the independent JPEG decoder measures **+118 784 bytes**
> (116 KiB), and #39 approved it on the condition that the shipping binary does not grow past 1 049 440. It
> exists, CI tests it on every run, and `cargo build --release --features jpeg-v1` turns it on. The gate in
> `ci.yml` is now this release's own measurement, to the byte, rather than a ceiling with 150 KiB of room
> underneath — see the errata below.
> Details: [benchmarks.md](benchmarks.md#1-binary-size--measured).

### Errata: the v0.12.1 "same size" claim

v0.12.1 shipped with the CHANGELOG, the release notes and this document all saying 1 049 440 bytes, and the
five release **tarballs** grew by 514–634 bytes each. The suspicion was that the binary had grown and nobody
had measured it, because the `size` job only ever tested an upper bound (`bytes <= 1 207 296`) — [issue
#62](https://github.com/Rudolf1517/tzim/issues/62). The number was right; the inference from the assets was
wrong. The `size` job printed

```
static musl binary: 1049440 bytes (1024 KiB)
```

on **every** run of that pull request — the eight runs of PR #60, from the first commit to the merge commit
(`8989774`, `95cb565`, `be16474`, `c012ba5`, `0ad7edc`, `0af29c9`, `ec423bc`, `cad83eb`) — so no commit
changed the binary by a byte.

What moved is the packing. A tarball holds `tzim`, `LICENSE`, `man/tzim.1` and three completion scripts, `tar`
pads each member to 512 bytes, and gzip then compresses the whole padded stream. The man page's version
string is the only member that changes between two patch releases: its byte length is identical, so every
member keeps its offset, but its *compressed* length is not, and the difference shifts the DEFLATE stream
that the binary's bytes sit inside. A ~500-byte swing on a 536 636-byte `.tar.gz` is packing, not code; the
`.zip` for Windows stores the binary with its own deflate, which is why its delta is the largest of the five
and still under a kilobyte. The lesson is kept rather than deleted: a released artifact's *compressed* size is
not evidence about its contents, which is why from v0.13 the gate compares the binary with the previous
release's measurement instead of a ceiling that leaves 150 KiB of room underneath.

---

## 1. Budget

These are **upper bounds** for the stripped release binary, one row per build CI actually performs. A row
that CI does not build does not belong here: an unchecked budget is decoration.

| Profile | Features | Measured | vs v0.12.1 |
| --- | --- | --- | --- |
| `png` | `png` | 750 368 B (732 KiB) | −73 728 |
| `core` | `png,gzip` | 762 656 B (744 KiB) | −69 632 |
| `+zip` | `png,gzip,zip` | 799 520 B (780 KiB) | −69 632 |
| `images` | `png,gzip,zip,ico` | 811 808 B (792 KiB) | −69 632 |
| `+jpeg` | `png,gzip,zip,ico,jpeg` | 865 056 B (844 KiB) | −73 728 |
| **`default`** | `png,gzip,zip,ico,jpeg,tiff,zlib,zopfli` | **979 808 B (956 KiB)** | **−69 632** |
| `+jpeg-v1` | `png,gzip,zip,ico,jpeg,jpeg-v1` | 983 840 B (960 KiB) | −55 168 |
| `all but fuzz-api` | `png,gzip,zip,ico,jpeg,jpeg-v1,tiff,zlib,zopfli` | 1 102 688 B (1 076 KiB) | +53 248 |
| `default +fits` | `png,gzip,zip,ico,jpeg,tiff,zlib,zopfli,fits` | 1 016 672 B (992 KiB) | −32 768 |
| `all +fits` | `png,gzip,zip,ico,jpeg,jpeg-v1,tiff,zlib,zopfli,fits` | 1 139 552 B (1 112 KiB) | +90 112 |

Every row fell by 68–73 KiB without a handler changing: the progress line left the tree (§3), and the largest
file in it — the one dependency that was not behind a feature — went with it. `jpeg-v1` is measured by
subtraction: 983 840 − 865 056 = **118 784 bytes** on the lean row, and 1 102 688 − 979 808 = the same on the
full one. `fits` is measured the same way and comes out at **36 864 bytes** (36 KiB) on both:
1 016 672 − 979 808, and 1 139 552 − 1 102 688.

Two rows moved for a reason that is not a handler: `png,gzip,zip` and `all but fuzz-api` measure 4 096 bytes
above the figures they carried, because `flate2` 1.1.10 (`miniz_oxide` 0.9 alongside 0.8) is a version further
along than the run that wrote them. `default` itself did not move — 979 808, exactly the gate — which is the
number that decides whether a new feature may join it.

**The one attempt to make `jpeg-v1` cheaper is measured and on record.** Building that dependency at
`opt-level = "s"` — the profile `indicatif` and `console` used before they left — moved the lean row
983 840 → **967 456** and the full row 1 098 592 → **1 082 208**, so the feature's cost fell from 118 784 to
**102 400 bytes** (run [40223835591](https://github.com/Rudolf1517/tzim/actions/runs/40223835591)). That is
16 384 bytes bought, and **32 768 still over** the 1 049 440-byte condition of issue #39, so the override is
not in the tree. Anything that puts the second opinion inside the shipped binary has to find another 32 KiB,
or the condition has to be renegotiated with the number in hand — there is no third option, and this paragraph
is where the next attempt starts.

`default` is gated on every pull request: the job fails when the static binary exceeds the measured 979 808
bytes. That is a *zero-tolerance* gate, and it is deliberate: v0.12.1's gate allowed 150 KiB of headroom, so
a green `size` job proved nothing about growth, which is what [issue
#62](https://github.com/Rudolf1517/tzim/issues/62) was about. A compiler bump is the one thing that can move
this number without a code change; the pull request that needs it updates the number and says why here.

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

[profile.release.package.console]
opt-level = "s"

[profile.release.package.indicatif]
opt-level = "s"
```

Notes:

- `opt-level = "s"` applies to **this crate** and to the two crates that draw the progress line; everything
  else it depends on — `miniz_oxide`, `fdeflate`, `png`, `zopfli` — is built at `3`. The two are named
  individually because they are not codecs: the measurement behind that line is in §5.2. The time is in the codecs, the bytes are mostly here: the same corpus runs
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

There is **no unconditional crate** any more. Until v0.13 `indicatif` was one, because the progress line is
part of the interface rather than of a format handler; v0.13 draws that line in `src/progress.rs`, so every
dependency in the tree is behind a feature and every row below can drop it.

```toml
default = ["png", "gzip", "zip", "ico", "jpeg", "tiff", "zlib", "zopfli"]
png     = ["dep:png", "dep:flate2", "dep:crc32fast"]
gzip    = ["dep:flate2", "dep:crc32fast"]
zip     = ["dep:flate2", "dep:crc32fast"]
ico     = ["png"]
jpeg    = []
jpeg-v1 = ["jpeg", "dep:jpeg-decoder", "dep:crc32fast"]   # the independent JPEG verifier, off by default
tiff    = ["dep:flate2"]
zlib    = ["dep:flate2"]
fits    = ["dep:flate2", "dep:crc32fast"]   # tile compression; off by default, see guarantees §4.12
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
| `png` | 750 368 | 732 |
| `png,gzip` | 762 656 | 744 |
| `png,gzip,zip` | 803 616 | 784 |
| `png,gzip,zip,ico` | 811 808 | 792 |
| `png,gzip,zip,ico,jpeg` | 865 056 | 844 |
| `png,gzip,zip,ico,jpeg,tiff,zlib,zopfli` (default) | 979 808 | 956 |
| `png,gzip,zip,ico,jpeg,jpeg-v1` | 983 840 | 960 |
| `png,gzip,zip,ico,jpeg,tiff,zlib,zopfli,fits` | 1 016 672 | 992 |
| `png,gzip,zip,ico,jpeg,jpeg-v1,tiff,zlib,zopfli` | 1 102 688 | 1 076 |
| `png,gzip,zip,ico,jpeg,jpeg-v1,tiff,zlib,zopfli,fits` | 1 139 552 | 1 112 |

Each feature's cost is visible by subtracting the row above it: `gzip` +12 KiB, `zip` +40 KiB, `ico` +8 KiB,
the JPEG codec +52 KiB, and the jump from `+jpeg` to the default row is TIFF, zlib and Zopfli together
+112 KiB — of which Zopfli is +84 KiB and appears nowhere else, which is why it is the one feature worth
turning off for a container or an installer. `jpeg-v1` is +116 KiB and `fits` is **+36 KiB**; both ship off,
`jpeg-v1` because it would break the condition #39 put on it and `fits` because it would break the gate on the
`default` row.

`fits` is the cheapest handler since zlib: it adds no crate ([dependencies
§2.11](dependencies.md#211-fits-no-cfitsio-and-why-that-is-not-a-gap)), and 36 KiB is what a parser, a table
writer and a verifier cost when the codec is already in the tree. The guess in issue #70 was 60–100 KiB; the
measurement is 36 864 bytes, and the guess is recorded here rather than quietly replaced.

The rows above `default` are the narrowing path a build takes when it wants a smaller binary; the rows below it
are the widening path — `jpeg-v1`, `fits`, and both together — taken only by `--all-features` in CI and by
anyone who wants the second opinion on a JPEG, or a second implementation of the FITS transform, inside the
binary.

The same deltas at `opt-level = 3` for this crate were 8, 49, 16, 65, 32 and 98 KiB — the profile change made
visible inside each handler, because the codecs are already compiled for speed.

The deltas are what a user saves by narrowing the build, not estimates: `png,gzip` is the smallest useful
optimizer (PNG and `.gz`), `png,gzip,zip,ico` is the full lossless set without JPEG, `+tiff` adds the one
format that is on by default, and the last row is the published binary.

`jpeg` and `fits` are the two features that are *also* off at runtime: `jpeg`'s handler requires an explicit
`--formats jpeg`, because the expected gain is small and the verification is the weakest in the project
([guarantees §2.6](guarantees.md#26-verification-independence)), and `fits`'s requires `--fits-tile`, because it
turns an `IMAGE` HDU into a `BINTABLE`
([guarantees §4.12](guarantees.md#412-fits--tile-compressed-image-hdus-v014-opt-in)).

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
| `indicatif` and `console` at `opt-level = 3` (the wildcard in §2) | 1 065 824 bytes in v0.10.0 | rejected: **−16 384 bytes** for code that runs ten times a second and touches no file byte, so §2 now names both crates and sets `"s"` for them alone |

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

1. builds the shipping profile (default features) for `x86_64-unknown-linux-musl`, and fails unless `file`
   reports it as static (`static-pie linked` since the musl target links position-independent);
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
