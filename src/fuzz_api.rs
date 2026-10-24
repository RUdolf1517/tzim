//! Entry points for the fuzz targets, and only for them.
//!
//! Compiled with `--features fuzz-api`; the release binary never contains this
//! module. The functions take arbitrary bytes and must never panic: a panic is
//! the defect the fuzzer is looking for, so nothing here is allowed to assume
//! that the input is well formed.

use crate::detect::detect;
use crate::formats::{self, Allow, Options};

fn options() -> Options {
    Options {
        zopfli: false,
        strip_metadata: false,
        max_pixels: u64::MAX,
        max_uncompressed: u64::MAX,
        max_ratio: u64::MAX,
        nested: true,
        max_depth: 3,
        depth: 0,
        allow: Allow {
            png: true,
            gzip: true,
            zip: true,
            ico: true,
            jpeg: true,
            tiff: true,
        },
    }
}

/// Detection, the full handler, and verification of whatever candidate appears.
/// Returns true when a candidate was produced and passed its own contract.
pub fn roundtrip(data: &[u8]) -> bool {
    let Some(format) = detect(data) else {
        return false;
    };
    let mut candidate = Vec::new();
    if formats::optimize(format, data, &mut candidate, &options()).is_err() {
        return false;
    }
    if candidate.is_empty() || candidate.len() >= data.len() {
        return false;
    }
    formats::verify(format, data, &candidate, &options()).is_ok()
}

/// The JPEG marker parser, analysis and entropy decoder, without writing.
#[cfg(feature = "jpeg")]
pub fn jpeg_decode(data: &[u8]) -> bool {
    formats::jpeg::probe(data)
}

/// The same round trip with `--strip-metadata`, so the removal paths get the
/// same treatment as the preservation paths.
pub fn roundtrip_strip(data: &[u8]) -> bool {
    let Some(format) = detect(data) else {
        return false;
    };
    let mut opts = options();
    opts.strip_metadata = true;
    let mut candidate = Vec::new();
    if formats::optimize(format, data, &mut candidate, &opts).is_err() {
        return false;
    }
    if candidate.is_empty() || candidate.len() > data.len() {
        return false;
    }
    formats::verify(format, data, &candidate, &opts).is_ok()
}
