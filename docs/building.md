# Building from source

Related: [dependencies](dependencies.md) · [size budget](size-budget.md) · [formats](formats.md) · [testing](testing.md)

Every prebuilt channel — `install.sh`, the Homebrew formula, the `.deb` and `.rpm` packages, the Windows
installer and the local winget manifest — installs one binary built with Cargo's `default` feature set. That
set covers PNG/APNG, gzip, ZIP, ICO/CUR, baseline JPEG, TIFF and zlib. It does **not** contain FITS, FLAC or
PDF: the handlers are in the repository and CI compiles them, but a released binary leaves those files
alone — `skipped`, with `this build has no <format> handler` — until it is rebuilt with the matching cargo
feature.

This page is the exact recipe for those three handlers, for a build that drops default handlers, and for
checking afterwards what a binary actually contains.

---

## 1. The switches

| Handler | Cargo feature | In `default` | What selecting it adds | Run-time switch |
| --- | --- | --- | --- | --- |
| FITS | `fits` | no | parser, `BINTABLE` writer and verifier; `flate2` and `crc32fast` are already in the tree | `--fits-tile`. Without it a FITS file is `skipped` with `pass --fits-tile` |
| FLAC | `flac` | no | handler plus the `claxon`, `flacenc` and `md-5` crates | none: enabled as soon as it is compiled; `--formats flac` can narrow a run |
| PDF | `pdf` | no | handler; enables the existing `zlib` feature | none: enabled as soon as it is compiled; `--formats pdf` can narrow a run |
| JPEG verifier | `jpeg-v1` | yes | the independent `jpeg-decoder` pixel check | none. A build with `jpeg` but not `jpeg-v1` has the codec only, keeps JPEG opt-in via `--formats jpeg`, and does not run the independent pixel check |
| Zopfli | `zopfli` | yes | the strongest DEFLATE encoder | `--zopfli` |

Two rules follow from that table and explain most surprises:

- **A cargo feature is a compile-time switch; `--formats` is a run-time allowlist.** `--formats flac` cannot
  make a build that was compiled without `flac` handle FLAC; it answers `this build has no flac handler`.
  Conversely, compiling `flac` or `pdf` is enough for the handler to run without a flag.
- **FITS has two switches and both are off.** Compiling `fits` makes the handler exist; only `--fits-tile`
  makes it change an `IMAGE` HDU into a tile-compressed `BINTABLE`. Compiled-but-not-asked-for FITS files are
  left alone with that reason. See [guarantees §4.12](guarantees.md#412-fits--tile-compressed-image-hdus-v014-opt-in).

`fuzz-api` is also non-default, and it is not a handler: it only exposes parsers to `fuzz/`. It is never part
of a distributable binary; see §6.

---

## 2. Add a handler to the published feature set

`--features` is additive unless `--no-default-features` is also given, so each command below keeps every
default handler and adds the named one. From a checkout:

```sh
cargo build --release --locked --features fits
cargo build --release --locked --features flac
cargo build --release --locked --features pdf

# all three at once
cargo build --release --locked --features "fits,flac,pdf"
```

The binary is `target/release/tzim`. Rust 1.88 or newer is required, as for any other build. No C compiler,
system codec or runtime library is needed: `flate2` uses `miniz_oxide`, Zopfli is pure Rust, `jpeg-decoder` is
selected with `platform_independent`, and the FLAC pair is pure Rust as well. `--locked` keeps the build on the
reviewed `Cargo.lock`; once the crates are fetched, the same command works with `--offline`.

For a static Linux binary, add the musl target and the same flags:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --locked --target x86_64-unknown-linux-musl \
    --features "fits,flac,pdf"
```

---

## 3. Install through cargo

`cargo install tzim --locked` installs the default set, which is why the README uses it. Add the feature list
to the same command to install a binary that has the opt-in handlers:

```sh
cargo install tzim --locked --features "fits,flac,pdf"

# the checkout you are in, including an unreleased branch
cargo install --path . --locked --features "fits,flac,pdf"
```

Cargo refuses to replace an existing installation unless `--force` is given. The shell installer, Homebrew,
the Linux packages and the Windows installer have no feature switch and cannot be used to add FITS, FLAC or
PDF; those channels always install the default binary described in [releases §2](releases.md#2-what-a-release-contains).

---

## 4. A build with only the handler you need

`--no-default-features` removes every default handler and dependency; name back exactly what you want. The
`flac` and `pdf` lines are what CI's per-handler `opt-in-format-matrix` compiles; `png,gzip` is the smallest
useful optimizer, and the `minimal` job goes one step further with `png` alone
([testing §4](testing.md#4-continuous-integration)):

```sh
# one opt-in handler and nothing else
cargo build --release --locked --no-default-features --features flac
cargo build --release --locked --no-default-features --features pdf
cargo build --release --locked --no-default-features --features fits

# the smallest useful optimizer
cargo build --release --locked --no-default-features --features png,gzip
```

Feature dependencies are resolved by Cargo, not by the command line: `ico` requires `png`, `pdf` requires
`zlib`, and `jpeg-v1` requires `jpeg`. `fits` pulls only `flate2` and `crc32fast`, so it does not bring PNG
or the `png` crate with it. A feature-only build has no `--zopfli` unless `zopfli` is named as well.

---

## 5. Check what was built

```sh
./target/release/tzim --list-formats
./target/release/tzim --capabilities
```

`--list-formats` prints a row per compiled handler, so FITS, FLAC and PDF appear only when selected.
`--capabilities` is the machine-readable answer and keeps three things apart
([cli §4](cli.md#capabilities-document)):

- `build_features` has a boolean per cargo feature, including `fits`, `flac` and `pdf`;
- `compiled_formats` lists the handlers in the binary;
- `default_enabled_formats` lists the handlers used without a run-time opt-in. FLAC and PDF appear here when
  compiled; **FITS does not**, even when compiled, because `--fits-tile` is the separate consent.

A file of a format this build lacks is reported as `skipped` with `this build has no <format> handler`; the
file is not modified. `--formats <name>` for a handler that is not compiled is a usage error (exit `2`).

---

## 6. Test builds and what not to ship

```sh
# every feature, including the optional handlers and fuzz-api
cargo test --locked --all-features

# the isolated handlers, exactly as CI tests them
cargo test --locked --no-default-features --features flac --lib
cargo test --locked --no-default-features --features pdf --lib
```

`--all-features` is the right command for tests and the wrong one for a binary: it also enables `fuzz-api`,
which exists only to let `fuzz/` call the parsers and is never enabled for a release. A binary with every
handler and no fuzz entry points is `--features "fits,flac,pdf"` on top of the default set, not
`--all-features`.

The `jpeg` feature without `jpeg-v1` is also deliberate but narrower than a release: it removes the
independent pixel verifier and keeps JPEG behind `--formats jpeg`. The published build includes `jpeg-v1`
([guarantees §2.6](guarantees.md#26-verification-independence)).

---

## 7. What the opt-in handlers cost

FITS adds **36 KiB** to the default build, which is why it is not in the released feature set: the default
binary has a byte-exact size gate and this is enough to break it
([size budget §3](size-budget.md#3-feature-matrix)). FLAC and PDF are recorded in the exact-head feature
report as complete feature-only binaries of 836 384 bytes (`flac`) and 742 176 bytes (`pdf`), not as deltas
over the default set; the report and its source SHA are in [size budget §1](size-budget.md#1-budget).

No number here is a promise about a different toolchain or target. To measure your own build, use the
procedure in [size budget §4](size-budget.md#4-measurement-procedure).
