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

#[derive(Clone, Copy)]
struct ZlibStream<'a> {
    header: [u8; 2],
    body: &'a [u8],
    stored_adler: u32,
}

/// Parses the header and trailer fields without reading the DEFLATE body.
fn parse_stream(data: &[u8]) -> Result<ZlibStream<'_>> {
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

    let stored_adler = u32::from_be_bytes([
        data[data.len() - 4],
        data[data.len() - 3],
        data[data.len() - 2],
        data[data.len() - 1],
    ]);
    Ok(ZlibStream {
        header,
        body: &data[2..data.len() - TRAILER],
        stored_adler,
    })
}

/// Parses the complete stream, validates its trailer and checks that DEFLATE
/// consumed exactly the body. Anything else is refused.
fn inspect(data: &[u8]) -> Result<Inspected> {
    inspect_stream(&parse_stream(data)?)
}

fn inspect_stream(stream: &ZlibStream<'_>) -> Result<Inspected> {
    let (payload_len, adler) = inflate(stream.body)?;
    if stream.stored_adler != adler {
        return Err(Error::parse(
            "zlib: Adler-32 does not match the compressed data; the file is corrupt",
        ));
    }
    Ok(Inspected {
        header: stream.header,
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
            Err(e) => return Err(classify_deflate_error(total, e)),
        }
    }
    if decoder.total_in() != body.len() as u64 {
        return Err(Error::refused(
            "zlib: trailing data after the stream (concatenated zlib files are not supported)",
        ));
    }
    Ok((total, adler.finish()))
}

fn classify_deflate_error(total: u64, error: io::Error) -> Error {
    if total == 0 {
        Error::unsupported(
            "zlib: nothing decodes behind the header; either the header was a \
             coincidence or the stream is too short to tell (the file is untouched)",
        )
    } else {
        Error::Io(error)
    }
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

struct StreamScan<'a> {
    stream: ZlibStream<'a>,
    decoder: flate2::read::DeflateDecoder<&'a [u8]>,
    adler: Adler32,
    payload_len: u64,
    result: Option<Result<Inspected>>,
}

impl<'a> StreamScan<'a> {
    fn new(stream: ZlibStream<'a>) -> Self {
        StreamScan {
            decoder: flate2::read::DeflateDecoder::new(stream.body),
            stream,
            adler: Adler32::new(),
            payload_len: 0,
            result: None,
        }
    }

    /// Fills one comparison block, accounting for short reads just like the
    /// verifier's previous bytewise comparison loop. `None` means the decoder
    /// failed; the precise error is kept in `result`.
    fn read_fully(&mut self, buffer: &mut [u8]) -> Option<usize> {
        if self.result.is_some() {
            return None;
        }
        let mut filled = 0;
        while filled < buffer.len() {
            match self.decoder.read(&mut buffer[filled..]) {
                Ok(0) => {
                    self.result = Some(self.finish());
                    return Some(filled);
                }
                Ok(n) => {
                    self.adler.update(&buffer[filled..filled + n]);
                    self.payload_len += n as u64;
                    filled += n;
                }
                Err(error) => {
                    self.result = Some(Err(classify_deflate_error(self.payload_len, error)));
                    return None;
                }
            }
        }
        Some(filled)
    }

    fn finish(&self) -> Result<Inspected> {
        if self.decoder.total_in() != self.stream.body.len() as u64 {
            return Err(Error::refused(
                "zlib: trailing data after the stream (concatenated zlib files are not supported)",
            ));
        }
        let adler = self.adler.finish();
        if self.stream.stored_adler != adler {
            return Err(Error::parse(
                "zlib: Adler-32 does not match the compressed data; the file is corrupt",
            ));
        }
        Ok(Inspected {
            header: self.stream.header,
            payload_len: self.payload_len,
            adler,
        })
    }
}

fn inflate_pair(
    stream_a: ZlibStream<'_>,
    stream_b: ZlibStream<'_>,
) -> Result<(Inspected, Inspected, bool)> {
    let mut a = StreamScan::new(stream_a);
    let mut b = StreamScan::new(stream_b);
    let mut equal = true;
    let mut buffer_a = [0u8; 64 * 1024];
    let mut buffer_b = [0u8; 64 * 1024];

    while a.result.is_none() || b.result.is_none() {
        let na = if a.result.is_none() {
            let count = a.read_fully(&mut buffer_a);
            if let Some(Err(error)) = &a.result {
                return Err(Error::verify(format!("original: {error}")));
            }
            count.unwrap_or(0)
        } else {
            0
        };
        let nb = if b.result.is_none() {
            b.read_fully(&mut buffer_b).unwrap_or(0)
        } else {
            0
        };
        if na != nb || buffer_a[..na] != buffer_b[..nb] {
            equal = false;
        }
    }

    let info_a = a
        .result
        .expect("the original stream scan must finish")
        .map_err(|error| Error::verify(format!("original: {error}")))?;
    let info_b = b
        .result
        .expect("the candidate stream scan must finish")
        .map_err(|error| Error::verify(format!("candidate: {error}")))?;
    Ok((info_a, info_b, equal))
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
    let stream_a = parse_stream(a).map_err(|error| Error::verify(format!("original: {error}")))?;
    let stream_b = match parse_stream(b) {
        Ok(stream) => stream,
        Err(candidate_error) => {
            // Preserve the old diagnostic priority: the original is fully
            // validated before a candidate-side parse error is reported.
            inspect_stream(&stream_a)
                .map_err(|error| Error::verify(format!("original: {error}")))?;
            return Err(Error::verify(format!("candidate: {candidate_error}")));
        }
    };
    let (info_a, info_b, payloads_equal) = inflate_pair(stream_a, stream_b)?;

    if info_a.header != info_b.header {
        return Err(Error::verify("zlib header was not preserved"));
    }
    if stream_a.stored_adler != stream_b.stored_adler {
        return Err(Error::verify("zlib Adler-32 trailer was not preserved"));
    }
    if info_a.payload_len != info_b.payload_len || info_a.adler != info_b.adler {
        return Err(Error::verify("zlib payload length or Adler-32 changed"));
    }
    if !payloads_equal {
        return Err(Error::verify("decompressed payload differs"));
    }
    Ok(())
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
            fits_tile: false,
        }
    }

    /// The pre-streaming verifier kept as a test oracle: it validates each
    /// stream in `inspect`, then decodes both bodies again for byte comparison.
    fn legacy_verify(a: &[u8], b: &[u8]) -> Result<()> {
        let info_a = inspect(a).map_err(|error| Error::verify(format!("original: {error}")))?;
        let info_b = inspect(b).map_err(|error| Error::verify(format!("candidate: {error}")))?;
        if info_a.header != info_b.header {
            return Err(Error::verify("zlib header was not preserved"));
        }
        if a[a.len() - TRAILER..] != b[b.len() - TRAILER..] {
            return Err(Error::verify("zlib Adler-32 trailer was not preserved"));
        }
        if info_a.payload_len != info_b.payload_len || info_a.adler != info_b.adler {
            return Err(Error::verify("zlib payload length or Adler-32 changed"));
        }

        let mut decoder_a = flate2::read::DeflateDecoder::new(&a[2..a.len() - TRAILER]);
        let mut decoder_b = flate2::read::DeflateDecoder::new(&b[2..b.len() - TRAILER]);
        let mut buffer_a = [0u8; 64 * 1024];
        let mut buffer_b = [0u8; 64 * 1024];
        loop {
            let na = legacy_read_fully(&mut decoder_a, &mut buffer_a)?;
            let nb = legacy_read_fully(&mut decoder_b, &mut buffer_b)?;
            if na != nb || buffer_a[..na] != buffer_b[..nb] {
                return Err(Error::verify("decompressed payload differs"));
            }
            if na == 0 {
                break;
            }
        }
        Ok(())
    }

    fn legacy_read_fully(reader: &mut impl Read, buffer: &mut [u8]) -> Result<usize> {
        let mut filled = 0;
        while filled < buffer.len() {
            match reader.read(&mut buffer[filled..])? {
                0 => break,
                n => filled += n,
            }
        }
        Ok(filled)
    }

    fn assert_verifier_matches_legacy(a: &[u8], b: &[u8]) {
        let expected = legacy_verify(a, b).map_err(|error| error.to_string());
        let actual = verify(a, b, &opts()).map_err(|error| error.to_string());
        assert_eq!(actual, expected, "streaming verifier changed the legacy result");
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
        assert_verifier_matches_legacy(&original, &out);

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
        assert_verifier_matches_legacy(&original, &tampered);
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
        assert_verifier_matches_legacy(&original, &other);
    }

    #[test]
    fn streaming_verifier_preserves_diagnostic_priority_and_bytewise_checks() {
        let payload = compressible(4096);
        let original = build(&payload, 1);

        // A different but valid FLEVEL is still a preserved-header violation.
        let mut changed_header = original.clone();
        let cmf = u16::from(changed_header[0]);
        let base = 0x40u16; // FLEVEL=1, FDICT=0
        let check = (0..32u16)
            .find(|check| ((cmf << 8) | (base | check)).is_multiple_of(31))
            .unwrap();
        changed_header[1] = (base | check) as u8;
        assert!(verify(&original, &changed_header, &opts())
            .unwrap_err()
            .to_string()
            .contains("zlib header was not preserved"));
        assert_verifier_matches_legacy(&original, &changed_header);

        // A valid, changed payload reaches the historical trailer diagnostic
        // before the length/Adler and bytewise comparisons.
        let mut changed_payload = payload.clone();
        changed_payload[100] ^= 0x80;
        let changed_payload = build(&changed_payload, 1);
        assert!(verify(&original, &changed_payload, &opts())
            .unwrap_err()
            .to_string()
            .contains("zlib Adler-32 trailer was not preserved"));
        assert_verifier_matches_legacy(&original, &changed_payload);

        // These distinct three-byte payloads collide in Adler-32 and have the
        // same length/header/trailer. Verification must still compare every byte.
        let collision_a = build(&[10, 10, 10], 9);
        let collision_b = build(&[11, 8, 11], 9);
        assert_eq!(inspect(&collision_a).unwrap().adler, inspect(&collision_b).unwrap().adler);
        assert!(verify(&collision_a, &collision_b, &opts())
            .unwrap_err()
            .to_string()
            .contains("decompressed payload differs"));
        assert_verifier_matches_legacy(&collision_a, &collision_b);

        // Original-side failure still dominates a candidate header failure.
        let mut invalid_original = vec![0x78, 0x9c];
        invalid_original.extend_from_slice(&[0xff; 16]);
        invalid_original.extend_from_slice(&[0, 0, 0, 1]);
        let mut invalid_candidate = original.clone();
        invalid_candidate[0] = 0x68;
        let error = verify(&invalid_original, &invalid_candidate, &opts())
            .unwrap_err()
            .to_string();
        assert!(error.contains("original: zlib: nothing decodes"), "{error}");
        assert_verifier_matches_legacy(&invalid_original, &invalid_candidate);

        // Trailing compressed bytes and an invalid candidate checksum remain
        // classified as candidate verification failures, not payload diffs.
        let mut trailing = original.clone();
        trailing.insert(trailing.len() - TRAILER, 0);
        assert_verifier_matches_legacy(&original, &trailing);
        let mut wrong_adler = original.clone();
        let last = wrong_adler.len() - 1;
        wrong_adler[last] ^= 0xff;
        assert_verifier_matches_legacy(&original, &wrong_adler);
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

    #[test]
    #[ignore = "manual release-mode comparison of streaming and pre-change verifiers"]
    fn streaming_verifier_microbenchmark() {
        use std::time::{Duration, Instant};

        fn median(mut samples: Vec<Duration>) -> Duration {
            samples.sort_unstable();
            samples[samples.len() / 2]
        }

        let payload = compressible(256 * 1024 * 1024);
        let payload_len = payload.len();
        let original = build(&payload, 1);
        drop(payload);
        let mut candidate = Vec::new();
        assert!(matches!(
            optimize(&original, &mut candidate, &opts()).unwrap(),
            Cand::Written(_)
        ));
        assert_verifier_matches_legacy(&original, &candidate);

        for _ in 0..2 {
            verify(&original, &candidate, &opts()).unwrap();
            legacy_verify(&original, &candidate).unwrap();
        }
        let mut streaming = Vec::new();
        let mut legacy = Vec::new();
        for iteration in 0..7 {
            if iteration % 2 == 0 {
                let start = Instant::now();
                verify(&original, &candidate, &opts()).unwrap();
                streaming.push(start.elapsed());
                let start = Instant::now();
                legacy_verify(&original, &candidate).unwrap();
                legacy.push(start.elapsed());
            } else {
                let start = Instant::now();
                legacy_verify(&original, &candidate).unwrap();
                legacy.push(start.elapsed());
                let start = Instant::now();
                verify(&original, &candidate, &opts()).unwrap();
                streaming.push(start.elapsed());
            }
        }
        let streaming_median = median(streaming);
        let legacy_median = median(legacy);
        println!(
            "zlib verify: payload={} bytes, input={} bytes, candidate={} bytes, samples=7; streaming_median_ms={:.3}, legacy_median_ms={:.3}, speedup={:.2}x",
            payload_len,
            original.len(),
            candidate.len(),
            streaming_median.as_secs_f64() * 1000.0,
            legacy_median.as_secs_f64() * 1000.0,
            legacy_median.as_secs_f64() / streaming_median.as_secs_f64(),
        );
    }

}
