//! JPEG (ITU-T T.81) — baseline sequential, Huffman-coded.
//!
//! Only the entropy-coded data is rewritten: every DCT coefficient is decoded,
//! counted, and re-encoded with optimal Huffman tables. The coefficients
//! themselves are never touched, so the decoded image is bit-identical.
//!
//! Every other marker is copied as a raw segment: `APPn` (JFIF, Exif, ICC),
//! `COM`, `DQT`, `SOF0`, `DRI`, the `SOS` header and the bytes after `EOI`.
//! The only things that change are the `DHT` tables that the scan actually uses,
//! the entropy-coded bytes, and the length fields that follow from those.
//!
//! Anything that is not baseline sequential with a single scan is refused
//! rather than approximated: progressive, arithmetic-coded, lossless,
//! hierarchical, 12-bit, SOF1 and multi-scan files.

use std::collections::BTreeMap;
use std::io::Write;

use crate::error::{Error, Result};

use super::{Cand, Options};

const SOI: u8 = 0xd8;
const EOI: u8 = 0xd9;
const SOS: u8 = 0xda;
const DHT: u8 = 0xc4;
const DRI: u8 = 0xdd;
const SOF0: u8 = 0xc0;
const DAC: u8 = 0xcc;
const DNL: u8 = 0xdc;

/// Upper bound on the coefficient arrays held in memory for one file.
const MAX_COEFFICIENT_BYTES: u64 = 256 * 1024 * 1024;

/// `(class, id)`: class 0 is DC, class 1 is AC.
type TableKey = (u8, u8);

// ---------------------------------------------------------------------------
// Container
// ---------------------------------------------------------------------------

struct Segment {
    marker: u8,
    /// Payload without the two length bytes; empty for stand-alone markers.
    payload: Vec<u8>,
    /// Present only for `SOS`: the entropy-coded bytes that follow the header.
    entropy: Option<Vec<u8>>,
}

struct Image {
    segments: Vec<Segment>,
    /// Bytes after `EOI`, if any. Preserved verbatim.
    trailing: Vec<u8>,
}

fn parse(data: &[u8]) -> Result<Image> {
    if data.len() < 4 || data[0] != 0xff || data[1] != SOI {
        return Err(Error::parse("jpeg: the file does not start with an SOI marker"));
    }
    let mut segments = Vec::new();
    let mut pos = 2usize;

    let trailing = loop {
        // A marker may be preceded by any number of 0xFF fill bytes.
        while pos + 1 < data.len() && data[pos] == 0xff && data[pos + 1] == 0xff {
            pos += 1;
        }
        if pos + 1 >= data.len() {
            return Err(Error::parse("jpeg: the file ends before an EOI marker"));
        }
        if data[pos] != 0xff {
            return Err(Error::parse(
                "jpeg: unexpected byte where a marker was expected",
            ));
        }
        let marker = data[pos + 1];
        pos += 2;

        if marker == EOI {
            break data[pos..].to_vec();
        }
        if marker == SOI || marker == 0x01 || marker == 0x00 || (0xd0..=0xd7).contains(&marker) {
            return Err(Error::parse(format!(
                "jpeg: unexpected stand-alone marker 0x{marker:02X}"
            )));
        }
        if pos + 2 > data.len() {
            return Err(Error::parse("jpeg: a marker length field is truncated"));
        }
        let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        if len < 2 || pos + len > data.len() {
            return Err(Error::parse(
                "jpeg: a marker length runs past the end of the file",
            ));
        }
        let payload = data[pos + 2..pos + len].to_vec();
        pos += len;

        if marker == SOS {
            let (entropy, next) = scan_entropy(data, pos)?;
            pos = next;
            segments.push(Segment {
                marker,
                payload,
                entropy: Some(entropy),
            });
        } else {
            segments.push(Segment {
                marker,
                payload,
                entropy: None,
            });
        }
    };

    Ok(Image { segments, trailing })
}

/// Splits the entropy-coded bytes from the marker that terminates the scan.
///
/// Stuffed zero bytes (`FF 00`), restart markers (`FF D0`–`FF D7`) and fill
/// bytes (`FF FF`) belong to the scan; anything else ends it.
fn scan_entropy(data: &[u8], start: usize) -> Result<(Vec<u8>, usize)> {
    let mut pos = start;
    while pos < data.len() {
        if data[pos] != 0xff {
            pos += 1;
            continue;
        }
        let mut next = pos + 1;
        while next < data.len() && data[next] == 0xff {
            next += 1;
        }
        if next >= data.len() {
            return Err(Error::parse(
                "jpeg: the scan ends inside a marker prefix; the file is truncated",
            ));
        }
        match data[next] {
            0x00 | 0xd0..=0xd7 => pos = next + 1,
            _ => return Ok((data[start..pos].to_vec(), pos)),
        }
    }
    Err(Error::parse(
        "jpeg: the scan data ends without a marker; the file is truncated",
    ))
}

// ---------------------------------------------------------------------------
// Frame and scan headers
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq, Eq, Debug)]
struct Component {
    id: u8,
    h: usize,
    v: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Frame {
    width: usize,
    height: usize,
    comps: Vec<Component>,
    hmax: usize,
    vmax: usize,
}

impl Frame {
    fn index_of(&self, id: u8) -> Option<usize> {
        self.comps.iter().position(|c| c.id == id)
    }

    fn mcu_grid(&self) -> (usize, usize) {
        (
            self.width.div_ceil(8 * self.hmax),
            self.height.div_ceil(8 * self.vmax),
        )
    }
}

fn parse_sof(payload: &[u8]) -> Result<Frame> {
    if payload.len() < 6 {
        return Err(Error::parse("jpeg: the frame header is too short"));
    }
    let precision = payload[0];
    if precision != 8 {
        return Err(Error::refused(format!(
            "jpeg: {precision}-bit samples are not supported (8-bit baseline only)"
        )));
    }
    let height = u16::from_be_bytes([payload[1], payload[2]]) as usize;
    let width = u16::from_be_bytes([payload[3], payload[4]]) as usize;
    let count = payload[5] as usize;
    if width == 0 || height == 0 {
        return Err(Error::refused(
            "jpeg: zero width or height (height deferred to a DNL marker) is not supported",
        ));
    }
    if count == 0 || count > 4 {
        return Err(Error::refused(format!(
            "jpeg: {count} components are not supported"
        )));
    }
    if payload.len() != 6 + count * 3 {
        return Err(Error::parse(
            "jpeg: the frame header length does not match its component count",
        ));
    }
    let mut comps: Vec<Component> = Vec::with_capacity(count);
    let mut hmax = 1usize;
    let mut vmax = 1usize;
    for i in 0..count {
        let id = payload[6 + i * 3];
        let hv = payload[7 + i * 3];
        let tq = payload[8 + i * 3];
        let h = (hv >> 4) as usize;
        let v = (hv & 0x0f) as usize;
        if !(1..=4).contains(&h) || !(1..=4).contains(&v) {
            return Err(Error::refused(format!(
                "jpeg: sampling factor {h}x{v} is not supported"
            )));
        }
        if tq > 3 {
            return Err(Error::parse(
                "jpeg: a quantization table selector is out of range",
            ));
        }
        if comps.iter().any(|c| c.id == id) {
            return Err(Error::parse("jpeg: duplicate component id in the frame header"));
        }
        hmax = hmax.max(h);
        vmax = vmax.max(v);
        comps.push(Component { id, h, v });
    }
    Ok(Frame {
        width,
        height,
        comps,
        hmax,
        vmax,
    })
}

struct ScanComponent {
    id: u8,
    dc: u8,
    ac: u8,
}

struct Scan {
    comps: Vec<ScanComponent>,
}

fn parse_sos(payload: &[u8], frame: &Frame) -> Result<Scan> {
    if payload.is_empty() {
        return Err(Error::parse("jpeg: the scan header is empty"));
    }
    let count = payload[0] as usize;
    if payload.len() != 1 + count * 2 + 3 {
        return Err(Error::parse(
            "jpeg: the scan header length does not match its component count",
        ));
    }
    let mut comps = Vec::with_capacity(count);
    for i in 0..count {
        let id = payload[1 + i * 2];
        let selectors = payload[2 + i * 2];
        let dc = selectors >> 4;
        let ac = selectors & 0x0f;
        if dc > 3 || ac > 3 {
            return Err(Error::parse(
                "jpeg: a Huffman table selector in the scan header is out of range",
            ));
        }
        if frame.index_of(id).is_none() {
            return Err(Error::parse(
                "jpeg: the scan header references a component that is not in the frame",
            ));
        }
        if comps.iter().any(|c: &ScanComponent| c.id == id) {
            return Err(Error::parse("jpeg: duplicate component in the scan header"));
        }
        comps.push(ScanComponent { id, dc, ac });
    }
    // A single baseline scan either carries every component of the frame
    // (interleaved) or is a one-component image.
    if count != frame.comps.len() {
        return Err(Error::refused(
            "jpeg: non-interleaved multi-component scans are not supported",
        ));
    }
    let (ss, se, ah_al) = (
        payload[1 + count * 2],
        payload[2 + count * 2],
        payload[3 + count * 2],
    );
    if ss != 0 || se != 63 || ah_al != 0 {
        return Err(Error::refused(
            "jpeg: progressive or successive-approximation scans are not supported (baseline only)",
        ));
    }
    Ok(Scan { comps })
}

// ---------------------------------------------------------------------------
// Huffman tables
// ---------------------------------------------------------------------------

/// A table exactly as it is stored in a `DHT` segment.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Huff {
    /// `counts[len]` is the number of codes of that length; index 0 is unused.
    counts: [u8; 17],
    vals: Vec<u8>,
}

fn parse_dht(payload: &[u8]) -> Result<Vec<(TableKey, Huff)>> {
    let mut tables = Vec::new();
    let mut p = 0usize;
    while p < payload.len() {
        if p + 17 > payload.len() {
            return Err(Error::parse("jpeg: a DHT segment is truncated"));
        }
        let class = payload[p] >> 4;
        let id = payload[p] & 0x0f;
        if class > 1 || id > 3 {
            return Err(Error::parse("jpeg: a Huffman table selector is out of range"));
        }
        let mut counts = [0u8; 17];
        counts[1..17].copy_from_slice(&payload[p + 1..p + 17]);
        p += 17;
        let total: usize = counts[1..].iter().map(|&c| c as usize).sum();
        if total > 256 || p + total > payload.len() {
            return Err(Error::parse("jpeg: a DHT segment has an invalid symbol count"));
        }
        let vals = payload[p..p + total].to_vec();
        p += total;
        tables.push(((class, id), Huff { counts, vals }));
    }
    Ok(tables)
}

/// Checks everything the standard requires of a table we are willing to decode:
/// no zero-length codes, no overfull space, no duplicates, no all-ones code.
#[allow(clippy::needless_range_loop)]
fn validate_table(h: &Huff) -> Result<()> {
    if h.counts[0] != 0 {
        return Err(Error::parse("jpeg: a Huffman table has a zero-length code"));
    }
    let total: usize = h.counts[1..].iter().map(|&c| c as usize).sum();
    if total == 0 || total > 256 || total != h.vals.len() {
        return Err(Error::parse(
            "jpeg: a Huffman table's symbol count is inconsistent",
        ));
    }
    let mut kraft: u64 = 0;
    for len in 1..=16usize {
        kraft += (h.counts[len] as u64) << (16 - len);
    }
    if kraft > 1u64 << 16 {
        return Err(Error::parse("jpeg: a Huffman table is overfull"));
    }
    let mut seen = [false; 256];
    for &v in &h.vals {
        if seen[v as usize] {
            return Err(Error::parse("jpeg: a Huffman table repeats a symbol"));
        }
        seen[v as usize] = true;
    }
    let mut code: u32 = 0;
    for len in 1..=16u32 {
        let count = h.counts[len as usize] as u32;
        if count > 0 && code + count - 1 == (1u32 << len) - 1 {
            return Err(Error::parse(
                "jpeg: a Huffman table uses the forbidden all-ones code",
            ));
        }
        code = (code + count) << 1;
    }
    Ok(())
}

/// Canonical codes for encoding, indexed by symbol.
struct Enc {
    codes: [u32; 256],
    lens: [u8; 256],
}

impl Enc {
    #[allow(clippy::needless_range_loop)]
    fn build(h: &Huff) -> Result<Enc> {
        validate_table(h)?;
        let mut enc = Enc {
            codes: [0u32; 256],
            lens: [0u8; 256],
        };
        let mut code = 0u32;
        let mut k = 0usize;
        for len in 1..=16u32 {
            for _ in 0..h.counts[len as usize] {
                let symbol = h.vals[k] as usize;
                enc.codes[symbol] = code;
                enc.lens[symbol] = len as u8;
                code += 1;
                k += 1;
            }
            code <<= 1;
        }
        Ok(enc)
    }
}

/// Canonical decoding information, built once per table.
struct HuffDec {
    min: [i32; 17],
    max: [i32; 17],
    valptr: [usize; 17],
    vals: Vec<u8>,
}

impl HuffDec {
    #[allow(clippy::needless_range_loop)]
    fn build(h: &Huff) -> Result<HuffDec> {
        validate_table(h)?;
        let mut dec = HuffDec {
            min: [-1i32; 17],
            max: [-1i32; 17],
            valptr: [0usize; 17],
            vals: h.vals.clone(),
        };
        let mut code = 0i32;
        let mut k = 0usize;
        for len in 1..=16usize {
            let count = h.counts[len] as i32;
            dec.valptr[len] = k;
            if count > 0 {
                dec.min[len] = code;
                dec.max[len] = code + count - 1;
                code += count;
                k += count as usize;
            }
            code <<= 1;
        }
        Ok(dec)
    }

    fn decode(&self, r: &mut Reader<'_>) -> Result<u8> {
        let mut code = 0i32;
        for len in 1..=16usize {
            code = (code << 1) | r.get_bits(1)? as i32;
            if code <= self.max[len] {
                let index = self.valptr[len] + (code - self.min[len]) as usize;
                return Ok(self.vals[index]);
            }
        }
        Err(Error::parse(
            "jpeg: the scan data contains a code that is not in the Huffman table",
        ))
    }
}

/// Builds a table from per-symbol code lengths, checking that the result is
/// legal before returning it.
#[allow(clippy::needless_range_loop)]
fn table_from_lengths(vals: &[u8], lengths: &[u8]) -> Result<Huff> {
    if vals.is_empty() || vals.len() != lengths.len() {
        return Err(Error::internal("jpeg: table lengths do not match the symbol list"));
    }
    let mut order: Vec<usize> = (0..vals.len()).collect();
    order.sort_by(|&a, &b| lengths[a].cmp(&lengths[b]).then(vals[a].cmp(&vals[b])));
    let mut counts = [0u32; 17];
    for &i in &order {
        let len = lengths[i] as usize;
        if len == 0 || len > 16 {
            return Err(Error::internal("jpeg: a generated code length is out of range"));
        }
        counts[len] += 1;
    }
    let mut huff = Huff {
        counts: [0u8; 17],
        vals: order.iter().map(|&i| vals[i]).collect(),
    };
    for len in 1..=16usize {
        if counts[len] > 255 {
            return Err(Error::internal(
                "jpeg: too many codes share one length in a generated table",
            ));
        }
        huff.counts[len] = counts[len] as u8;
    }
    validate_table(&huff)?;
    Ok(huff)
}

/// A legal table that cannot be worse than no table at all, used only if the
/// optimal construction fails its own validation (it should never happen).
fn flat_table(counts: &[u64; 256]) -> Huff {
    let vals: Vec<u8> = (0..256)
        .filter(|&s| counts[s] > 0)
        .map(|s| s as u8)
        .collect();
    let n = vals.len().max(1);
    let mut len = 1usize;
    while (1usize << len) <= n {
        len += 1;
    }
    let mut out = [0u8; 17];
    let mut remaining = n;
    let mut level = len;
    while remaining > 0 && level <= 16 {
        let take = remaining.min(255);
        out[level] = take as u8;
        remaining -= take;
        level += 1;
    }
    Huff {
        counts: out,
        vals,
    }
}

/// Optimal Huffman table for the observed symbol frequencies (Annex K.2),
/// with code lengths limited to 16 bits and the all-ones code excluded.
///
/// The construction adds one dummy symbol so that the tree is never degenerate;
/// the dummy always receives one of the longest codes and is dropped before the
/// table is written, which leaves a legal, slightly incomplete table.
#[allow(clippy::needless_range_loop)]
fn optimal(counts: &[u64; 256]) -> Huff {
    if counts.iter().all(|&c| c == 0) {
        return flat_table(counts);
    }

    let mut freq = [0u64; 257];
    freq[..256].copy_from_slice(counts);
    freq[256] = 1;
    let mut codesize = [0i32; 257];
    let mut others = [-1i32; 257];
    for i in 0..=256usize {
        codesize[i] = if freq[i] > 0 { 0 } else { -1 };
    }

    loop {
        let mut c1 = usize::MAX;
        let mut best = u64::MAX;
        for i in 0..=256usize {
            if freq[i] > 0 && freq[i] < best {
                best = freq[i];
                c1 = i;
            }
        }
        let mut c2 = usize::MAX;
        let mut best2 = u64::MAX;
        for i in 0..=256usize {
            if i != c1 && freq[i] > 0 && freq[i] < best2 {
                best2 = freq[i];
                c2 = i;
            }
        }
        if c2 == usize::MAX {
            break;
        }
        freq[c1] += freq[c2];
        freq[c2] = 0;
        let mut k = c1;
        loop {
            codesize[k] += 1;
            let next = others[k];
            if next < 0 {
                break;
            }
            k = next as usize;
        }
        others[k] = c2 as i32;
        let mut k = c2;
        loop {
            codesize[k] += 1;
            let next = others[k];
            if next < 0 {
                break;
            }
            k = next as usize;
        }
    }

    let mut bits = [0u32; 257];
    for &size in codesize.iter() {
        if size > 0 {
            bits[size as usize] += 1;
        }
    }

    // Limit the code lengths to 16 bits by moving the deepest codes upwards.
    let mut limited = true;
    for i in (17..=256usize).rev() {
        while bits[i] > 0 {
            if bits[i] < 2 {
                limited = false;
                break;
            }
            let mut j = i - 2;
            while j > 0 && bits[j] == 0 {
                j -= 1;
            }
            if j == 0 {
                limited = false;
                break;
            }
            bits[i] -= 2;
            bits[i - 1] += 1;
            bits[j + 1] += 2;
            bits[j] -= 1;
        }
        if !limited {
            break;
        }
    }
    if !limited {
        return flat_table(counts);
    }

    let mut symbols: Vec<(u64, usize)> = (0..256)
        .filter(|&s| counts[s] > 0)
        .map(|s| (counts[s], s))
        .collect();
    symbols.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut lengths: Vec<u8> = Vec::with_capacity(symbols.len() + 1);
    for len in 1..=256usize {
        for _ in 0..bits[len] {
            lengths.push(len as u8);
        }
    }
    while lengths.len() > symbols.len() {
        lengths.pop();
    }
    if lengths.len() != symbols.len() {
        return flat_table(counts);
    }
    let vals: Vec<u8> = symbols.iter().map(|&(_, s)| s as u8).collect();

    // If the longest code is the forbidden all-ones pattern, give the least
    // frequent symbol one more bit. The table stays legal and effectively
    // optimal to within a single bit.
    for _ in 0..4 {
        if let Ok(huff) = table_from_lengths(&vals, &lengths) {
            return huff;
        }
        match lengths.last_mut() {
            Some(last) if *last < 16 => *last += 1,
            _ => break,
        }
    }
    flat_table(counts)
}

// ---------------------------------------------------------------------------
// Bit I/O
// ---------------------------------------------------------------------------

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    bits: u32,
    /// Set when the reader runs into a marker instead of a data byte.
    marker: Option<u8>,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Reader<'a> {
        Reader {
            data,
            pos: 0,
            buf: 0,
            bits: 0,
            marker: None,
        }
    }

    fn next_data_byte(&mut self) -> Option<u8> {
        if self.pos >= self.data.len() {
            return None;
        }
        let byte = self.data[self.pos];
        if byte != 0xff {
            self.pos += 1;
            return Some(byte);
        }
        if self.pos + 1 >= self.data.len() {
            self.pos = self.data.len();
            return None;
        }
        let next = self.data[self.pos + 1];
        if next == 0x00 {
            self.pos += 2;
            return Some(0xff);
        }
        self.marker = Some(next);
        None
    }

    fn fill(&mut self, want: u32) {
        while self.bits < want && self.marker.is_none() {
            match self.next_data_byte() {
                Some(byte) => {
                    self.buf = (self.buf << 8) | byte as u32;
                    self.bits += 8;
                }
                None => break,
            }
        }
    }

    fn get_bits(&mut self, want: u32) -> Result<u32> {
        if want == 0 {
            return Ok(0);
        }
        debug_assert!(want <= 16);
        self.fill(want);
        if self.bits < want {
            return Err(Error::parse(
                "jpeg: the entropy-coded data ends in the middle of a block",
            ));
        }
        let shift = self.bits - want;
        let value = (self.buf >> shift) & ((1u32 << want) - 1);
        self.bits -= want;
        self.buf &= (1u32 << self.bits) - 1;
        Ok(value)
    }

    fn align_to_byte(&mut self) {
        self.bits = 0;
        self.buf = 0;
    }

    fn expect_restart(&mut self, index: u8) -> Result<()> {
        if self.marker.is_none() {
            // Probing consumes the marker prefix if one is there; if a data
            // byte comes back instead, the stream is not at an interval bound.
            if self.next_data_byte().is_some() {
                return Err(Error::parse(
                    "jpeg: the scan continues past a restart interval boundary",
                ));
            }
        }
        match self.marker.take() {
            Some(m) if m == 0xd0u8.wrapping_add(index) => Ok(()),
            Some(m) => Err(Error::parse(format!(
                "jpeg: expected restart marker RST{index}, found marker 0x{m:02X}"
            ))),
            None => Err(Error::parse("jpeg: a restart marker is missing")),
        }
    }
}

struct Writer {
    out: Vec<u8>,
    buf: u32,
    bits: u32,
}

impl Writer {
    fn new() -> Writer {
        Writer {
            out: Vec::new(),
            buf: 0,
            bits: 0,
        }
    }

    fn emit(&mut self, byte: u8) {
        self.out.push(byte);
        if byte == 0xff {
            self.out.push(0x00);
        }
    }

    fn put(&mut self, value: u32, width: u32) {
        if width == 0 {
            return;
        }
        self.buf = (self.buf << width) | (value & ((1u32 << width) - 1));
        self.bits += width;
        while self.bits >= 8 {
            let byte = ((self.buf >> (self.bits - 8)) & 0xff) as u8;
            self.emit(byte);
            self.bits -= 8;
        }
        if self.bits == 0 {
            self.buf = 0;
        } else {
            self.buf &= (1u32 << self.bits) - 1;
        }
    }

    fn restart(&mut self, index: u8) {
        self.pad_with_ones();
        self.out.push(0xff);
        self.out.push(0xd0u8.wrapping_add(index));
    }

    fn pad_with_ones(&mut self) {
        if self.bits > 0 {
            let pad = 8 - self.bits;
            self.put((1u32 << pad) - 1, pad);
        }
    }
}

/// `(size, bits)` of one coefficient, as the standard defines the pair.
fn magnitude(value: i32) -> (u32, u32) {
    let size = 32 - value.unsigned_abs().leading_zeros();
    let bits = if value < 0 {
        ((value - 1) as u32) & ((1u32 << size) - 1)
    } else {
        value as u32
    };
    (size, bits)
}

fn receive(r: &mut Reader<'_>, size: u32) -> Result<i32> {
    if size == 0 {
        return Ok(0);
    }
    let raw = r.get_bits(size)? as i32;
    let half = 1i32 << (size - 1);
    Ok(if raw < half {
        raw - (1i32 << size) + 1
    } else {
        raw
    })
}

fn to_coefficient(value: i32) -> Result<i16> {
    if value < i16::MIN as i32 || value > i16::MAX as i32 {
        return Err(Error::parse(
            "jpeg: a decoded coefficient does not fit the 16-bit range",
        ));
    }
    Ok(value as i16)
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

struct Layout {
    frame: Frame,
    scan: Scan,
    /// Effective tables before the scan, indexed by `class * 4 + id`.
    tables: [Option<Huff>; 8],
    /// Every table declared before the scan, in first-definition order.
    declared: Vec<(TableKey, Huff)>,
    used: Vec<TableKey>,
    restart: u32,
    sos_index: usize,
}

fn analyze(image: &Image) -> Result<Layout> {
    let mut frame: Option<Frame> = None;
    let mut restart = 0u32;
    let mut declared: Vec<(TableKey, Huff)> = Vec::new();
    let mut sos_index: Option<usize> = None;

    for (index, segment) in image.segments.iter().enumerate() {
        if sos_index.is_some() && segment.marker == DHT {
            // A table defined after the only scan is not used by anything;
            // it is preserved verbatim and does not take part in the rewrite.
            continue;
        }
        match segment.marker {
            SOF0 => {
                if frame.is_some() {
                    return Err(Error::parse("jpeg: more than one frame header"));
                }
                frame = Some(parse_sof(&segment.payload)?);
            }
            0xc1..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => {
                return Err(Error::refused(format!(
                    "jpeg: only baseline sequential (SOF0) is supported; this file uses SOF{}",
                    segment.marker - 0xc0
                )))
            }
            DAC => {
                return Err(Error::refused(
                    "jpeg: arithmetic-coded files are not supported",
                ))
            }
            DNL => {
                return Err(Error::refused(
                    "jpeg: DNL (height defined after the scan) is not supported",
                ))
            }
            DHT => {
                for (key, table) in parse_dht(&segment.payload)? {
                    match declared.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = table,
                        None => declared.push((key, table)),
                    }
                }
            }
            DRI => {
                if segment.payload.len() != 2 {
                    return Err(Error::parse("jpeg: the DRI segment has the wrong length"));
                }
                restart = u16::from_be_bytes([segment.payload[0], segment.payload[1]]) as u32;
            }
            SOS => {
                if sos_index.is_some() {
                    return Err(Error::refused(
                        "jpeg: files with more than one scan are not supported (baseline only)",
                    ));
                }
                sos_index = Some(index);
            }
            _ => {}
        }
    }

    let frame = frame.ok_or_else(|| Error::parse("jpeg: no frame header (SOF0) was found"))?;
    let sos_index = sos_index.ok_or_else(|| Error::parse("jpeg: no scan was found"))?;
    let sos = image
        .segments
        .get(sos_index)
        .ok_or_else(|| Error::internal("jpeg: scan index out of range"))?;
    let scan = parse_sos(&sos.payload, &frame)?;
    if sos.entropy.is_none() {
        return Err(Error::parse("jpeg: the scan carries no entropy-coded data"));
    }

    let mut tables: [Option<Huff>; 8] = std::array::from_fn(|_| None);
    for (key, table) in &declared {
        tables[(key.0 as usize) * 4 + key.1 as usize] = Some(table.clone());
    }

    let mut used: Vec<TableKey> = Vec::new();
    for component in &scan.comps {
        for key in [(0u8, component.dc), (1u8, component.ac)] {
            if tables[(key.0 as usize) * 4 + key.1 as usize].is_none() {
                return Err(Error::parse(format!(
                    "jpeg: the scan uses {}-table {} which is not defined",
                    if key.0 == 0 { "DC" } else { "AC" },
                    key.1
                )));
            }
            if !used.contains(&key) {
                used.push(key);
            }
        }
    }

    Ok(Layout {
        frame,
        scan,
        tables,
        declared,
        used,
        restart,
        sos_index,
    })
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

struct Stats {
    dc: Vec<[u64; 256]>,
    ac: Vec<[u64; 256]>,
}

struct Decoded {
    /// Per component, `bw * bh * 64` coefficients in zig-zag order.
    planes: Vec<Vec<i16>>,
    stats: Stats,
}

#[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
fn decode_block(
    r: &mut Reader<'_>,
    dc: &HuffDec,
    ac: &HuffDec,
    dc_stats: &mut [u64; 256],
    ac_stats: &mut [u64; 256],
    predict: &mut i32,
    out: &mut [i16],
) -> Result<()> {
    let size = dc.decode(r)? as u32;
    if size > 15 {
        return Err(Error::parse("jpeg: a DC category is out of range"));
    }
    dc_stats[size as usize] += 1;
    *predict += receive(r, size)?;
    out[0] = to_coefficient(*predict)?;

    let mut k = 1usize;
    while k < 64 {
        let symbol = ac.decode(r)?;
        ac_stats[symbol as usize] += 1;
        let run = (symbol >> 4) as usize;
        let size = (symbol & 0x0f) as u32;
        if size == 0 {
            if run == 15 {
                k += 16;
                continue;
            }
            break;
        }
        k += run;
        if k >= 64 {
            return Err(Error::parse(
                "jpeg: an AC zero-run runs past the end of the block",
            ));
        }
        out[k] = to_coefficient(receive(r, size)?)?;
        k += 1;
    }
    Ok(())
}

fn decode(image: &Image, layout: &Layout) -> Result<Decoded> {
    let frame = &layout.frame;
    let scan = &layout.scan;
    let (mcux, mcuy) = frame.mcu_grid();

    let mut planes = Vec::with_capacity(frame.comps.len());
    let mut coefficient_bytes: u64 = 0;
    for component in &frame.comps {
        let width = mcux
            .checked_mul(component.h)
            .ok_or_else(|| Error::limit("jpeg: the image dimensions overflow"))?;
        let height = mcuy
            .checked_mul(component.v)
            .ok_or_else(|| Error::limit("jpeg: the image dimensions overflow"))?;
        let count = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(64))
            .ok_or_else(|| Error::limit("jpeg: the image dimensions overflow"))?;
        coefficient_bytes += (count as u64) * 2;
        if coefficient_bytes > MAX_COEFFICIENT_BYTES {
            return Err(Error::limit(
                "jpeg: the coefficient arrays would exceed 256 MiB",
            ));
        }
        planes.push(vec![0i16; count]);
    }

    let mut dec_dc: [Option<HuffDec>; 4] = std::array::from_fn(|_| None);
    let mut dec_ac: [Option<HuffDec>; 4] = std::array::from_fn(|_| None);
    for component in &scan.comps {
        let dc_id = component.dc as usize;
        if dec_dc[dc_id].is_none() {
            let table = layout.tables[dc_id]
                .as_ref()
                .ok_or_else(|| Error::parse("jpeg: a DC table is not defined"))?;
            dec_dc[dc_id] = Some(HuffDec::build(table)?);
        }
        let ac_id = component.ac as usize;
        if dec_ac[ac_id].is_none() {
            let table = layout.tables[4 + ac_id]
                .as_ref()
                .ok_or_else(|| Error::parse("jpeg: an AC table is not defined"))?;
            dec_ac[ac_id] = Some(HuffDec::build(table)?);
        }
    }

    let entropy = image
        .segments
        .get(layout.sos_index)
        .and_then(|s| s.entropy.as_deref())
        .ok_or_else(|| Error::parse("jpeg: the scan carries no data"))?;
    let mut reader = Reader::new(entropy);
    let mut predicts = vec![0i32; frame.comps.len()];
    let mut stats = Stats {
        dc: vec![[0u64; 256]; 4],
        ac: vec![[0u64; 256]; 4],
    };
    let restart = layout.restart;
    let mut restart_index = 0u8;

    for mcu in 0..mcux * mcuy {
        if restart != 0 && mcu > 0 && mcu % restart as usize == 0 {
            reader.align_to_byte();
            reader.expect_restart(restart_index)?;
            restart_index = (restart_index + 1) & 7;
            for predict in predicts.iter_mut() {
                *predict = 0;
            }
        }
        let mcu_x = mcu % mcux;
        let mcu_y = mcu / mcux;
        for component in &scan.comps {
            let index = frame
                .index_of(component.id)
                .ok_or_else(|| Error::parse("jpeg: the scan references an unknown component"))?;
            let info = &frame.comps[index];
            let plane_width = mcux * info.h;
            let dc_table = dec_dc[component.dc as usize]
                .as_ref()
                .ok_or_else(|| Error::internal("jpeg: a DC table was not built"))?;
            let ac_table = dec_ac[component.ac as usize]
                .as_ref()
                .ok_or_else(|| Error::internal("jpeg: an AC table was not built"))?;
            for by in 0..info.v {
                for bx in 0..info.h {
                    let block_x = mcu_x * info.h + bx;
                    let block_y = mcu_y * info.v + by;
                    let offset = (block_y * plane_width + block_x) * 64;
                    let (dc_stats, ac_stats) = (
                        &mut stats.dc[component.dc as usize],
                        &mut stats.ac[component.ac as usize],
                    );
                    let block = stats_block(&mut planes[index], offset)?;
                    decode_block(
                        &mut reader,
                        dc_table,
                        ac_table,
                        dc_stats,
                        ac_stats,
                        &mut predicts[index],
                        block,
                    )?;
                }
            }
        }
    }

    // Nothing but padding and, at most, a trailing restart marker may follow
    // the last block. Anything else means the decoder and the file disagree
    // about the image, which is a corruption we must not silently truncate.
    reader.align_to_byte();
    if reader.pos < reader.data.len() && reader.data[reader.pos] != 0xff {
        return Err(Error::parse(
            "jpeg: the scan contains data after the last block",
        ));
    }

    Ok(Decoded { planes, stats })
}

fn stats_block(plane: &mut [i16], offset: usize) -> Result<&mut [i16]> {
    plane
        .get_mut(offset..offset + 64)
        .ok_or_else(|| Error::internal("jpeg: block offset outside the coefficient plane"))
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

#[allow(clippy::needless_range_loop)]
fn encode_block(w: &mut Writer, dc: &Enc, ac: &Enc, predict: &mut i32, block: &[i16]) {
    let value = block[0] as i32;
    let difference = value - *predict;
    *predict = value;
    let (size, bits) = magnitude(difference);
    w.put(dc.codes[size as usize], dc.lens[size as usize] as u32);
    w.put(bits, size);

    let mut run = 0usize;
    for k in 1..64 {
        let coefficient = block[k];
        if coefficient == 0 {
            run += 1;
            continue;
        }
        while run > 15 {
            w.put(ac.codes[0xf0], ac.lens[0xf0] as u32);
            run -= 16;
        }
        let (size, bits) = magnitude(coefficient as i32);
        let symbol = (run << 4) | size as usize;
        w.put(ac.codes[symbol], ac.lens[symbol] as u32);
        w.put(bits, size);
        run = 0;
    }
    if run > 0 {
        w.put(ac.codes[0], ac.lens[0] as u32);
    }
}

fn encode_scan(
    decoded: &Decoded,
    layout: &Layout,
    tables: &BTreeMap<TableKey, Huff>,
) -> Result<Vec<u8>> {
    let frame = &layout.frame;
    let scan = &layout.scan;
    let (mcux, mcuy) = frame.mcu_grid();

    let mut enc_dc: [Option<Enc>; 4] = std::array::from_fn(|_| None);
    let mut enc_ac: [Option<Enc>; 4] = std::array::from_fn(|_| None);
    for component in &scan.comps {
        let dc_id = component.dc as usize;
        if enc_dc[dc_id].is_none() {
            let table = tables
                .get(&(0, component.dc))
                .ok_or_else(|| Error::internal("jpeg: an optimized DC table is missing"))?;
            enc_dc[dc_id] = Some(Enc::build(table)?);
        }
        let ac_id = component.ac as usize;
        if enc_ac[ac_id].is_none() {
            let table = tables
                .get(&(1, component.ac))
                .ok_or_else(|| Error::internal("jpeg: an optimized AC table is missing"))?;
            enc_ac[ac_id] = Some(Enc::build(table)?);
        }
    }

    let mut writer = Writer::new();
    let mut predicts = vec![0i32; frame.comps.len()];
    let restart = layout.restart;
    let mut restart_index = 0u8;

    for mcu in 0..mcux * mcuy {
        if restart != 0 && mcu > 0 && mcu % restart as usize == 0 {
            writer.restart(restart_index);
            restart_index = (restart_index + 1) & 7;
            for predict in predicts.iter_mut() {
                *predict = 0;
            }
        }
        let mcu_x = mcu % mcux;
        let mcu_y = mcu / mcux;
        for component in &scan.comps {
            let index = frame
                .index_of(component.id)
                .ok_or_else(|| Error::internal("jpeg: the scan references an unknown component"))?;
            let info = &frame.comps[index];
            let plane_width = mcux * info.h;
            let dc_table = enc_dc[component.dc as usize]
                .as_ref()
                .ok_or_else(|| Error::internal("jpeg: a DC encoder table was not built"))?;
            let ac_table = enc_ac[component.ac as usize]
                .as_ref()
                .ok_or_else(|| Error::internal("jpeg: an AC encoder table was not built"))?;
            for by in 0..info.v {
                for bx in 0..info.h {
                    let block_x = mcu_x * info.h + bx;
                    let block_y = mcu_y * info.v + by;
                    let offset = (block_y * plane_width + block_x) * 64;
                    let block = decoded.planes[index]
                        .get(offset..offset + 64)
                        .ok_or_else(|| Error::internal("jpeg: block offset outside the plane"))?;
                    encode_block(
                        &mut writer,
                        dc_table,
                        ac_table,
                        &mut predicts[index],
                        block,
                    );
                }
            }
        }
    }
    writer.pad_with_ones();
    Ok(writer.out)
}

// ---------------------------------------------------------------------------
// Reassembling the file
// ---------------------------------------------------------------------------

fn write_segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) -> Result<()> {
    let length = payload.len() + 2;
    if length > u16::MAX as usize {
        return Err(Error::internal("jpeg: a marker segment became too long"));
    }
    out.push(0xff);
    out.push(marker);
    out.extend_from_slice(&(length as u16).to_be_bytes());
    out.extend_from_slice(payload);
    Ok(())
}

fn write_table(payload: &mut Vec<u8>, key: TableKey, table: &Huff) {
    payload.push((key.0 << 4) | key.1);
    payload.extend_from_slice(&table.counts[1..17]);
    payload.extend_from_slice(&table.vals);
}

fn assemble(
    image: &Image,
    layout: &Layout,
    optimized: &BTreeMap<TableKey, Huff>,
    entropy: &[u8],
) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(entropy.len() + 1024);
    out.push(0xff);
    out.push(SOI);

    let mut merged = false;
    let mut seen_scan = false;
    for segment in &image.segments {
        if segment.marker == SOS {
            seen_scan = true;
            write_segment(&mut out, SOS, &segment.payload)?;
            out.extend_from_slice(entropy);
            continue;
        }
        if segment.marker == DHT && !seen_scan {
            if merged {
                continue;
            }
            // All tables declared before the scan are emitted together, with
            // the ones the scan uses replaced by their optimized versions.
            let mut payload = Vec::new();
            for (key, table) in &layout.declared {
                let replacement = optimized.get(key).unwrap_or(table);
                write_table(&mut payload, *key, replacement);
            }
            write_segment(&mut out, DHT, &payload)?;
            merged = true;
            continue;
        }
        write_segment(&mut out, segment.marker, &segment.payload)?;
    }

    out.push(0xff);
    out.push(EOI);
    out.extend_from_slice(&image.trailing);
    Ok(out)
}

fn marker_sequence(image: &Image) -> Vec<(u8, &[u8])> {
    image
        .segments
        .iter()
        .filter(|s| s.marker != DHT)
        .map(|s| (s.marker, s.payload.as_slice()))
        .collect()
}

fn build_tables(layout: &Layout, stats: &Stats) -> BTreeMap<TableKey, Huff> {
    let mut tables = BTreeMap::new();
    for &(class, id) in &layout.used {
        let counts = if class == 0 {
            &stats.dc[id as usize]
        } else {
            &stats.ac[id as usize]
        };
        tables.insert((class, id), optimal(counts));
    }
    tables
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

pub fn optimize(input: &[u8], dst: &mut dyn Write, opts: &Options) -> Result<Cand> {
    let image = parse(input)?;
    let layout = analyze(&image)?;

    let pixels = (layout.frame.width as u64) * (layout.frame.height as u64);
    if pixels > opts.max_pixels {
        return Err(Error::limit(format!(
            "jpeg: {pixels} pixels exceeds --max-pixels ({})",
            opts.max_pixels
        )));
    }

    let decoded = decode(&image, &layout)?;
    let tables = build_tables(&layout, &decoded.stats);
    let entropy = encode_scan(&decoded, &layout, &tables)?;
    let candidate = assemble(&image, &layout, &tables, &entropy)?;

    if candidate.len() >= input.len() {
        return Ok(Cand::NoGain);
    }
    dst.write_all(&candidate)?;
    Ok(Cand::Written(candidate.len() as u64))
}

pub fn verify(original: &[u8], candidate: &[u8]) -> Result<()> {
    let before = parse(original)?;
    let after = parse(candidate)?;
    let layout_before = analyze(&before)?;
    let layout_after = analyze(&after)?;

    if layout_before.frame != layout_after.frame {
        return Err(Error::verify("jpeg: the frame header changed"));
    }
    if marker_sequence(&before) != marker_sequence(&after) {
        return Err(Error::verify(
            "jpeg: markers outside the Huffman tables changed",
        ));
    }
    if before.trailing != after.trailing {
        return Err(Error::verify("jpeg: the bytes after EOI changed"));
    }
    for (key, table) in &layout_before.declared {
        if layout_before.used.contains(key) {
            continue;
        }
        let other = layout_after
            .declared
            .iter()
            .find(|entry| &entry.0 == key)
            .map(|entry| &entry.1);
        if other != Some(table) {
            return Err(Error::verify(
                "jpeg: a Huffman table that the scan does not use changed",
            ));
        }
    }

    let decoded_before = decode(&before, &layout_before)?;
    let decoded_after = decode(&after, &layout_after)?;
    if decoded_before.planes != decoded_after.planes {
        return Err(Error::verify(
            "jpeg: DCT coefficients changed, the candidate is not lossless",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random source; no dev-dependency.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0 >> 33
        }
    }

    fn table_is_legal(h: &Huff) -> bool {
        validate_table(h).is_ok()
    }

    #[test]
    fn generated_tables_are_always_legal() {
        let mut rng = Lcg(0x5eed_1234);
        for round in 0..4000u64 {
            let alphabet = 1 + (rng.next() % 200) as usize;
            let mut counts = [0u64; 256];
            // A mixture of skewed and flat distributions, including the
            // degenerate case of a single used symbol.
            for slot in counts.iter_mut().take(alphabet) {
                *slot = 1 + rng.next() % (1 + round % 97);
            }
            let table = optimal(&counts);
            assert!(table_is_legal(&table), "round {round}: table is not legal");
            let total: usize = table.counts[1..].iter().map(|&c| c as usize).sum();
            assert_eq!(total, table.vals.len(), "round {round}: counts disagree");
            let mut seen = [false; 256];
            for &v in &table.vals {
                assert!(!seen[v as usize], "round {round}: duplicate symbol");
                seen[v as usize] = true;
            }
            for (symbol, present) in seen.iter().take(alphabet).enumerate() {
                assert!(*present, "round {round}: symbol {symbol} has no code");
            }
            // Every symbol that occurred must be encodable.
            let enc = Enc::build(&table).expect("encoder table");
            for (symbol, length) in enc.lens.iter().take(alphabet).enumerate() {
                assert!(*length > 0, "round {round}: symbol {symbol} is not encodable");
            }
        }
    }

    #[test]
    fn single_symbol_table_is_legal() {
        let mut counts = [0u64; 256];
        counts[0] = 1_000_000;
        let table = optimal(&counts);
        assert!(table_is_legal(&table));
        let enc = Enc::build(&table).expect("encoder table");
        assert!(enc.lens[0] > 0 && enc.lens[0] <= 16);
    }

    #[test]
    fn all_ones_code_is_rejected() {
        // Two one-bit codes: the second one is the forbidden all-ones pattern.
        let mut counts = [0u8; 17];
        counts[1] = 2;
        let table = Huff {
            counts,
            vals: vec![0, 1],
        };
        assert!(validate_table(&table).is_err());
    }

    #[test]
    fn overfull_table_is_rejected() {
        let mut counts = [0u8; 17];
        counts[1] = 3;
        let table = Huff {
            counts,
            vals: vec![0, 1, 2],
        };
        assert!(validate_table(&table).is_err());
    }

    #[test]
    fn magnitude_and_receive_agree() {
        for value in -2047i32..=2047 {
            let (size, bits) = magnitude(value);
            let mut writer = Writer::new();
            writer.put(bits, size);
            writer.pad_with_ones();
            let mut reader = Reader::new(&writer.out);
            let decoded = receive(&mut reader, size).expect("receive");
            assert_eq!(decoded, value, "value {value} did not survive the round trip");
        }
    }

    #[test]
    fn writer_stuffs_zero_bytes_after_ff() {
        let mut writer = Writer::new();
        writer.put(0xff, 8);
        writer.put(0x00, 8);
        assert_eq!(writer.out, vec![0xff, 0x00, 0x00]);
        let mut reader = Reader::new(&writer.out);
        assert_eq!(reader.get_bits(8).unwrap(), 0xff);
        assert_eq!(reader.get_bits(8).unwrap(), 0x00);
    }

    #[test]
    fn truncated_file_is_a_parse_error() {
        let data = [0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08];
        assert!(parse(&data).is_err());
    }

    #[test]
    fn missing_soi_is_a_parse_error() {
        assert!(parse(b"not a jpeg at all").is_err());
    }
}
