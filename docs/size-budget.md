# Binary size budget

Related: [dependencies](dependencies.md) · [roadmap](roadmap.md)

A small binary is a project requirement, not a nice-to-have. But "small" is only meaningful when it is
measured, so this document defines the budget, the build profile, and the exact procedure used to check it.

> **Current status:** pre-alpha. Nothing has been built yet, so **no size number in this document is a
> measurement**. The targets below are provisional budgets to be validated and tightened at v0.1, when the
> first binary exists.

---

## 1. Budget

These are **upper bounds** for the stripped release binary. Exceeding one fails CI.

| Profile | Features | Budget | Rationale |
| --- | --- | --- | --- |
| `minimal` | none (CLI, walker, replacement pipeline) | 700 KiB | proves the core is thin; also a quick sanity check for accidental pulls |
| `core` | `png,gzip` | 1.5 MiB | the v0.1 shipping configuration |
| `core+zopfli` | `png,gzip,zopfli` | 1.8 MiB | +300 KiB allowed for the strongest encoder |
| `+zip` | `png,gzip,zip` | 2.0 MiB | v0.2 |
| `images` | `png,gzip,zip,ico,apng` | 2.2 MiB | v0.3, mostly reuses the PNG engine |
| `full` | everything, including `jpeg` | 2.6 MiB | v0.4 |

After the first real measurement, each budget is tightened to **measured size + 15 %**, so that regressions
are caught early rather than after they accumulate.

Reference point only (not a target): the official oxipng 10.2.1 Linux release archive is ~493 KiB
compressed, and Leanify advertises "under 1 MB" for a C++ tool covering many formats. We are in the same
order of magnitude, with a stricter contract.

---

## 2. Release profile

```toml
[profile.release]
opt-level = 3          # codecs are hot; "z" is tested separately, see §5
lto = "fat"
codegen-units = 1
panic = "abort"        # no unwinding tables; a panic is a bug, not a recoverable error
strip = "symbols"
debug = false
incremental = false
```

Notes:

- `panic = "abort"` means `Drop` guards do not run on panic. The original file is still intact (replacement
  happens last), but a temporary file may remain. This is documented in [safety](safety.md#5-failure-modes).
- `strip = "symbols"` is the default. `strip = "debuginfo"` and no stripping are compared once, and the
  smallest is chosen.
- LTO is "fat" because the tree is tiny; thin LTO was not worth the extra measurement complexity.
- No `opt-level = "s"` or `"z"` for the whole crate: it slows the DEFLATE/PNG hot paths measurably and the
  saving is small. Per-package overrides are evaluated per release.

---

## 3. Feature matrix

Every format family is an opt-in feature; the default is `full`.

```
default = ["png", "gzip", "zip", "ico", "apng"]
png     = []            # PNG + APNG engine, needs flate2, crc32fast, png
gzip    = []            # gzip container, needs flate2, crc32fast
zip     = []            # ZIP rewriter, needs flate2, crc32fast
ico     = ["png"]
apng    = ["png"]
jpeg    = []            # opt-in at runtime too: requires --formats jpeg
zopfli  = ["dep:zopfli"]
```

Build examples:

```sh
# smallest useful build
cargo build --release --no-default-features --features png,gzip

# everything except the optional encoder
cargo build --release --no-default-features --features png,gzip,zip

# maximum compression
cargo build --release --features zopfli
```

`jpeg` is the only feature that is also disabled at runtime by default: its handler requires an explicit
`--formats jpeg`, because the expected gain is small and the risk is the highest in the project.

---

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

What gets recorded for every release, in `docs/benchmarks.md`:

- artifact size in bytes, per profile and per target triple;
- size of `.text`, `.rodata`, `.data`, and the total of everything else;
- top 10 crates by size with `cargo-bloat`;
- number of lines from `cargo tree`;
- wall-clock clean build time;
- the exact commit hash and toolchain version.

---

## 5. Experiments to run at v0.1

Each is measured, kept only if it helps, and recorded either way:

| Experiment | Expected effect | Decision rule |
| --- | --- | --- |
| `opt-level = "z"` globally | −5…15 % size, slower codecs | keep only if the size win exceeds 10 % and runtime cost is under 20 % |
| `opt-level = "z"` for the CLI/reporting modules only | small | per-package override if measurable |
| `strip = "debuginfo"` vs `"symbols"` vs none | small | pick the smallest |
| `panic = "abort"` (already on) | tens of KiB | stays |
| `-Z build-std=std,panic_abort -Z build-std-features=panic_immediate_abort` (nightly) | 20–30 % smaller | **not default**; offered only in a documented "minimal" build, because it needs nightly and weakens reproducibility |
| `flate2` `zlib-rs` backend instead of `miniz_oxide` | faster, possibly larger, adds `unsafe` | rejected by default; re-evaluate only if decode speed becomes the bottleneck |
| `zopfli` default-features off (already) | avoids `log`, `simd-adler32`, gzip/zlib wrappers | stays |
| `png` default-features off (already) | avoids `unstable`, `zlib-rs` | stays |
| UPX | −50…70 % on disk | **never**: false positives in antivirus, decompression at startup, additional attack surface, and it hides the real size instead of reducing it |
| dynamic linking against system zlib | smaller artifact | **never**: breaks static cross-platform builds and reproducibility |

---

## 6. CI gate

The size check runs on every pull request that touches `Cargo.toml`, `Cargo.lock`, or `src/`:

1. build the `core` profile for `x86_64-unknown-linux-musl`;
2. compare `stat -c %s` with the budget in §1 (a machine-readable budget file is added with the first
   implementation, so the numbers live in one place);
3. if the size grew by more than 5 % compared with the recorded baseline, the pull request must either
   justify it in the description or reduce it;
4. publish the number in the PR comment, so reviewers never have to guess.

The baseline is updated only by the release commit, together with `docs/benchmarks.md`.

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
