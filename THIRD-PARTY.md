# Third-party notices

Tzim links five direct crates and a handful of their dependencies. This file lists them so that a
distribution packager does not have to unpack the build. The list is short on purpose: the policy behind it,
and the reasoning for every entry, is in [docs/dependencies.md](docs/dependencies.md).

## Direct dependencies

| Crate | Used for | Licence | Ships a `NOTICE`? |
| --- | --- | --- | --- |
| [`flate2`](https://crates.io/crates/flate2) | DEFLATE for PNG, gzip, ZIP, TIFF and zlib | MIT OR Apache-2.0 | no |
| [`crc32fast`](https://crates.io/crates/crc32fast) | CRC-32 checksums | MIT OR Apache-2.0 | no |
| [`png`](https://crates.io/crates/png) | the independent verification decoder | MIT OR Apache-2.0 | no |
| [`zopfli`](https://crates.io/crates/zopfli) | the optional strongest DEFLATE encoder | Apache-2.0 | no — it ships `COPYING` and `CONTRIBUTORS` |
| [`jpeg-decoder`](https://crates.io/crates/jpeg-decoder) | the independent JPEG decoder used to verify a candidate (`jpeg-v1`) | MIT | no |

`indicatif`, which drew the progress line from v0.10 to v0.12, is no longer linked: v0.13 draws the line in
`src/progress.rs`, and the crates it brought in — `console`, `portable-atomic`, `unit-prefix`,
`encode_unicode`, `libc`, `windows-sys`, `windows-link` — left the tree with it.

Where a crate offers a choice (MIT OR Apache-2.0), this project relies on **MIT**. Apache-2.0 §4(d) requires
the reproduction of a `NOTICE` file when the work ships one; no crate in this tree ships one at the versions
in `Cargo.lock`, and the `COPYING` file of `zopfli` is the Apache-2.0 licence text itself, reproduced in that
crate's own archive.

## Transitive dependencies

`miniz_oxide`, `adler2`, `fdeflate`, `bitflags`, `cfg-if` (through the codecs above). All are permissively
licensed (MIT, Apache-2.0, Zlib, 0BSD); the exact
versions and their checksums are in `Cargo.lock`, and the allow-list is enforced by `cargo deny` in CI
([docs/dependencies.md §5](docs/dependencies.md#5-licence-policy)).

## This project

Tzim itself is [MIT](LICENSE).
