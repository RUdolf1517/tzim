//! zlib (RFC 1950).
//!
//! A zlib stream is a two-byte header, a DEFLATE stream and a four-byte
//! big-endian Adler-32 of the uncompressed data. Only the DEFLATE stream is
//! rewritten; the header is copied verbatim and the trailer is copied after
//! being proven to describe the same payload.
//!
//! The container is small enough to state completely, which is why this handler
//! can be strict: everything it does not recognise is refused rather than
//! guessed at.

use std::io::{self, Read, Write};

use crate::error::{Error, Result};

use super::{Cand, Options};

/// The trailer: Adler-32 of the uncompressed payload, big-endian.
const TRAILER: usize = 4;

/// The smallest possible stream: two header bytes, an empty fixed-Huffman block
/// and the checksum.
const MIN_STREAM: usize = 2 + 2 + TRAILER;

/// `CINFO` is `CMF >> 4`; it names a window of `2^(CINFO + 8)` bytes, and `15`
/// (32 KiB) is what every normal encoder uses. A smaller window cannot be
/// trusted after recompression: our encoder is free to emit distances up to
/// 32 KiB, and a decoder that honours the header would reject them.
const FULL_WINDOW_CINFO: u8 = 7;

#[derive(Debug)]
struct Inspected {
    /// The two header bytes, kept so that "copied verbatim" can be checked.
    header: [u8; 2],
    payload_len: u64,
    adler: u32,
}

/// Parses the header, validates the trailer and confirms the body is exactly one
/// complete DEFLATE stream. Anything else is refused.
fn inspect(data: &[u8]) -> Result<Inspected> {
    if data.len() < MIN_STREAM {
        return Err(Error::parse("zlib: shorter than a header and a checksum"));
    }
    let header = [data[0], data[1]];
    let (cmf, flg) = (data[0], data[1]);

    if cmf & 0x0f != 8 {
        return Err(Error::refused(format!(
            "zlib: compression method {} is not DEFLATE",
            cmf & 0x0f
        )));
    }
    let cinfo = cmf >> 4;
    if cinfo > FULL_WINDOW_CINFO {
        return Err(Error::parse(
            "zlib: window size code above 7 is invalid in a zlib header",
        ));
    }
    // RFC 1950 §2.2: the two header bytes, read as a big-endian integer, must be
    // a multiple of 31. This is the container's only self-check.
    let check = (u16::from(cmf) << 8) | u16::from(flg);
    if !check.is_multiple_of(31) {
        return Err(Error::parse(
            "zlib: header check bits are wrong; the file is corrupt",
        ));
    }
    if flg & 0x20 != 0 {
        return Err(Error::refused(
            "zlib: preset dictionary (FDICT) is set; the stream needs a dictionary Tzim does not have",
        ));
    }
    if cinfo < FULL_WINDOW_CINFO {
        return Err(Error::refused(format!(
            "zlib: window of 2^{} bytes; recompressing with a 32 KiB window could exceed it",
            cinfo as u16 + 8
        )));
    }

    let body = &data[2..data.len() - TRAILER];
    let (payload_len, adler) = inflate(body)?;

    let stored = u32::from_be_bytes([
        data[data.len() - 4],
        data[data.len() - 3],
        data[data.len() - 2],
        data[data.len() - 1],
    ]);
    if stored != adler {
        return Err(Error::parse(
            "zlib: Adler-32 does not match the compressed data; the file is corrupt",
        ));
    }

    Ok(Inspected {
        header,
        payload_len,
        adler,
    })
}

/// Decompresses `body` completely and returns its length and Adler-32.
///
/// `total_in` is compared with the body length so that a second stream or any
/// trailing bytes are refused rather than silently dropped.
///
/// The first failure is classified by how far the stream got, and that is the
/// whole point of this function (#52). RFC 1950's header check is what
/// detection has to go on — two bytes divisible by 31 — and it is satisfied by
/// 1 random two-byte sequence in 1984, so a tree of 2000 unrelated files has a
/// ~63 % chance of containing one. If such a file decodes **nothing** before
/// failing, the header was a coincidence: that is `unsupported`, not a failure,
/// and the run keeps its exit code. If it decodes **at least one byte** and
/// then breaks, the data really was a DEFLATE stream, and that stays `failed`
/// with exit 1 — a truncated `.zlib` must not hide behind exit 0.
fn inflate(body: &[u8]) -> Result<(u64, u32)> {
    let mut decoder = flate2::read::DeflateDecoder::new(body);
    let mut adler = Adler32::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        match decoder.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                adler.update(&buffer[..n]);
                total += n as u64;
            }
            Err(_) if total == 0 => {
                return Err(Error::unsupported(
                    "zlib: nothing decodes behind the header; either the header was a \
                     coincidence or the stream is too short to tell (the file is untouched)",
                ));
            }
            Err(e) => return Err(Error::Io(e)),
        }
    }
    if decoder.total_in() != body.len() as u64 {
        return Err(Error::refused(
            "zlib: trailing data after the stream (concatenated zlib files are not supported)",
        ));
    }
    Ok((total, adler.finish()))
}

/// Adler-32 (RFC 1950 §9). Twenty lines of arithmetic, and a checksum is not an
/// algorithm worth a crate: `crc32fast` computes a different checksum entirely.
struct Adler32 {
    a: u32,
    b: u32,
}

impl Adler32 {
    fn new() -> Adler32 {
        Adler32 { a: 1, b: 0 }
    }

    fn update(&mut self, bytes: &[u8]) {
        // 5552 is the largest run for which the sums cannot overflow a `u32`
        // before the modulus is applied (zlib uses the same bound).
        const NMAX: usize = 5552;
        let mut a = self.a;
        let mut b = self.b;
        for chunk in bytes.chunks(NMAX) {
            for &byte in chunk {
                a += u32::from(byte);
                b += a;
            }
            a %= 65521;
            b %= 65521;
        }
        self.a = a;
        self.b = b;
    }

    fn finish(&self) -> u32 {
        (self.b << 16) | self.a
    }
}

fn recompress(body: &[u8], zopfli: bool) -> Result<Vec<u8>> {
    if zopfli {
        #[cfg(feature = "zopfli")]
        {
            let mut out = Vec::new();
            zopfli::compress(
                zopfli::Options::default(),
                zopfli::Format::Deflate,
                flate2::read::DeflateDecoder::new(body),
                &mut out,
            )
            .map_err(|e| Error::Internal(format!("zopfli failed: {e}")))?;
            return Ok(out);
        }
        #[cfg(not(feature = "zopfli"))]
        {
            let _ = zopfli;
        }
    }

    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(9));
    io::copy(&mut flate2::read::DeflateDecoder::new(body), &mut encoder)?;
    Ok(encoder.finish()?)
}

pub fn optimize(data: &[u8], dst: &mut dyn Write, opts: &Options) -> Result<Cand> {
    let info = inspect(data)?;

    let body = &data[2..data.len() - TRAILER];
    let mut best = recompress(body, false)?;
    if opts.zopfli {
        let strong = recompress(body, true)?;
        if strong.len() < best.len() {
            best = strong;
        }
    }

    let total = 2u64 + best.len() as u64 + TRAILER as u64;
    // `--strip-metadata` has nothing to remove here: a zlib header carries no
    // name, no comment and no timestamp, and `FLEVEL` is a property of the
    // encoder rather than metadata about the file. The flag changes nothing.
    if total >= data.len() as u64 {
        return Ok(Cand::NoGain);
    }

    dst.write_all(&info.header)?;
    dst.write_all(&best)?;
    // The trailer is copied verbatim: it was proven to match the payload above,
    // and the payload is the same payload.
    dst.write_all(&data[data.len() - TRAILER..])?;
    Ok(Cand::Written(total))
}

pub fn verify(a: &[u8], b: &[u8], opts: &Options) -> Result<()> {
    let _ = opts;
    let info_a = inspect(a).map_err(|e| Error::verify(format!("original: {e}")))?;
    let info_b = inspect(b).map_err(|e| Error::verify(format!("candidate: {e}")))?;

    if info_a.header != info_b.header {
        return Err(Error::verify("zlib header was not preserved"));
    }
    if a[a.len() - TRAILER..] != b[b.len() - TRAILER..] {
        return Err(Error::verify("zlib Adler-32 trailer was not preserved"));
    }
    if info_a.payload_len != info_b.payload_len || info_a.adler != info_b.adler {
        return Err(Error::verify("zlib payload length or Adler-32 changed"));
    }

    let body_a = &a[2..a.len() - TRAILER];
    let body_b = &b[2..b.len() - TRAILER];
    let mut da = flate2::read::DeflateDecoder::new(body_a);
    let mut db = flate2::read::DeflateDecoder::new(body_b);
    let mut ba = [0u8; 64 * 1024];
    let mut bb = [0u8; 64 * 1024];
    loop {
        let na = read_fully(&mut da, &mut ba)?;
        let nb = read_fully(&mut db, &mut bb)?;
        if na != nb || ba[..na] != bb[..nb] {
            return Err(Error::verify("decompressed payload differs"));
        }
        if na == 0 {
            break;
        }
    }
    Ok(())
}

fn read_fully(reader: &mut impl Read, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zlib stream at the given level, built with the same encoder the handler
    /// uses — the *fixture* is not what is under test, the round trip is.
    fn build(payload: &[u8], level: u32) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(level));
        encoder.write_all(payload).unwrap();
        encoder.finish().unwrap()
    }

    fn compressible(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 7) as u8).collect()
    }

    fn opts() -> Options {
        Options {
            zopfli: false,
            max_pixels: u64::MAX,
            max_uncompressed: u64::MAX,
            max_ratio: u64::MAX,
            strip_metadata: false,
            nested: false,
            max_depth: 0,
            depth: 0,
            allow: super::super::Allow::default(),
        }
    }

    #[test]
    fn a_weakly_compressed_stream_gets_smaller_and_stays_identical() {
        let payload = compressible(200_000);
        let original = build(&payload, 1);
        let mut out = Vec::new();
        let cand = optimize(&original, &mut out, &opts()).unwrap();
        match cand {
            Cand::Written(len) => assert_eq!(len as usize, out.len()),
            Cand::NoGain => panic!("expected a gain on a level-1 stream"),
        }
        assert!(out.len() < original.len());
        verify(&original, &out, &opts()).unwrap();

        let mut decoded = Vec::new();
        flate2::read::ZlibDecoder::new(&out[..])
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn the_header_and_trailer_are_copied_verbatim() {
        let payload = compressible(64_000);
        let original = build(&payload, 1);
        let mut out = Vec::new();
        optimize(&original, &mut out, &opts()).unwrap();
        assert_eq!(out[..2], original[..2]);
        assert_eq!(out[out.len() - 4..], original[original.len() - 4..]);
    }

    #[test]
    fn an_already_optimal_stream_is_left_alone() {
        let payload = compressible(50_000);
        let original = build(&payload, 9);
        let mut out = Vec::new();
        assert!(matches!(
            optimize(&original, &mut out, &opts()).unwrap(),
            Cand::NoGain
        ));
        assert!(
            out.is_empty(),
            "nothing may be written when there is no gain"
        );
    }

    #[test]
    fn a_preset_dictionary_is_refused() {
        // CMF = 0x78, FDICT = 0x20, and a FLG that satisfies the check value.
        let flg = {
            let mut f = 0x20u16;
            while !((0x78u16 << 8) | f).is_multiple_of(31) {
                f += 1;
            }
            f as u8
        };
        let mut data = vec![0x78, flg];
        data.extend_from_slice(&[0, 0, 0, 0]); // dictionary id
        data.extend_from_slice(&[0x03, 0x00, 0, 0, 0, 1]);
        let err = inspect(&data).unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "{err}");
    }

    #[test]
    fn a_small_window_is_refused() {
        // CINFO = 4 → a 4 KiB window. Legal, and not something we can promise to
        // stay inside after recompressing.
        let mut flg = 0u16;
        while !((0x48u16 << 8) | flg).is_multiple_of(31) {
            flg += 1;
        }
        let mut data = vec![0x48, flg as u8];
        data.extend_from_slice(&[0x03, 0x00, 0, 0, 0, 1]);
        let err = inspect(&data).unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "{err}");
    }

    #[test]
    fn a_broken_header_check_is_a_parse_error() {
        let mut data = build(&compressible(1000), 6);
        data[1] ^= 0x01; // breaks the multiple-of-31 check
        let err = inspect(&data).unwrap_err();
        assert!(matches!(err, Error::Parse(_)), "{err}");
    }

    #[test]
    fn a_wrong_adler_is_a_parse_error() {
        let mut data = build(&compressible(1000), 6);
        let last = data.len() - 1;
        data[last] ^= 0xff;
        let err = inspect(&data).unwrap_err();
        assert!(matches!(err, Error::Parse(_)), "{err}");
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let mut data = build(&compressible(5000), 6);
        data.insert(data.len() - 4, 0);
        let err = inspect(&data).unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "{err}");
    }

    /// The half of #52 that must not move: a stream that really was DEFLATE and
    /// then ends early stays a failure, even though it is truncated.
    ///
    /// The cost of the rule is stated in `guarantees.md` §4.10 and not hidden
    /// here: a stream cut so short that *nothing* decodes is indistinguishable
    /// from the coincidental header above, and is reported `unsupported`. The
    /// distinction is how far the decoder got, so it cannot be made finer.
    #[test]
    fn a_truncated_stream_that_decoded_bytes_is_still_a_failure() {
        let data = build(&compressible(50_000), 6);
        let cut = data.len() * 3 / 4;
        let err = inspect(&data[..cut]).unwrap_err();
        assert!(
            !matches!(err, Error::Unsupported(_)),
            "a stream that produced bytes and then broke is corruption, not a coincidence: {err}"
        );
        assert!(matches!(err, Error::Parse(_) | Error::Io(_)), "{err}");
    }

    /// The other half of #52: two bytes that satisfy RFC 1950's check are not a
    /// stream. Random bytes hit that check once in 1984, so without this a tree
    /// of a few thousand files fails the run for no reason.
    #[test]
    fn a_coincidental_header_that_decodes_nothing_is_unsupported() {
        let mut data = vec![0x78, 0x9c];
        // BFINAL = 1, BTYPE = 3: a reserved block type, so the decoder fails
        // without producing a byte.
        data.extend_from_slice(&[0xff; 16]);
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);

        // The fixture is worthless unless detection really calls it zlib: a
        // file the detector ignores is reported `unsupported` for the other
        // reason, and the test would pass without the handler running at all.
        assert_eq!(
            crate::detect::detect(&data),
            Some(crate::detect::Format::Zlib),
            "the fixture must pass detection's header check"
        );
        let err = inspect(&data).unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)), "{err}");

        // The message was asserting more than the code knows (#62): for a real
        // stream cut to a few dozen bytes, "this is not a zlib stream" was
        // false. What is known is only that nothing decodes, and the reason is
        // one of two. The prose is part of the interface here, so it is
        // asserted: a message that states a conclusion the decoder cannot reach
        // is a bug in the contract, not a wording preference.
        let message = err.to_string();
        assert!(
            message.contains("either the header was a coincidence or the stream is too short"),
            "the note must name both possibilities, not one: {message}"
        );
        assert!(
            !message.contains("this is not a zlib stream"),
            "the decoder cannot know that: {message}"
        );
    }

    #[test]
    fn verification_rejects_a_changed_header() {
        let payload = compressible(100_000);
        let original = build(&payload, 1);
        let mut out = Vec::new();
        optimize(&original, &mut out, &opts()).unwrap();
        // The header is copied verbatim, so a changed byte is a contract
        // violation even before the payload is compared — and this one also
        // breaks the check value, so it cannot even be parsed.
        let mut tampered = out.clone();
        tampered[0] = 0x68; // CINFO 6, and no longer a valid check value
        assert!(verify(&original, &tampered, &opts()).is_err());
    }

    #[test]
    fn verification_rejects_a_changed_payload() {
        let payload = compressible(100_000);
        let original = build(&payload, 1);
        let mut out = Vec::new();
        optimize(&original, &mut out, &opts()).unwrap();

        let mut other_payload = payload.clone();
        other_payload[100] ^= 0xff;
        let other = build(&other_payload, 9);
        assert!(verify(&original, &other, &opts()).is_err());
    }

    #[test]
    fn adler32_matches_the_reference_values() {
        // RFC 1950 test vectors, and the classic "Wikipedia" negation.
        assert_eq!(
            {
                let mut a = Adler32::new();
                a.update(b"Wikipedia");
                a.finish()
            },
            0x11E6_0398
        );
        assert_eq!(Adler32::new().finish(), 1);
        assert_eq!(
            {
                let mut a = Adler32::new();
                a.update(&[0u8; 10_000]);
                a.finish()
            },
            // Ten thousand **zero bytes**, so `a += byte` never changes `a`:
            // a = 1 and b = 1 added 10 000 times = 10 000 = 0x2710. Hence
            // (b << 16) | a = 0x2710_0001, which is what `zlib.adler32` returns
            // for the same input. (The arithmetic that looks natural here — the
            // sum of 0..9 999 — belongs to a comment measuring the length, not
            // the bytes. Getting this wrong in the code would refuse every
            // zlib file as corrupt.)
            0x2710_0001
        );
    }

    #[test]
    fn adler32_survives_a_large_input_without_overflow() {
        let bytes = vec![0xffu8; 1 << 20];
        let mut a = Adler32::new();
        a.update(&bytes);
        assert_eq!(a.finish(), 0x8E88_EF11);
    }
}
