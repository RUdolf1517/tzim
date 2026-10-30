# Third-party notices

The default build has five direct crate dependencies; the non-default #79 FLAC feature adds three more. This
file lists each dependency and its license so a distribution packager does not have to unpack the build. The
policy and reasoning for each entry are in [docs/dependencies.md](docs/dependencies.md).

## Direct dependencies

| Crate | Used for | Licence | Ships a `NOTICE`? |
| --- | --- | --- | --- |
| [`flate2`](https://crates.io/crates/flate2) | DEFLATE for PNG, gzip, ZIP, TIFF, zlib, FITS, and PDF FlateDecode streams | MIT OR Apache-2.0 | no |
| [`crc32fast`](https://crates.io/crates/crc32fast) | CRC-32 checksums | MIT OR Apache-2.0 | no |
| [`png`](https://crates.io/crates/png) | independent PNG verification decoder | MIT OR Apache-2.0 | no |
| [`zopfli`](https://crates.io/crates/zopfli) | optional strongest DEFLATE encoder | Apache-2.0 | no — it ships `COPYING` and `CONTRIBUTORS` |
| [`jpeg-decoder`](https://crates.io/crates/jpeg-decoder) | independent JPEG pixel verification (`jpeg-v1`) | MIT | no |
| [`flacenc`](https://crates.io/crates/flacenc) | FLAC frame encoder (`flac`, non-default) | Apache-2.0 | no |
| [`claxon`](https://crates.io/crates/claxon) | independent FLAC decoder/PCM verifier (`flac`, non-default) | Apache-2.0 | no |
| [`md-5`](https://crates.io/crates/md-5) | FLAC STREAMINFO MD5 verification (`flac`, non-default) | MIT OR Apache-2.0 | no |

`indicatif`, which drew the progress line from v0.10 to v0.12, is no longer linked: v0.13 draws the line in
`src/progress.rs`, and the crates it brought in — `console`, `portable-atomic`, `unit-prefix`,
`encode_unicode`, `libc`, `windows-sys`, `windows-link` — left the tree with it.

Where a crate offers a choice (MIT OR Apache-2.0), this project relies on **MIT**. Apache-2.0 §4(d) requires
the reproduction of a `NOTICE` file when the work ships one; the inspected `flacenc` and `claxon` sources
contain no `NOTICE` file. Check the locked crate sources for `NOTICE` files when preparing a distribution.
`zopfli`'s `COPYING` file is the Apache-2.0 licence text itself.

## Transitive dependencies

The codec tree includes `miniz_oxide`, `adler2`, `fdeflate`, `bitflags`, `cfg-if`, `simd-adler32`,
`zlib-rs`, `bumpalo`, and `log`. The optional FLAC tree adds `autocfg`, `block-buffer`, `built`, `byteorder`,
`crc`, `crc-catalog`, `crypto-common`, `digest`, `generic-array`, `hash32`, `heapless`, `num-traits`,
`rustversion`, `seq-macro`, `stable_deref_trait`, `typenum`, and `version_check`. These are permissively
licensed (MIT, Apache-2.0, Zlib, or 0BSD); exact versions and checksums are recorded in `Cargo.lock`, and the
allow-list is enforced by `cargo deny` in CI ([docs/dependencies.md §5](docs/dependencies.md#5-licence-policy)).

## This project

Tzim itself is [MIT](LICENSE).
