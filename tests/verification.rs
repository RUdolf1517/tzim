//! Negative tests for the verifiers (#43).
//!
//! The pipeline is only as safe as its second line of defence: a handler writes a
//! candidate, and `formats::*::verify` decides whether that candidate may replace
//! the original. Until this file existed, every verifier except `zlib`'s was
//! reachable only through a run that produced a *correct* candidate, so a
//! verifier that accepted anything passed the whole suite.
//!
//! Every test here builds a valid original, builds a candidate that differs in
//! exactly one field the contract names, and requires a refusal. The corruption is
//! chosen so that the *other* checks in the same function cannot see it: a test
//! that survives the deletion of the check it is supposed to guard is decoration,
//! not evidence.
//!
//! Two consequences of that rule are worth stating:
//!
//! - There is no APNG `fdAT` sequence-number test. Bumping the sequence number
//!   also makes the `png` crate's frame reader fail, so `compare_frames` refuses
//!   the candidate whether or not the sequence check exists: the check is defence
//!   in depth and cannot be pinned on its own.
//! - There is no JPEG coefficient test. Changing a symbol in the entropy stream
//!   changes the decoded planes *and* usually breaks the parse, so the test would
//!   pass for the wrong reason. What is pinned instead is the rule that the bytes
//!   after `EOI` survive, which nothing else compares.

// This file is compiled in every configuration CI builds, including
// `--no-default-features --features png` (#68), so a test that reaches into a
// handler which is not compiled in carries that handler's `#[cfg]`. The gates are
// exhaustive rather than defensive: with all features on — which is what `test
// (…, all-features)` and `(…, default)` run — every one of them is true, so no
// test is skipped there, and the minimal build compiles the file instead of the
// file quietly becoming all-features-only.
//
// The fixture writers are shared with `tests/integration.rs` and already carry
// their own `#![allow(dead_code)]`; a second attribute here is a clippy error.
mod common;

use common::*;
use tzim::formats::{self, Allow, Options};

fn options(strip_metadata: bool) -> Options {
    Options {
        zopfli: false,
        max_pixels: u64::MAX,
        max_uncompressed: u64::MAX,
        max_ratio: u64::MAX,
        strip_metadata,
        nested: false,
        max_depth: 3,
        depth: 0,
        fits_tile: true,
        allow: Allow {
            png: true,
            gzip: true,
            zip: true,
            ico: true,
            jpeg: true,
            tiff: true,
            zlib: true,
            fits: true,
        },
    }
}

/// Re-encodes the IDAT payload of a PNG after `mutate` has been applied to the
/// raw scanlines, keeping every other chunk byte-identical.
fn png_with_mutated_pixels(original: &[u8], mutate: impl Fn(&mut Vec<u8>)) -> Vec<u8> {
    use std::io::{Read, Write};

    let chunks = png_chunks(original).expect("the fixture must parse");
    let mut raw = Vec::new();
    let idat_original = chunks
        .iter()
        .find(|(typ, _)| typ.as_str() == "IDAT")
        .expect("a PNG has an IDAT")
        .1
        .clone();
    let mut decoder = flate2::read::ZlibDecoder::new(&idat_original[..]);
    decoder.read_to_end(&mut raw).unwrap();
    mutate(&mut raw);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    encoder.write_all(&raw).unwrap();
    let idat = encoder.finish().unwrap();

    let mut out = original[..8].to_vec();
    for (typ, payload) in &chunks {
        let mut t = [0u8; 4];
        t.copy_from_slice(typ.as_bytes());
        if typ.as_str() == "IDAT" {
            out.extend_from_slice(&chunk(&t, &idat));
        } else {
            out.extend_from_slice(&chunk(&t, payload));
        }
    }
    out
}

#[test]
fn verification_rejects_changed_pixels() {
    let spec = PngSpec {
        width: 32,
        height: 32,
        level: 6,
        ..Default::default()
    };
    let original = build_png(&spec);
    // One byte of one pixel, with the chunk CRCs recomputed: the file stays valid
    // PNG, and nothing but the decoded scanline comparison can notice.
    let candidate = png_with_mutated_pixels(&original, |raw| {
        raw[1] = raw[1].wrapping_add(1);
    });
    assert_ne!(candidate, original);
    assert!(
        png_chunks(&candidate).is_ok(),
        "the candidate must be a valid PNG"
    );

    let err = formats::png::verify(&original, &candidate, &options(false)).unwrap_err();
    assert!(err.to_string().contains("decoded pixels differ"), "{err}");
}

#[test]
fn verification_rejects_a_dropped_chunk() {
    let spec = PngSpec {
        level: 6,
        extra_chunks: vec![
            ("tEXt".to_string(), b"Comment\0kept as is".to_vec()),
            ("prVt".to_string(), vec![1, 2, 3, 4]),
        ],
        ..Default::default()
    };
    let original = build_png(&spec);

    // Rebuild the file without the `tEXt` chunk. The pixels, the IHDR and every
    // other chunk stay the same, so the chunk inventory is the only difference.
    let chunks = png_chunks(&original).unwrap();
    let mut candidate = original[..8].to_vec();
    for (typ, payload) in &chunks {
        if typ.as_str() == "tEXt" {
            continue;
        }
        let mut t = [0u8; 4];
        t.copy_from_slice(typ.as_bytes());
        candidate.extend_from_slice(&chunk(&t, payload));
    }

    let err = formats::png::verify(&original, &candidate, &options(false)).unwrap_err();
    assert!(err.to_string().contains("chunk count changed"), "{err}");
}

#[cfg(feature = "gzip")]
#[test]
fn verification_rejects_a_kept_filename() {
    // Under `--strip-metadata` exactly one gzip header is acceptable: the stripped
    // one. A candidate that keeps `FNAME` has to be refused even though its
    // payload, CRC and ISIZE are perfect.
    let payload = b"the payload is the content".repeat(100);
    let original = build_gzip(&payload, Some(b"secret-name.txt"), None, 0, 6);
    let candidate = build_gzip(&payload, Some(b"secret-name.txt"), None, 0, 9);

    let err = formats::gzip::verify(&original, &candidate, &options(true)).unwrap_err();
    assert!(err.to_string().contains("not the stripped header"), "{err}");
}

#[cfg(feature = "zip")]
#[test]
fn verification_rejects_a_changed_stored_entry() {
    let entry = ZipEntrySpec::stored("a.txt", b"stored bytes are never recompressed");
    let original = build_zip(&[entry], b"");
    // One byte of the payload, with the CRC in both headers left as it was: the
    // archive is structurally intact, and the decompressed content is the only
    // thing that changed.
    let mut candidate = original.clone();
    let parsed = parse_zip(&original);
    let offset =
        parsed[0].local_offset as usize + 30 + parsed[0].name.len() + parsed[0].local_extra.len();
    candidate[offset] ^= 0x01;

    let err = formats::zip::verify(&original, &candidate, &options(false)).unwrap_err();
    assert!(err.to_string().contains("stored data changed"), "{err}");
}

#[cfg(feature = "zip")]
#[test]
fn verification_rejects_a_surviving_comment() {
    // The archive comment is the one thing `--strip-metadata` removes from the
    // container itself; a candidate that puts it back must be refused.
    let entry = ZipEntrySpec::deflated("a.txt", &b"contents".repeat(200), 1);
    let original = build_zip(&[entry], b"archive comment");
    let candidate = build_zip(
        &[ZipEntrySpec::deflated("a.txt", &b"contents".repeat(200), 9)],
        b"archive comment",
    );

    let err = formats::zip::verify(&original, &candidate, &options(true)).unwrap_err();
    assert!(
        err.to_string().contains("archive comment survived"),
        "{err}"
    );
}

#[cfg(feature = "tiff")]
#[test]
fn verification_rejects_a_changed_tag_value() {
    // Same geometry, same strips, same compression — only an ASCII tag's value
    // differs. Nothing except the tag-value comparison can see it.
    let original = build_tiff(&TiffSpec {
        extra_ascii: vec![(315, "the author".to_string())],
        ..Default::default()
    });
    let candidate = build_tiff(&TiffSpec {
        extra_ascii: vec![(315, "another author".to_string())],
        ..Default::default()
    });

    let err = formats::tiff::verify(&original, &candidate, &options(false)).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("315") || message.contains("value"),
        "{message}"
    );
}

#[test]
fn verification_rejects_a_changed_bitmap_entry() {
    let bitmap = bmp_icon_payload(64);
    let spec = IcoEntrySpec {
        width: 16,
        height: 16,
        bit_count: 32,
        payload: bitmap.clone(),
    };
    let original = build_ico(1, &[spec]);

    let mut damaged = bitmap;
    // Byte 44 is past the 40-byte `BITMAPINFOHEADER`, in the pixel data.
    damaged[44] ^= 0xff;
    let candidate = build_ico(
        1,
        &[IcoEntrySpec {
            width: 16,
            height: 16,
            bit_count: 32,
            payload: damaged,
        }],
    );

    let err = formats::png::verify_icon(&original, &candidate, &options(false)).unwrap_err();
    assert!(err.to_string().contains("bitmap payload changed"), "{err}");
}

#[cfg(feature = "jpeg")]
#[test]
fn verification_rejects_changed_trailing_bytes() {
    // Bytes after `EOI` are part of the file, not padding: cameras append a second
    // image there. The marker inventory is identical, so this is the only check
    // that can catch it.
    let original = build_jpeg(16, 16, 3);
    let mut candidate = original.clone();
    candidate.extend_from_slice(b"appended payload");

    let err = formats::jpeg::verify(&original, &candidate, &options(false)).unwrap_err();
    assert!(err.to_string().contains("after EOI"), "{err}");
}

#[test]
fn verification_rejects_a_larger_candidate() {
    // G3 clause 4 lives in the dispatcher, before any format-specific work: a
    // candidate that is larger than its original is refused whatever it contains.
    let opts = options(false);
    let err = formats::verify(
        tzim::detect::Format::Png,
        b"not a png",
        b"definitely not a png",
        &opts,
    )
    .unwrap_err();
    assert!(err.is_verify(), "{err}");
    assert!(err.to_string().contains("candidate is"), "{err}");
}

#[test]
fn verification_accepts_an_equal_size_candidate_only_when_stripping() {
    // The same two blobs, the same length. Without the flag the size gate fires;
    // with it the gate passes and the *parser* is what refuses them, which proves
    // the gate is the size rule and not an accident of the error path.
    let equal_a = b"0123456789";
    let equal_b = b"9876543210";

    let plain =
        formats::verify(tzim::detect::Format::Png, equal_a, equal_b, &options(false)).unwrap_err();
    assert!(
        plain.to_string().contains("candidate is"),
        "an equal-size candidate must be refused without the flag: {plain}"
    );

    let stripped =
        formats::verify(tzim::detect::Format::Png, equal_a, equal_b, &options(true)).unwrap_err();
    assert!(
        !stripped.to_string().contains("candidate is"),
        "the size gate must not fire under --strip-metadata: {stripped}"
    );
}
