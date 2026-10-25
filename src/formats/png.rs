//! PNG (ISO/IEC 15948).
//!
//! The container is walked by hand so that every chunk that is not IDAT is copied
//! byte for byte: ancillary chunks, private chunks, text, ICC profiles, APNG
//! control chunks and their order all survive untouched. Only the image data
//! stream is recompressed, optionally after re-selecting the scanline filters.
//!
//! Verification parses the candidate strictly (including every chunk CRC) and then
//! decodes both files with the `png` crate, which is independent of the writer
//! above, comparing the decoded image bytes.

use std::io::{self, Cursor, Read, Write};

use crate::error::{Error, Result};

use super::{Cand, Options};

pub const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
const MAX_CHUNK: usize = 0x7fff_ffff;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
    interlace: u8,
}

impl Header {
    fn channels(self) -> usize {
        match self.color {
            0 => 1, // grayscale
            2 => 3, // truecolor
            3 => 1, // palette index
            4 => 2, // grayscale + alpha
            6 => 4, // truecolor + alpha
            _ => 1,
        }
    }

    /// Bytes per pixel, rounded up to one, as used by the PNG filters.
    fn bpp(self) -> usize {
        let bits = self.channels() * self.depth as usize;
        (bits / 8).max(1)
    }

    /// Bytes in one scanline, excluding the filter byte.
    fn row_bytes(self) -> usize {
        let bits = self.width as usize * self.channels() * self.depth as usize;
        bits.div_ceil(8)
    }

    fn pixels(self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

struct Chunk<'a> {
    typ: [u8; 4],
    data: &'a [u8],
    /// Length + type + data + CRC, exactly as it appears in the file.
    raw: &'a [u8],
}

struct Parsed<'a> {
    header: Header,
    chunks: Vec<Chunk<'a>>,
    idat: Vec<&'a [u8]>,
    has_actl: bool,
}

/// Parses a PNG completely. Any inconsistency is an error: Tzim never repairs.
fn parse(data: &[u8]) -> Result<Parsed<'_>> {
    if data.len() < SIGNATURE.len() + 12 {
        return Err(Error::parse("png: file is too short to be a PNG"));
    }
    if data[..SIGNATURE.len()] != SIGNATURE {
        return Err(Error::parse("png: bad signature"));
    }

    let mut chunks: Vec<Chunk<'_>> = Vec::new();
    let mut pos = SIGNATURE.len();
    let mut ended = false;

    while pos < data.len() {
        if data.len() - pos < 12 {
            return Err(Error::parse("png: truncated chunk header"));
        }
        let len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        if len > MAX_CHUNK {
            return Err(Error::parse("png: chunk length exceeds the format limit"));
        }
        let end = pos
            .checked_add(12)
            .and_then(|v| v.checked_add(len))
            .ok_or_else(|| Error::parse("png: chunk length overflows"))?;
        if end > data.len() {
            return Err(Error::parse("png: truncated chunk data"));
        }
        let typ = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];
        let payload = &data[pos + 8..pos + 8 + len];
        let stored_crc =
            u32::from_be_bytes([data[end - 4], data[end - 3], data[end - 2], data[end - 1]]);

        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&typ);
        hasher.update(payload);
        if hasher.finalize() != stored_crc {
            return Err(Error::parse(format!(
                "png: CRC mismatch in chunk `{}`; the file is corrupt",
                String::from_utf8_lossy(&typ)
            )));
        }

        chunks.push(Chunk {
            typ,
            data: payload,
            raw: &data[pos..end],
        });
        pos = end;

        if typ == *b"IEND" {
            ended = true;
            break;
        }
        if chunks.len() > 1_000_000 {
            return Err(Error::limit("png: too many chunks"));
        }
    }

    if !ended {
        return Err(Error::parse("png: missing IEND chunk"));
    }
    if pos != data.len() {
        return Err(Error::parse("png: data after IEND"));
    }
    if chunks[0].typ != *b"IHDR" || chunks[0].data.len() != 13 {
        return Err(Error::parse("png: IHDR is missing or malformed"));
    }

    let ihdr = chunks[0].data;
    let header = Header {
        width: u32::from_be_bytes([ihdr[0], ihdr[1], ihdr[2], ihdr[3]]),
        height: u32::from_be_bytes([ihdr[4], ihdr[5], ihdr[6], ihdr[7]]),
        depth: ihdr[8],
        color: ihdr[9],
        interlace: ihdr[12],
    };
    if ihdr[10] != 0 || ihdr[11] != 0 {
        return Err(Error::parse("png: unknown compression or filter method"));
    }
    if header.width == 0 || header.height == 0 {
        return Err(Error::parse("png: zero width or height"));
    }
    let valid_depth = match header.color {
        0 => matches!(header.depth, 1 | 2 | 4 | 8 | 16),
        2 | 4 | 6 => matches!(header.depth, 8 | 16),
        3 => matches!(header.depth, 1 | 2 | 4 | 8),
        _ => false,
    };
    if !valid_depth {
        return Err(Error::parse(format!(
            "png: invalid bit depth {} for color type {}",
            header.depth, header.color
        )));
    }
    if header.interlace > 1 {
        return Err(Error::parse("png: unknown interlace method"));
    }

    let idat: Vec<&[u8]> = chunks
        .iter()
        .filter(|c| c.typ == *b"IDAT")
        .map(|c| c.data)
        .collect();
    if idat.is_empty() {
        return Err(Error::parse("png: no IDAT chunk"));
    }

    // IDAT chunks must be consecutive.
    let first = chunks.iter().position(|c| c.typ == *b"IDAT").unwrap();
    let last = chunks.iter().rposition(|c| c.typ == *b"IDAT").unwrap();
    if chunks[first..=last].iter().any(|c| c.typ != *b"IDAT") {
        return Err(Error::parse("png: IDAT chunks are not consecutive"));
    }
    if chunks.last().map(|c| c.typ) != Some(*b"IEND")
        || chunks.last().map(|c| c.data.len()) != Some(0)
    {
        return Err(Error::parse("png: IEND is malformed or not last"));
    }

    let has_actl = chunks.iter().any(|c| c.typ == *b"acTL");

    Ok(Parsed {
        header,
        chunks,
        idat,
        has_actl,
    })
}

// ---------------------------------------------------------------------------
// Recompression
// ---------------------------------------------------------------------------

/// Yields the concatenated payloads of the IDAT chunks.
struct Chained<'a> {
    parts: &'a [&'a [u8]],
    index: usize,
    offset: usize,
}

impl<'a> Chained<'a> {
    fn new(parts: &'a [&'a [u8]]) -> Self {
        Chained {
            parts,
            index: 0,
            offset: 0,
        }
    }
}

impl Read for Chained<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.index < self.parts.len() {
            let part = self.parts[self.index];
            if self.offset < part.len() {
                let n = (part.len() - self.offset).min(buf.len());
                buf[..n].copy_from_slice(&part[self.offset..self.offset + n]);
                self.offset += n;
                return Ok(n);
            }
            self.index += 1;
            self.offset = 0;
        }
        Ok(0)
    }
}

/// Reverses the scanline filters, re-selects them with the minimum-sum heuristic
/// used by libpng and optipng, and re-emits the filtered stream.
///
/// The pixels themselves are never modified: unfiltering and filtering are exact
/// inverses, so a different filter choice changes bytes, not content.
struct Refilter<R: Read> {
    inner: R,
    row_bytes: usize,
    bpp: usize,
    prev: Vec<u8>,
    out: Vec<u8>,
    pos: usize,
    done: bool,
}

impl<R: Read> Refilter<R> {
    fn new(inner: R, row_bytes: usize, bpp: usize) -> Self {
        Refilter {
            inner,
            row_bytes,
            bpp,
            prev: vec![0u8; row_bytes],
            out: Vec::new(),
            pos: 0,
            done: false,
        }
    }

    fn refill(&mut self) -> io::Result<()> {
        self.out.clear();
        self.pos = 0;

        let mut raw = vec![0u8; self.row_bytes + 1];
        let mut filled = 0;
        while filled < raw.len() {
            match self.inner.read(&mut raw[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        if filled == 0 {
            self.done = true;
            return Ok(());
        }
        if filled != raw.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "png: scanline is shorter than the header promises",
            ));
        }

        let filter = raw[0];
        let mut cur = vec![0u8; self.row_bytes];
        unfilter(filter, &raw[1..], &self.prev, self.bpp, &mut cur)?;

        let mut best_cost = u64::MAX;
        let mut best_filter = 0u8;
        let mut best = vec![0u8; self.row_bytes];
        let mut scratch = vec![0u8; self.row_bytes];
        for candidate in 0..5u8 {
            filter_row(candidate, &cur, &self.prev, self.bpp, &mut scratch);
            let cost = cost_of(&scratch);
            if cost < best_cost {
                best_cost = cost;
                best_filter = candidate;
                best.copy_from_slice(&scratch);
            }
        }

        self.out.push(best_filter);
        self.out.extend_from_slice(&best);
        self.prev = cur;
        Ok(())
    }
}

impl<R: Read> Read for Refilter<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.pos < self.out.len() {
                let n = (self.out.len() - self.pos).min(buf.len());
                buf[..n].copy_from_slice(&self.out[self.pos..self.pos + n]);
                self.pos += n;
                return Ok(n);
            }
            if self.done {
                return Ok(0);
            }
            self.refill()?;
            if self.done {
                return Ok(0);
            }
        }
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i32 + b as i32 - c as i32;
    let pa = (p - a as i32).abs();
    let pb = (p - b as i32).abs();
    let pc = (p - c as i32).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn unfilter(filter: u8, raw: &[u8], prev: &[u8], bpp: usize, out: &mut [u8]) -> io::Result<()> {
    for i in 0..raw.len() {
        let a = if i >= bpp { out[i - bpp] } else { 0 };
        let b = prev[i];
        let c = if i >= bpp { prev[i - bpp] } else { 0 };
        let value = match filter {
            0 => raw[i],
            1 => raw[i].wrapping_add(a),
            2 => raw[i].wrapping_add(b),
            3 => raw[i].wrapping_add(((a as u16 + b as u16) / 2) as u8),
            4 => raw[i].wrapping_add(paeth(a, b, c)),
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("png: unknown scanline filter {other}"),
                ))
            }
        };
        out[i] = value;
    }
    Ok(())
}

fn filter_row(filter: u8, cur: &[u8], prev: &[u8], bpp: usize, out: &mut [u8]) {
    if filter == 0 {
        out.copy_from_slice(cur);
        return;
    }
    for i in 0..cur.len() {
        let a = if i >= bpp { cur[i - bpp] } else { 0 };
        let b = prev[i];
        let c = if i >= bpp { prev[i - bpp] } else { 0 };
        out[i] = match filter {
            1 => cur[i].wrapping_sub(a),
            2 => cur[i].wrapping_sub(b),
            3 => cur[i].wrapping_sub(((a as u16 + b as u16) / 2) as u8),
            4 => cur[i].wrapping_sub(paeth(a, b, c)),
            _ => cur[i],
        };
    }
}

fn cost_of(bytes: &[u8]) -> u64 {
    let mut cost = 0u64;
    for &b in bytes {
        cost += if b < 128 {
            b as u64
        } else {
            (256 - b as u16) as u64
        };
    }
    cost
}

/// Streams the decompressed (filtered) image data, optionally through the
/// refiltering stage.
enum Body<'a> {
    Plain(flate2::read::ZlibDecoder<Chained<'a>>),
    Refiltered(Refilter<flate2::read::ZlibDecoder<Chained<'a>>>),
}

impl Read for Body<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Body::Plain(r) => r.read(buf),
            Body::Refiltered(r) => r.read(buf),
        }
    }
}

/// Wraps a reader and computes the Adler-32 checksum of everything read.
/// Adler-32 is a checksum, not a cryptographic primitive; roughly fifteen lines
/// here avoid pulling in another crate only to wrap Zopfli's raw DEFLATE output
/// in a zlib stream.
struct AdlerReader<R> {
    inner: R,
    a: u32,
    b: u32,
}

impl<R: Read> AdlerReader<R> {
    fn new(inner: R) -> Self {
        AdlerReader { inner, a: 1, b: 0 }
    }

    fn finish(&self) -> u32 {
        (self.b << 16) | self.a
    }
}

impl<R: Read> Read for AdlerReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        for &byte in &buf[..n] {
            self.a = (self.a + byte as u32) % 65521;
            self.b = (self.b + self.a) % 65521;
        }
        Ok(n)
    }
}

fn encode<'a>(
    parts: &'a [&'a [u8]],
    refilter: Option<(usize, usize)>,
    zopfli: bool,
) -> Result<Vec<u8>> {
    let chained = Chained::new(parts);
    let decoder = flate2::read::ZlibDecoder::new(chained);
    let body = match refilter {
        Some((row_bytes, bpp)) => Body::Refiltered(Refilter::new(decoder, row_bytes, bpp)),
        None => Body::Plain(decoder),
    };

    if zopfli {
        #[cfg(feature = "zopfli")]
        {
            let mut source = AdlerReader::new(body);
            let mut deflate = Vec::new();
            zopfli::compress(
                zopfli::Options::default(),
                zopfli::Format::Deflate,
                &mut source,
                &mut deflate,
            )
            .map_err(|e| Error::Internal(format!("zopfli failed: {e}")))?;
            let mut out = Vec::with_capacity(deflate.len() + 6);
            out.extend_from_slice(&[0x78, 0xda]);
            out.extend_from_slice(&deflate);
            out.extend_from_slice(&source.finish().to_be_bytes());
            return Ok(out);
        }
        #[cfg(not(feature = "zopfli"))]
        {
            let _ = zopfli;
        }
    }

    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    let mut body = body;
    io::copy(&mut body, &mut encoder)?;
    Ok(encoder.finish()?)
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Recompresses the default image and, for animated files, every frame.
///
/// For `fdAT` runs the chunk count, the chunk order and the sequence numbers are
/// preserved: the recompressed stream is split back into the same number of
/// chunks, each keeping the sequence number of the chunk it replaces. If the new
/// stream is too short to fill them, that frame is left exactly as it was. This
/// is why `fcTL` payloads can stay byte-identical.
pub fn optimize(input: &[u8], out: &mut dyn Write, opts: &Options) -> Result<Cand> {
    let parsed = parse(input)?;

    if parsed.header.pixels() > opts.max_pixels {
        return Err(Error::limit(format!(
            "png: {} pixels exceeds the limit of {}",
            parsed.header.pixels(),
            opts.max_pixels
        )));
    }

    let mut best = encode(&parsed.idat, None, opts.zopfli)?;

    // Filter re-selection needs whole, non-interlaced frames.
    if parsed.header.interlace == 0 && !parsed.has_actl {
        let candidate = encode(
            &parsed.idat,
            Some((parsed.header.row_bytes(), parsed.header.bpp())),
            opts.zopfli,
        )?;
        if candidate.len() < best.len() {
            best = candidate;
        }
    }

    // One replacement per chunk; `None` means "copy the original chunk".
    let mut replacements: Vec<Option<Vec<u8>>> = vec![None; parsed.chunks.len()];
    let idat_positions: Vec<usize> = parsed
        .chunks
        .iter()
        .enumerate()
        .filter(|(_, c)| c.typ == *b"IDAT")
        .map(|(i, _)| i)
        .collect();
    if let Some(&first) = idat_positions.first() {
        replacements[first] = Some(best);
    }
    for &position in &idat_positions {
        if position != idat_positions[0] {
            replacements[position] = Some(Vec::new());
        }
    }

    if parsed.has_actl {
        for (positions, stream) in frame_runs(&parsed) {
            let new_stream = match encode(&[&stream], None, opts.zopfli) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            if new_stream.len() >= stream.len() || positions.len() < 2 {
                continue;
            }
            for (slot, piece) in positions
                .iter()
                .zip(split_evenly(&new_stream, positions.len()))
            {
                replacements[*slot] = Some(piece.to_vec());
            }
        }
    }

    let total = total_len(&parsed, &replacements, opts.strip_metadata)?;
    if total >= input.len() as u64 {
        return Ok(Cand::NoGain);
    }

    write_candidate(&parsed, &replacements, opts.strip_metadata, out)?;
    Ok(Cand::Written(total))
}

/// Maximal runs of consecutive `fdAT` chunks, with their concatenated image data
/// (the leading sequence number of each chunk removed).
fn frame_runs(parsed: &Parsed<'_>) -> Vec<(Vec<usize>, Vec<u8>)> {
    let mut runs = Vec::new();
    let mut current: Vec<usize> = Vec::new();

    for (index, chunk) in parsed.chunks.iter().enumerate() {
        if chunk.typ == *b"fdAT" {
            if chunk.data.len() < 4 {
                current.clear();
                continue;
            }
            current.push(index);
        } else if !current.is_empty() {
            let stream = collect_fdat(parsed, &current);
            runs.push((std::mem::take(&mut current), stream));
        }
    }
    if !current.is_empty() {
        let stream = collect_fdat(parsed, &current);
        runs.push((current, stream));
    }
    runs.retain(|(positions, stream)| !positions.is_empty() && !stream.is_empty());
    runs
}

fn collect_fdat(parsed: &Parsed<'_>, positions: &[usize]) -> Vec<u8> {
    let mut out = Vec::new();
    for &position in positions {
        if let Some(data) = parsed.chunks[position].data.get(4..) {
            out.extend_from_slice(data);
        }
    }
    out
}

/// Splits `data` into `count` non-empty pieces of near-equal size.
fn split_evenly(data: &[u8], count: usize) -> Vec<&[u8]> {
    let base = data.len() / count;
    let extra = data.len() % count;
    let mut pieces = Vec::with_capacity(count);
    let mut at = 0;
    for i in 0..count {
        let size = base + usize::from(i < extra);
        pieces.push(&data[at..at + size]);
        at += size;
    }
    pieces
}

fn total_len(parsed: &Parsed<'_>, replacements: &[Option<Vec<u8>>], strip: bool) -> Result<u64> {
    let mut total = SIGNATURE.len() as u64;
    for (index, chunk) in parsed.chunks.iter().enumerate() {
        if !survives(&chunk.typ, strip) {
            continue;
        }
        match &replacements[index] {
            Some(data) => total += 12 + data.len() as u64,
            None => total += chunk.raw.len() as u64,
        }
    }
    Ok(total)
}

fn write_candidate(
    parsed: &Parsed<'_>,
    replacements: &[Option<Vec<u8>>],
    strip: bool,
    out: &mut dyn Write,
) -> Result<()> {
    out.write_all(&SIGNATURE)?;
    for (index, chunk) in parsed.chunks.iter().enumerate() {
        if !survives(&chunk.typ, strip) {
            continue;
        }
        match &replacements[index] {
            Some(data) => {
                if data.len() > MAX_CHUNK {
                    return Err(Error::internal("png: recompressed chunk is too large"));
                }
                let mut payload = Vec::with_capacity(data.len() + 4);
                if chunk.typ == *b"fdAT" {
                    // Keep the sequence number of the chunk being replaced.
                    payload.extend_from_slice(&chunk.data[..4]);
                }
                payload.extend_from_slice(data);
                write_chunk(out, &chunk.typ, &payload)?;
            }
            None => out.write_all(chunk.raw)?,
        }
    }
    Ok(())
}

fn write_chunk(w: &mut dyn Write, typ: &[u8; 4], data: &[u8]) -> Result<()> {
    let len = u32::try_from(data.len()).map_err(|_| Error::internal("png: chunk exceeds u32"))?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(typ)?;
    w.write_all(data)?;
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(typ);
    hasher.update(data);
    w.write_all(&hasher.finalize().to_be_bytes())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// What `--strip-metadata` removes
// ---------------------------------------------------------------------------

/// Chunks removed when the user asks for `--strip-metadata`.
///
/// The list is deliberately short and textual. Anything that changes how the
/// image looks, prints or animates stays: `iCCP`, `sRGB`, `gAMA`, `cHRM`,
/// `pHYs` (print size), `sBIT`, `bKGD`, `tRNS`, `hIST`, `sPLT`, `acTL`, `fcTL`,
/// `fdAT`. Chunks this build does not know are kept as well: deleting something
/// whose meaning we cannot check is not a decision this tool makes, even when
/// asked nicely.
pub fn is_stripped(typ: &[u8; 4]) -> bool {
    matches!(typ, b"tEXt" | b"zTXt" | b"iTXt" | b"tIME" | b"eXIf")
}

/// True when this chunk survives a stripping rewrite.
fn survives(typ: &[u8; 4], strip: bool) -> bool {
    !(strip && is_stripped(typ))
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

type PngReader<'a> = png::Reader<Cursor<&'a [u8]>>;

fn open_reader(data: &[u8]) -> Result<PngReader<'_>> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::IDENTITY);
    decoder
        .read_info()
        .map_err(|e| Error::verify(format!("png: cannot decode: {e}")))
}

pub fn verify(original: &[u8], candidate: &[u8], opts: &Options) -> Result<()> {
    let pa = parse(original).map_err(|e| Error::verify(format!("original: {e}")))?;
    let pb = parse(candidate).map_err(|e| Error::verify(format!("candidate: {e}")))?;

    if pa.header != pb.header {
        return Err(Error::verify("IHDR changed"));
    }

    // With stripping enabled, a chunk may only disappear when it is on the list,
    // and it must actually be gone: a verifier that accepted both would hide the
    // case where the writer forgot to remove it.
    if opts.strip_metadata {
        for chunk in &pb.chunks {
            if is_stripped(&chunk.typ) {
                return Err(Error::verify(format!(
                    "chunk `{}` survived --strip-metadata",
                    String::from_utf8_lossy(&chunk.typ)
                )));
            }
        }
    }

    // Everything that is neither IDAT nor fdAT must be byte-identical and in the
    // same order. That covers PLTE, tRNS, iCCP, text, unknown chunks, acTL and
    // every fcTL (frame delays, offsets, blend and dispose operations).
    let kept_a: Vec<_> = pa
        .chunks
        .iter()
        .filter(|c| c.typ != *b"IDAT" && c.typ != *b"fdAT")
        .filter(|c| survives(&c.typ, opts.strip_metadata))
        .collect();
    let kept_b: Vec<_> = pb
        .chunks
        .iter()
        .filter(|c| c.typ != *b"IDAT" && c.typ != *b"fdAT")
        .collect();
    if kept_a.len() != kept_b.len() {
        return Err(Error::verify("chunk count changed"));
    }
    for (x, y) in kept_a.iter().zip(kept_b.iter()) {
        if x.typ != y.typ || x.raw != y.raw {
            return Err(Error::verify(format!(
                "chunk `{}` was not preserved",
                String::from_utf8_lossy(&x.typ)
            )));
        }
    }

    // Frame chunks keep their count, their order and their sequence numbers.
    let fdat_a: Vec<_> = pa.chunks.iter().filter(|c| c.typ == *b"fdAT").collect();
    let fdat_b: Vec<_> = pb.chunks.iter().filter(|c| c.typ == *b"fdAT").collect();
    if fdat_a.len() != fdat_b.len() {
        return Err(Error::verify("fdAT chunk count changed"));
    }
    for (x, y) in fdat_a.iter().zip(fdat_b.iter()) {
        if x.data.len() < 4 || y.data.len() < 4 || x.data[..4] != y.data[..4] {
            return Err(Error::verify("fdAT sequence number changed"));
        }
    }

    compare_images(original, candidate, pa.header, pa.has_actl)
}

fn compare_images(a: &[u8], b: &[u8], header: Header, animated: bool) -> Result<()> {
    if header.interlace == 0 && !animated {
        let mut ra = open_reader(a)?;
        let mut rb = open_reader(b)?;
        let row = header.row_bytes();
        if row == 0 {
            return Err(Error::verify("zero-width scanline"));
        }
        let mut buf_a = vec![0u8; row];
        let mut buf_b = vec![0u8; row];
        loop {
            let na = ra
                .read_row(&mut buf_a)
                .map_err(|e| Error::verify(format!("original: {e}")))?;
            let nb = rb
                .read_row(&mut buf_b)
                .map_err(|e| Error::verify(format!("candidate: {e}")))?;
            match (na, nb) {
                (None, None) => break,
                (Some(_), Some(_)) => {
                    if buf_a != buf_b {
                        return Err(Error::verify("decoded pixels differ"));
                    }
                }
                _ => return Err(Error::verify("frame row count differs")),
            }
        }
        Ok(())
    } else {
        compare_frames(a, b)
    }
}

/// Decodes every frame of both files and compares the frame metadata and the
/// decoded bytes. Used for interlaced images and for APNG.
fn compare_frames(a: &[u8], b: &[u8]) -> Result<()> {
    let mut ra = open_reader(a)?;
    let mut rb = open_reader(b)?;
    let size = ra
        .output_buffer_size()
        .ok_or_else(|| Error::limit("png: image is too large to verify"))?;
    if size > 512 * 1024 * 1024 {
        return Err(Error::limit(
            "png: image is too large to verify within the memory budget",
        ));
    }
    let mut buf_a = vec![0u8; size];
    let mut buf_b = vec![0u8; size];

    let mut frames = 0usize;
    loop {
        let fa = ra.next_frame(&mut buf_a);
        let fb = rb.next_frame(&mut buf_b);
        match (fa, fb) {
            (Ok(ia), Ok(ib)) => {
                frames += 1;
                if ia.width != ib.width
                    || ia.height != ib.height
                    || ia.line_size != ib.line_size
                    || ia.color_type != ib.color_type
                    || ia.bit_depth != ib.bit_depth
                {
                    return Err(Error::verify("frame layout differs"));
                }
                let n = ia.buffer_size().min(ib.buffer_size());
                if buf_a[..n] != buf_b[..n] {
                    return Err(Error::verify(format!("frame {frames} pixels differ")));
                }
                if frame_control_debug(&ra) != frame_control_debug(&rb) {
                    return Err(Error::verify(format!(
                        "frame {frames} control data differs"
                    )));
                }
            }
            (Err(_), Err(_)) => break,
            (Ok(_), Err(e)) => return Err(Error::verify(format!("candidate stops early: {e}"))),
            (Err(e), Ok(_)) => return Err(Error::verify(format!("original stops early: {e}"))),
        }
    }

    if frames == 0 {
        return Err(Error::verify("no frames could be decoded"));
    }
    Ok(())
}

fn frame_control_debug(reader: &PngReader<'_>) -> Option<String> {
    reader
        .info()
        .frame_control
        .as_ref()
        .map(|f| format!("{f:?}"))
}

// ---------------------------------------------------------------------------
// ICO / CUR
// ---------------------------------------------------------------------------

const ICO_HEADER: usize = 6;
const ICO_ENTRY: usize = 16;

/// Extracts the embedded images of an icon file.
struct Icon<'a> {
    kind: u16,
    header: &'a [u8],
    entries: Vec<IconEntry<'a>>,
}

struct IconEntry<'a> {
    /// The complete 16-byte directory entry.
    record: &'a [u8],
    payload: &'a [u8],
}

fn parse_icon(data: &[u8]) -> Result<Icon<'_>> {
    if data.len() < ICO_HEADER {
        return Err(Error::parse("ico: file is too short"));
    }
    let reserved = u16::from_le_bytes([data[0], data[1]]);
    let kind = u16::from_le_bytes([data[2], data[3]]);
    let count = u16::from_le_bytes([data[4], data[5]]) as usize;
    if reserved != 0 {
        return Err(Error::parse("ico: reserved field is not zero"));
    }
    if kind != 1 && kind != 2 {
        return Err(Error::parse("ico: not an icon or cursor file"));
    }
    if count == 0 || count > 256 {
        return Err(Error::parse("ico: implausible image count"));
    }
    let directory_end = ICO_HEADER + count * ICO_ENTRY;
    if data.len() < directory_end {
        return Err(Error::parse("ico: truncated directory"));
    }

    let mut entries = Vec::with_capacity(count);
    let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(count);
    for i in 0..count {
        let at = ICO_HEADER + i * ICO_ENTRY;
        let record = &data[at..at + ICO_ENTRY];
        let size = u32::from_le_bytes([record[8], record[9], record[10], record[11]]) as usize;
        let offset = u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
        let start = offset as usize;
        let end = start
            .checked_add(size)
            .ok_or_else(|| Error::parse("ico: image extent overflows"))?;
        if size == 0 || end > data.len() || start < directory_end {
            return Err(Error::parse("ico: image lies outside the file"));
        }
        ranges.push((start, end));
        entries.push(IconEntry {
            record,
            payload: &data[start..end],
        });
    }

    // Images must not overlap, and every byte of the file must belong to one of
    // them: an unreferenced tail would be silently lost by a rewrite.
    ranges.sort_by_key(|r| r.0);
    let mut covered = directory_end;
    for (start, end) in &ranges {
        if *start != covered {
            return Err(Error::parse(
                "ico: images overlap or the file has unreferenced data",
            ));
        }
        covered = *end;
    }
    if covered != data.len() {
        return Err(Error::parse(
            "ico: trailing data after the last image is not supported",
        ));
    }

    Ok(Icon {
        kind,
        header: &data[..directory_end],
        entries,
    })
}

pub fn optimize_icon(input: &[u8], out: &mut dyn Write, opts: &Options) -> Result<Cand> {
    let icon = parse_icon(input)?;

    // Optimize every embedded PNG; non-PNG (DIB) images are copied verbatim.
    let mut payloads: Vec<Vec<u8>> = Vec::with_capacity(icon.entries.len());
    for entry in &icon.entries {
        if let Some(bytes) = optimize_embedded_png(entry.payload, opts)? {
            payloads.push(bytes);
        } else {
            payloads.push(entry.payload.to_vec());
        }
    }
    if payloads.len() == icon.entries.len()
        && payloads
            .iter()
            .zip(icon.entries.iter())
            .all(|(new, old)| new.len() == old.payload.len())
    {
        return Ok(Cand::NoGain);
    }

    // Header stays as it was; only the size and offset fields of each directory
    // entry change, because the images moved.
    let mut directory = icon.header.to_vec();
    let mut body = Vec::new();
    let mut cursor = directory.len();
    for (index, _entry) in icon.entries.iter().enumerate() {
        let payload = &payloads[index];
        let at = ICO_HEADER + index * ICO_ENTRY;
        directory[at + 8..at + 12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        directory[at + 12..at + 16].copy_from_slice(&(cursor as u32).to_le_bytes());
        cursor += payload.len();
        body.extend_from_slice(payload);
    }

    let total = (directory.len() + body.len()) as u64;
    if total >= input.len() as u64 {
        return Ok(Cand::NoGain);
    }

    out.write_all(&directory)?;
    out.write_all(&body)?;
    Ok(Cand::Written(total))
}

/// Returns the optimized bytes of an embedded PNG, or `None` if the payload is
/// not a PNG or gained nothing.
fn optimize_embedded_png(payload: &[u8], opts: &Options) -> Result<Option<Vec<u8>>> {
    if payload.len() < SIGNATURE.len() || payload[..SIGNATURE.len()] != SIGNATURE {
        return Ok(None);
    }
    let mut candidate = Vec::new();
    match optimize(payload, &mut candidate, opts)? {
        Cand::Written(_) if candidate.len() < payload.len() => Ok(Some(candidate)),
        _ => Ok(None),
    }
}

pub fn verify_icon(original: &[u8], candidate: &[u8], opts: &Options) -> Result<()> {
    let a = parse_icon(original).map_err(|e| Error::verify(format!("original: {e}")))?;
    let b = parse_icon(candidate).map_err(|e| Error::verify(format!("candidate: {e}")))?;

    if a.kind != b.kind {
        return Err(Error::verify("icon type changed"));
    }
    if a.entries.len() != b.entries.len() {
        return Err(Error::verify("image count changed"));
    }
    for (index, (ea, eb)) in a.entries.iter().zip(b.entries.iter()).enumerate() {
        // Everything except the size and offset fields must be identical.
        if ea.record[..8] != eb.record[..8] {
            return Err(Error::verify(format!(
                "image {index}: directory fields changed"
            )));
        }
        let is_png =
            ea.payload.len() >= SIGNATURE.len() && ea.payload[..SIGNATURE.len()] == SIGNATURE;
        if is_png {
            verify(ea.payload, eb.payload, opts)
                .map_err(|e| Error::verify(format!("image {index}: {e}")))?;
        } else if ea.payload != eb.payload {
            return Err(Error::verify(format!(
                "image {index}: bitmap payload changed"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(typ: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(typ);
        out.extend_from_slice(data);
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(typ);
        hasher.update(data);
        out.extend_from_slice(&hasher.finalize().to_be_bytes());
        out
    }

    /// Builds a small truecolor RGBA PNG with the given scanline filter.
    pub fn build_png(
        width: u32,
        height: u32,
        filter: u8,
        extra_chunk: Option<(&[u8; 4], Vec<u8>)>,
    ) -> Vec<u8> {
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // depth 8, color 6, no interlace

        let row_bytes = width as usize * 4;
        let mut raw = Vec::new();
        for y in 0..height as usize {
            raw.push(filter);
            for x in 0..width as usize {
                raw.push((x * 7 + y * 3) as u8);
                raw.push((x * 5) as u8);
                raw.push((y * 11) as u8);
                raw.push(0xff);
            }
        }

        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(&raw).unwrap();
        let idat = encoder.finish().unwrap();

        let mut out = SIGNATURE.to_vec();
        out.extend_from_slice(&chunk(b"IHDR", &ihdr));
        if let Some((typ, data)) = extra_chunk {
            out.extend_from_slice(&chunk(typ, &data));
        }
        out.extend_from_slice(&chunk(b"IDAT", &idat));
        out.extend_from_slice(&chunk(b"IEND", &[]));
        let _ = row_bytes;
        out
    }

    #[test]
    fn parses_a_valid_png() {
        let data = build_png(4, 4, 0, None);
        let parsed = parse(&data).unwrap();
        assert_eq!(parsed.header.width, 4);
        assert_eq!(parsed.header.channels(), 4);
        assert_eq!(parsed.header.row_bytes(), 16);
        assert_eq!(parsed.header.bpp(), 4);
        assert_eq!(parsed.idat.len(), 1);
    }

    #[test]
    fn refuses_bad_crc() {
        let mut data = build_png(2, 2, 0, None);
        let position = data.len() - 20;
        data[position] ^= 0xff;
        assert!(parse(&data).is_err());
    }

    #[test]
    fn refuses_trailing_data() {
        let mut data = build_png(2, 2, 0, None);
        data.push(0);
        assert!(parse(&data).is_err());
    }

    #[test]
    fn refuses_truncated_file() {
        let data = build_png(2, 2, 0, None);
        assert!(parse(&data[..data.len() - 3]).is_err());
    }

    #[test]
    fn filters_roundtrip_for_all_types() {
        // Every bytes-per-pixel value a real file can have, not just RGBA8's
        // four: bpp 1 is 1-bit gray or palette, 2 is gray+alpha or 16-bit gray,
        // 3 is RGB or 16-bit palette, 4 is RGBA or 16-bit gray+alpha, 6 is RGB16
        // and 8 is RGBA16. The filter byte arithmetic is defined in terms of bpp,
        // so this is the table that pins it. (#33: the old version tested bpp 4
        // only while `docs/testing.md` claimed every value.)
        for bpp in 1..=8usize {
            let row_len = bpp * 4;
            let prev: Vec<u8> = (0..row_len).map(|i| (i * 7 + 3) as u8).collect();
            let cur: Vec<u8> = (0..row_len).map(|i| (i * 11 + 5) as u8).collect();
            for filter in 0..5u8 {
                let mut filtered = vec![0u8; cur.len()];
                filter_row(filter, &cur, &prev, bpp, &mut filtered);
                let mut restored = vec![0u8; cur.len()];
                unfilter(filter, &filtered, &prev, bpp, &mut restored).unwrap();
                assert_eq!(
                    restored, cur,
                    "bpp {bpp} filter {filter} did not round-trip"
                );
            }
        }
    }

    #[test]
    fn refilter_preserves_pixels() {
        let width = 8usize;
        let height = 5usize;
        let row_bytes = width * 4;
        // Build the filtered stream with filter 0, then refilter it.
        let mut raw = Vec::new();
        for y in 0..height {
            raw.push(0u8);
            for x in 0..width {
                raw.extend_from_slice(&[(x + y) as u8, (x * 3) as u8, y as u8, 0xff]);
            }
        }
        let mut out = Vec::new();
        let mut refilter = Refilter::new(Cursor::new(raw.clone()), row_bytes, 4);
        refilter.read_to_end(&mut out).unwrap();
        assert_eq!(out.len(), raw.len());

        // Unfilter the produced stream and compare with the original pixels.
        let mut restored = Vec::new();
        let mut prev = vec![0u8; row_bytes];
        for chunk_start in (0..out.len()).step_by(row_bytes + 1) {
            let filter = out[chunk_start];
            let mut cur = vec![0u8; row_bytes];
            unfilter(
                filter,
                &out[chunk_start + 1..chunk_start + 1 + row_bytes],
                &prev,
                4,
                &mut cur,
            )
            .unwrap();
            restored.extend_from_slice(&cur);
            prev = cur;
        }
        let expected: Vec<u8> = (0..height)
            .flat_map(|y| {
                (0..width).flat_map(move |x| [(x + y) as u8, (x * 3) as u8, y as u8, 0xff])
            })
            .collect();
        assert_eq!(restored, expected);
    }

    #[test]
    fn adler_matches_reference_values() {
        let mut reader = AdlerReader::new(Cursor::new(b"Wikipedia".to_vec()));
        let mut sink = Vec::new();
        reader.read_to_end(&mut sink).unwrap();
        assert_eq!(reader.finish(), 0x11E6_0398);
    }
}
