//! gzip (RFC 1952).
//!
//! The container is handled here in full: the header is parsed only to find where
//! the body starts and is then copied verbatim, so `FEXTRA`, `FNAME`, `FCOMMENT`,
//! `FHCRC`, `XFL` and `OS` survive exactly as they were, including non-UTF-8
//! names. Only the DEFLATE body is rewritten.

use std::io::{self, Read, Write};

use crate::error::{Error, Result};

use super::{Cand, Options};

const FTEXT: u8 = 0x01;
const FHCRC: u8 = 0x02;
const FEXTRA: u8 = 0x04;
const FNAME: u8 = 0x08;
const FCOMMENT: u8 = 0x10;
const RESERVED: u8 = 0xe0;

const MAX_HEADER: usize = 1 << 20;
const TRAILER: usize = 8;

struct Inspected {
    header_len: usize,
    payload_len: u64,
    crc: u32,
}

/// Parses the header, validates the trailer and confirms the body is exactly one
/// complete DEFLATE stream. Anything else is refused.
fn inspect(data: &[u8]) -> Result<Inspected> {
    let (header_len, _flags) = parse_header(data)?;
    if data.len() < header_len + TRAILER {
        return Err(Error::parse(
            "gzip: file ends before the trailer; the member is truncated",
        ));
    }
    let body = &data[header_len..data.len() - TRAILER];
    let trailer = &data[data.len() - TRAILER..];

    let (payload_len, crc) = inflate(body)?;

    let stored_crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    let stored_size = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
    if stored_crc != crc {
        return Err(Error::parse(
            "gzip: CRC32 does not match the compressed data; the file is corrupt",
        ));
    }
    if stored_size != payload_len as u32 {
        return Err(Error::parse(
            "gzip: ISIZE does not match the compressed data; the file is corrupt",
        ));
    }

    Ok(Inspected {
        header_len,
        payload_len,
        crc,
    })
}

fn parse_header(data: &[u8]) -> Result<(usize, u8)> {
    if data.len() < 10 {
        return Err(Error::parse("gzip: header is shorter than 10 bytes"));
    }
    if data[0] != 0x1f || data[1] != 0x8b {
        return Err(Error::parse("gzip: bad magic bytes"));
    }
    if data[2] != 8 {
        return Err(Error::refused(format!(
            "gzip: unsupported compression method {}",
            data[2]
        )));
    }
    let flags = data[3];
    if flags & RESERVED != 0 {
        return Err(Error::parse("gzip: reserved flag bits are set"));
    }

    let mut pos = 10usize;
    if flags & FEXTRA != 0 {
        if data.len() < pos + 2 {
            return Err(Error::parse("gzip: truncated FEXTRA length"));
        }
        let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        if data.len() < pos + xlen {
            return Err(Error::parse("gzip: truncated FEXTRA field"));
        }
        pos += xlen;
    }
    if flags & FNAME != 0 {
        pos = skip_c_string(data, pos, "FNAME")?;
    }
    if flags & FCOMMENT != 0 {
        pos = skip_c_string(data, pos, "FCOMMENT")?;
    }
    if flags & FHCRC != 0 {
        if data.len() < pos + 2 {
            return Err(Error::parse("gzip: truncated FHCRC"));
        }
        pos += 2;
    }
    let _ = FTEXT;

    if pos > MAX_HEADER {
        return Err(Error::limit("gzip: header is unreasonably large; refused"));
    }
    Ok((pos, flags))
}

fn skip_c_string(data: &[u8], from: usize, field: &str) -> Result<usize> {
    let limit = (from + MAX_HEADER).min(data.len());
    match data[from..limit].iter().position(|b| *b == 0) {
        Some(offset) => Ok(from + offset + 1),
        None => Err(Error::parse(format!("gzip: unterminated {field} field"))),
    }
}

/// Decompresses `body` completely and returns its length and CRC-32.
///
/// `total_in` is compared with the body length so that a second member or any
/// trailing garbage is refused rather than silently dropped.
fn inflate(body: &[u8]) -> Result<(u64, u32)> {
    let mut decoder = flate2::read::DeflateDecoder::new(body);
    let mut hasher = crc32fast::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = decoder.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        total += n as u64;
    }
    if decoder.total_in() != body.len() as u64 {
        return Err(Error::refused(
            "gzip: trailing data after the member (multi-member files are not supported)",
        ));
    }
    Ok((total, hasher.finalize()))
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
    io::copy(
        &mut flate2::read::DeflateDecoder::new(body),
        &mut encoder,
    )?;
    Ok(encoder.finish()?)
}

pub fn optimize(data: &[u8], dst: &mut dyn Write, opts: &Options) -> Result<Cand> {
    let info = inspect(data)?;

    let body = &data[info.header_len..data.len() - TRAILER];
    let mut best = recompress(body, false)?;
    if opts.zopfli {
        let strong = recompress(body, true)?;
        if strong.len() < best.len() {
            best = strong;
        }
    }
    let _ = (info.payload_len, info.crc);

    let total = info.header_len as u64 + best.len() as u64 + TRAILER as u64;
    if total >= data.len() as u64 {
        return Ok(Cand::NoGain);
    }

    dst.write_all(&data[..info.header_len])?;
    dst.write_all(&best)?;
    // The trailer is copied verbatim: it was proven to match the payload above.
    dst.write_all(&data[data.len() - TRAILER..])?;
    Ok(Cand::Written(total))
}

pub fn verify(a: &[u8], b: &[u8]) -> Result<()> {
    let info_a = inspect(a).map_err(|e| Error::verify(format!("original: {e}")))?;
    let info_b = inspect(b).map_err(|e| Error::verify(format!("candidate: {e}")))?;

    if a[..info_a.header_len] != b[..info_b.header_len] {
        return Err(Error::verify("gzip header was not preserved"));
    }
    if a[a.len() - TRAILER..] != b[b.len() - TRAILER..] {
        return Err(Error::verify("gzip trailer was not preserved"));
    }
    if info_a.payload_len != info_b.payload_len || info_a.crc != info_b.crc {
        return Err(Error::verify("gzip payload length or CRC changed"));
    }

    let body_a = &a[info_a.header_len..a.len() - TRAILER];
    let body_b = &b[info_b.header_len..b.len() - TRAILER];
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

    /// Builds a gzip member with optional FNAME and FEXTRA fields.
    pub fn build(payload: &[u8], name: Option<&[u8]>, extra: Option<&[u8]>) -> Vec<u8> {
        let mut flags = 0u8;
        if name.is_some() {
            flags |= FNAME;
        }
        if extra.is_some() {
            flags |= FEXTRA;
        }
        let mut out = vec![0x1f, 0x8b, 0x08, flags, 0, 0, 0, 0, 0, 255];
        if let Some(x) = extra {
            out.extend_from_slice(&(x.len() as u16).to_le_bytes());
            out.extend_from_slice(x);
        }
        if let Some(n) = name {
            out.extend_from_slice(n);
            out.push(0);
        }
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(payload).unwrap();
        out.extend_from_slice(&encoder.finish().unwrap());
        out.extend_from_slice(&crc32fast::hash(payload).to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out
    }

    #[test]
    fn header_is_parsed_with_all_optional_fields() {
        let data = build(b"hello", Some(b"name.txt"), Some(b"abcd"));
        let (len, flags) = parse_header(&data).unwrap();
        assert_eq!(flags & FNAME, FNAME);
        assert_eq!(&data[len - 3..len], b"xt\0");
    }

    #[test]
    fn inspect_accepts_a_valid_member() {
        let data = build(b"hello world", None, None);
        let info = inspect(&data).unwrap();
        assert_eq!(info.payload_len, 11);
        assert_eq!(info.crc, crc32fast::hash(b"hello world"));
    }

    #[test]
    fn reserved_flags_are_refused() {
        let mut data = build(b"x", None, None);
        data[3] |= RESERVED;
        assert!(parse_header(&data).is_err());
    }

    #[test]
    fn bad_method_is_refused() {
        let mut data = build(b"x", None, None);
        data[2] = 7;
        assert!(parse_header(&data).is_err());
    }

    #[test]
    fn corrupt_crc_is_refused() {
        let mut data = build(b"hello", None, None);
        let last = data.len() - 8;
        data[last] ^= 0xff;
        assert!(inspect(&data).is_err());
    }

    #[test]
    fn truncated_member_is_refused() {
        let data = build(b"hello", None, None);
        assert!(inspect(&data[..data.len() - 4]).is_err());
    }

    #[test]
    fn trailing_data_is_refused() {
        let mut data = build(b"hello", None, None);
        let extra = build(b"second", None, None);
        // Append a second member before the first trailer: not a single member.
        let insert = data.len() - 8;
        data.splice(insert..insert, extra.iter().copied());
        assert!(inspect(&data).is_err());
    }

    #[test]
    fn roundtrip_recompression_keeps_payload() {
        let payload = b"the quick brown fox jumps over the lazy dog".repeat(100);
        let data = build(&payload, Some(b"a.txt"), Some(b"zz"));
        let info = inspect(&data).unwrap();
        let body = &data[info.header_len..data.len() - TRAILER];
        let re = recompress(body, false).unwrap();
        let mut rebuilt = data[..info.header_len].to_vec();
        rebuilt.extend_from_slice(&re);
        rebuilt.extend_from_slice(&data[data.len() - TRAILER..]);
        let info2 = inspect(&rebuilt).unwrap();
        assert_eq!(info2.payload_len, info.payload_len);
        assert_eq!(info2.crc, info.crc);
    }
}
