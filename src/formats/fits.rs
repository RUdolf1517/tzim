//! FITS, tile-compressed image HDUs — v0.14, opt-in behind `--fits-tile`.
//!
//! One tier ships, the one the measurement admitted
//! ([fits-audit §8](../../docs/fits-audit.md#8-the-measurement-and-the-rule-fixed-in-advance)):
//! an uncompressed image HDU is written as a tile-compressed `BINTABLE` under the
//! registered tile-compression convention — `ZIMAGE = T`, one `COMPRESSED_DATA`
//! column, every tile a gzip member in the table's heap. The other tier,
//! recompressing tiles that are already `GZIP_1`/`GZIP_2`, measured 0.00 % and is
//! not implemented: a compressed HDU is copied verbatim, which is what the
//! contract promises for every HDU this handler does not rewrite.
//!
//! Three properties make this format the strongest position in the program, and
//! all three are consequences of the standard rather than of this code:
//!
//! - **There is no pointer table.** Everything in a FITS file is found by walking
//!   2880-byte records in order, so a shorter HDU moves what follows it without
//!   anything needing to be rewritten — unlike TIFF and PDF.
//! - **The data is an array of numbers, not an image.** `BITPIX` says how wide a
//!   sample is and in which byte order; nothing is scaled, normalised or
//!   re-interpreted here, and an image that carries a non-identity
//!   `BSCALE`/`BZERO` is refused rather than scaled.
//! - **The tile payload carries its own checksum.** A `GZIP_1`/`GZIP_2` tile is a
//!   gzip member (RFC 1952), so `flate2` verifies CRC-32 and `ISIZE` on the way in
//!   and the round trip is a byte comparison; `NaN != NaN` never enters the
//!   argument because no arithmetic does.
//!
//! # What is written
//!
//! For each eligible image HDU: a `BINTABLE` with `NAXIS1 = 8` (one `P`/`Q`
//! descriptor), `NAXIS2` = the number of tiles, `PCOUNT` = the heap length, and
//! one row per tile. **The tiling rule is one tile per row of the first axis**
//! (`ZTILE1 = ZNAXIS1`, `ZTILEn = 1`): it is the tiling the reference writer
//! chooses, it bounds memory to one row, and it is stated here rather than
//! inferred by a reader. Integer arrays use `GZIP_1`, floating-point arrays use
//! `GZIP_2`, which shuffles each sample's bytes into decreasing order of
//! significance before deflating. Quantisation is disabled for both
//! (`ZNAME1 = 'NOISEBIT'`, `ZVAL1 = 0`), so the rewrite is lossless by
//! construction and not by argument.
//!
//! A compressed primary array needs an empty `PrimaryHDU` in front of it, because
//! a `BINTABLE` cannot be the first HDU; that is the shape `fpack` writes. It is
//! the one case where the HDU count changes, and [the contract](../../docs/guarantees.md#412-fits--tile-compressed-image-hdus-v014-opt-in)
//! says so in as many words.
//!
//! # What is refused
//!
//! Refusals are decisions, not gaps: `GROUPS = T`, `PCOUNT`/`GCOUNT` that are not
//! the identity, a zero-length axis, `BITPIX` outside this writer's 8/16/32/−32/−64
//! subset, an image rank above six, a `Z*` card on an image HDU, an image whose
//! `BSCALE`/`BZERO` is not the identity, tiles whose `ZCMPTYPE` is
//! `RICE_1`/`PLIO_1`/`HCOMPRESS_1` (this build has no codec for them, and
//! `HCOMPRESS_1` may already be lossy), and **an HDU carrying
//! `CHECKSUM`/`DATASUM`** — a stale checksum is a corrupted file by the format's
//! own definition, and this build does not recompute them yet (fits-audit §9,
//! question 3). Such an HDU is copied verbatim; the rest of the file is still
//! rewritten.
//!
//! `--strip-metadata` removes nothing here: every FITS card is either structure
//! or a record of the observation, and the convention has no decorative field.
//! The flag is accepted and the output is the same as without it, as for zlib.

use std::io::{Read, Write};

use crate::error::{Error, Result};

use super::{Cand, Options};

/// A FITS record: the unit every header and every data block is padded to.
const RECORD: usize = 2880;
/// A FITS card.
const CARD: usize = 80;

/// The standard's limit on the number of axes.
const MAX_NAXIS: u64 = 999;
/// How many records a single header may occupy before the file is refused. A
/// real header is one to three records; this bound is 11.8 MiB of cards, which
/// is far past anything a reader would accept and short of anything that costs
/// memory to walk.
const MAX_HEADER_RECORDS: usize = 4096;

const GZIP_1: &str = "GZIP_1";
const GZIP_2: &str = "GZIP_2";

/// Axis-rank ceiling for this writer, aligned with pinned CFITSIO's
/// `MAX_COMPRESS_DIM = 6` (`fitsio.h` at commit
/// `113781d8aa442cf8d3c27295d9084674b8b039f2`); the HEASARC gate enforces
/// independent read-back. The FITS parser still accepts ranks through 999.
const MAX_REWRITE_NAXIS: usize = 6;
/// Standard BITPIX values the FITS reader can parse.
const BITPIX_VALUES: [i64; 6] = [8, 16, 32, 64, -32, -64];
/// The narrower integer/float types accepted by this tile writer.
const REWRITE_BITPIX_VALUES: [i64; 5] = [8, 16, 32, -32, -64];

/// A standard, interoperable `P` descriptor stores both values as signed
/// 32-bit integers, so its exact largest non-negative count or heap offset is
/// `2^31 - 1 = 2,147,483,647` (FITS Standard v4.0). The pinned CFITSIO gate
/// source also warns when either field exceeds that value
/// (`utilities/fvrf_data.c` at CFITSIO commit
/// `113781d8aa442cf8d3c27295d9084674b8b039f2`). CFITSIO's optional unsigned
/// extension is intentionally not adopted. This handler writes `1P`, not `1Q`;
/// values beyond the interoperable limit are refused rather than widened.
const MAX_P_DESCRIPTOR_VALUE: u64 = i32::MAX as u64;

fn p_descriptor_value(value: u64, field: &str) -> Result<i32> {
    // This explicit conversion is shared by the tile length and heap offset so
    // neither can silently become a negative signed descriptor.
    i32::try_from(value).map_err(|_| {
        Error::limit(format!(
            "fits: 1P descriptor {field} exceeds {MAX_P_DESCRIPTOR_VALUE}"
        ))
    })
}

// ---------------------------------------------------------------------------
// Cards
// ---------------------------------------------------------------------------

/// One 80-character card, kept as bytes.
///
/// Cards are copied verbatim into the candidate, so the raw bytes are what is
/// stored; the accessors parse the value field on demand. Nothing is normalised:
/// a card that came in with a comment leaves with the same comment.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Card {
    raw: [u8; CARD],
}

impl Card {
    /// The keyword: columns 1–8, without the trailing spaces.
    fn keyword(&self) -> &str {
        let mut end = 8;
        while end > 0 && self.raw[end - 1] == b' ' {
            end -= 1;
        }
        std::str::from_utf8(&self.raw[..end]).unwrap_or("")
    }

    /// The value field: columns 11–30, right-trimmed.
    fn field(&self) -> &str {
        std::str::from_utf8(&self.raw[10..30])
            .unwrap_or("")
            .trim_end()
    }

    /// An integer value, or `None` if the card holds something else.
    fn int(&self) -> Option<i64> {
        let text = self.field().trim();
        if text.is_empty() {
            return None;
        }
        text.parse::<i64>().ok()
    }

    /// A logical value: `T` or `F`.
    fn truth(&self) -> Option<bool> {
        match self.field().trim() {
            "T" => Some(true),
            "F" => Some(false),
            _ => None,
        }
    }

    /// A character string value, without its quotes.
    ///
    /// A FITS string may extend past column 30. Scan the complete card value
    /// area and stop at its closing quote, before any trailing comment. A slash
    /// before that quote is part of the value, and doubled quotes are skipped
    /// while locating the terminator but kept in the returned slice.
    fn text(&self) -> Option<&str> {
        let value_area = std::str::from_utf8(&self.raw[10..]).ok()?.trim_start();
        let rest = value_area.strip_prefix('\'')?;
        let bytes = rest.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'\'' {
                if bytes.get(index + 1) == Some(&b'\'') {
                    index += 2;
                    continue;
                }
                return Some(&rest[..index]);
            }
            index += 1;
        }
        None
    }
}

/// Parses a FITS decimal or exponent value without converting it to `f64`.
/// The coefficient digits and decimal-point position are kept exact; a very
/// large exponent is saturated because it cannot make a nonzero coefficient
/// equal to either identity value, while a zero coefficient remains exactly 0.
fn decimal_parts(text: &str) -> Option<(bool, Vec<u8>, usize, i64)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (negative, text) = match text.as_bytes()[0] {
        b'+' => (false, &text[1..]),
        b'-' => (true, &text[1..]),
        _ => (false, text),
    };
    if text.is_empty() {
        return None;
    }
    let exponent_at = text
        .bytes()
        .position(|byte| matches!(byte, b'E' | b'e' | b'D' | b'd'));
    let (mantissa, exponent) = if let Some(at) = exponent_at {
        let exponent = &text[at + 1..];
        if exponent.is_empty() {
            return None;
        }
        let (exponent_negative, digits) = match exponent.as_bytes()[0] {
            b'+' => (false, &exponent[1..]),
            b'-' => (true, &exponent[1..]),
            _ => (false, exponent),
        };
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let magnitude = digits.bytes().fold(0u64, |value, byte| {
            value
                .saturating_mul(10)
                .saturating_add(u64::from(byte - b'0'))
        });
        let exponent = if exponent_negative {
            if magnitude > i64::MAX as u64 {
                i64::MIN
            } else {
                -(magnitude as i64)
            }
        } else {
            magnitude.min(i64::MAX as u64) as i64
        };
        (&text[..at], exponent)
    } else {
        (text, 0)
    };

    let mut digits = Vec::new();
    let mut fractional_digits = 0usize;
    let mut after_point = false;
    for byte in mantissa.bytes() {
        match byte {
            b'.' if !after_point => after_point = true,
            b'0'..=b'9' => {
                digits.push(byte);
                if after_point {
                    fractional_digits += 1;
                }
            }
            _ => return None,
        }
    }
    if digits.is_empty() {
        return None;
    }
    Some((negative, digits, fractional_digits, exponent))
}

/// Whether a syntactically valid FITS number is exactly zero.
fn decimal_is_zero(text: &str) -> Option<bool> {
    let (_, digits, _, _) = decimal_parts(text)?;
    Some(digits.iter().all(|digit| *digit == b'0'))
}

/// Whether a syntactically valid FITS number is exactly one, with no rounding.
fn decimal_is_one(text: &str) -> Option<bool> {
    let (negative, digits, fractional_digits, exponent) = decimal_parts(text)?;
    if negative {
        return Some(false);
    }
    let Some(first) = digits.iter().position(|digit| *digit != b'0') else {
        return Some(false);
    };
    if digits[first] != b'1' || digits[first + 1..].iter().any(|digit| *digit != b'0') {
        return Some(false);
    }
    let fractional_digits = i64::try_from(fractional_digits).ok()?;
    let trailing_zeros = i64::try_from(digits.len() - first - 1).ok()?;
    let scale = exponent
        .checked_sub(fractional_digits)
        .and_then(|scale| scale.checked_add(trailing_zeros));
    Some(scale == Some(0))
}

/// An 80-byte card, split out so that a malformed record is a parse failure
/// rather than a slice panic.
fn card_at(record: &[u8], offset: usize) -> Result<Card> {
    let mut raw = [0u8; CARD];
    raw.copy_from_slice(&record[offset..offset + CARD]);
    Ok(Card { raw })
}

/// A numeric card: the value is right-justified in columns 11–30.
fn numeric_card(keyword: &str, value: i64, comment: &str) -> [u8; CARD] {
    assemble(&format!("{keyword:<8}= {value:>20}"), comment)
}

/// A logical card.
fn boolean_card(keyword: &str, value: bool, comment: &str) -> [u8; CARD] {
    let value = if value { "T" } else { "F" };
    assemble(&format!("{keyword:<8}= {value:>20}"), comment)
}

/// A character-string card. The quotes are written immediately around the value
/// and the rest of the twenty columns are left blank, which is the form the
/// reference writer produces.
fn string_card(keyword: &str, value: &str, comment: &str) -> [u8; CARD] {
    assemble(&format!("{keyword:<8}= '{value}'"), comment)
}

/// Places `head` at column 1 and `comment` after it, padding with spaces.
///
/// The comment starts at column 31, never earlier: columns 11–30 are the value
/// field, and a slash inside it would be read as part of the value. A card whose
/// head is shorter than 30 columns is padded out to the field width first, which
/// is the one formatting rule this writer has to get exactly right.
fn assemble(head: &str, comment: &str) -> [u8; CARD] {
    let mut out = [b' '; CARD];
    let head = head.as_bytes();
    let used = head.len().min(CARD);
    out[..used].copy_from_slice(&head[..used]);
    if !comment.is_empty() {
        let start = used.clamp(30, CARD);
        let text = format!(" / {comment}");
        let bytes = text.as_bytes();
        let take = bytes.len().min(CARD - start);
        out[start..start + take].copy_from_slice(&bytes[..take]);
    }
    out
}

// ---------------------------------------------------------------------------
// Headers and HDUs
// ---------------------------------------------------------------------------

/// A parsed header: its cards, where it starts and how long it is.
struct Header {
    cards: Vec<Card>,
    start: usize,
    bytes: usize,
}

impl Header {
    fn get(&self, keyword: &str) -> Option<&Card> {
        self.cards.iter().find(|c| c.keyword() == keyword)
    }

    fn has(&self, keyword: &str) -> bool {
        self.get(keyword).is_some()
    }

    fn count(&self, keyword: &str) -> usize {
        self.cards.iter().filter(|c| c.keyword() == keyword).count()
    }

    fn int(&self, keyword: &str) -> Option<i64> {
        self.get(keyword)?.int()
    }

    fn truth(&self, keyword: &str) -> Option<bool> {
        self.get(keyword)?.truth()
    }

    fn text(&self, keyword: &str) -> Option<&str> {
        self.get(keyword)?.text()
    }
}

/// What an HDU is, as far as the walk is concerned.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Primary,
    ImageExt,
    BinTable,
    AsciiTable,
    /// Anything else, including `HDRLET` and other non-standard extensions. It
    /// is copied verbatim: a handler that drops an unknown extension is
    /// destroying a file while reporting success.
    Other,
}

/// One HDU: its header, where its data sits, and what kind it is.
struct Hdu {
    header: Header,
    kind: Kind,
    bitpix: i64,
    axes: Vec<u64>,
    data_start: usize,
    data_len: u64,
    /// Offset of the next HDU: the data padded to a whole record.
    end: usize,
}

/// Parses one header at `start`, stopping after the `END` card.
fn parse_header(data: &[u8], start: usize) -> Result<Header> {
    let mut cards: Vec<Card> = Vec::new();
    let mut offset = start;
    let mut records = 0usize;
    loop {
        if offset + RECORD > data.len() {
            return Err(Error::parse(
                "fits: a header record runs past the end of the file",
            ));
        }
        if records >= MAX_HEADER_RECORDS {
            return Err(Error::limit("fits: header is longer than 4096 records"));
        }
        let record = &data[offset..offset + RECORD];
        records += 1;
        for within in (0..RECORD).step_by(CARD) {
            let card = card_at(record, within)?;
            if card.keyword() == "END" {
                // Everything after END in this record must be blank, and the
                // header ends here.
                let rest = &record[within + CARD..];
                if rest.iter().any(|b| *b != b' ') {
                    return Err(Error::parse("fits: bytes after the END card"));
                }
                return Ok(Header {
                    cards,
                    start,
                    bytes: offset + RECORD - start,
                });
            }
            cards.push(card);
        }
        offset += RECORD;
    }
}

/// The number of bytes of data an HDU holds, from its own cards.
///
/// The declared data length is `|BITPIX|/8 × GCOUNT × (PCOUNT + Π axes)`.
/// Random groups are the one special case: `NAXIS1` is the required zero
/// placeholder and the product starts at `NAXIS2`, with `PCOUNT` added for each
/// group. For a `BINTABLE`, the product is the table and `PCOUNT` is the heap;
/// for any extension this handler does not know, the cards are still the only
/// length the file itself declares.
fn declared_data_len(header: &Header) -> Result<(i64, Vec<u64>, u64)> {
    let bitpix = header
        .int("BITPIX")
        .ok_or_else(|| Error::parse("fits: header has no BITPIX card"))?;
    if !BITPIX_VALUES.contains(&bitpix) {
        return Err(Error::refused(format!(
            "fits: BITPIX {bitpix} is not one of 8, 16, 32, 64, -32 or -64"
        )));
    }
    let naxis = header
        .int("NAXIS")
        .ok_or_else(|| Error::parse("fits: header has no NAXIS card"))?;
    if naxis < 0 {
        return Err(Error::parse("fits: NAXIS is negative"));
    }
    if naxis as u64 > MAX_NAXIS {
        return Err(Error::limit(format!(
            "fits: NAXIS = {naxis} is beyond the standard's limit of {MAX_NAXIS}"
        )));
    }
    let random_groups = header.truth("GROUPS") == Some(true);
    if random_groups {
        if !header.has("SIMPLE") {
            return Err(Error::parse(
                "fits: GROUPS = T is only valid in a primary HDU",
            ));
        }
        if naxis < 2 {
            return Err(Error::parse("fits: random groups require NAXIS >= 2"));
        }
        if !header.has("PCOUNT") || !header.has("GCOUNT") {
            return Err(Error::parse(
                "fits: random groups require PCOUNT and GCOUNT",
            ));
        }
    }
    let mut axes = Vec::with_capacity(naxis as usize);
    let mut product: u64 = 1;
    for axis in 1..=naxis {
        let keyword = format!("NAXIS{axis}");
        let length = header
            .int(&keyword)
            .ok_or_else(|| Error::parse(format!("fits: header has no {keyword} card")))?;
        if length < 0 {
            return Err(Error::parse(format!("fits: {keyword} is negative")));
        }
        let length = length as u64;
        if random_groups && axis == 1 && length != 0 {
            return Err(Error::parse("fits: random groups require NAXIS1 = 0"));
        }
        axes.push(length);
        if !random_groups || axis > 1 {
            product = product
                .checked_mul(length)
                .ok_or_else(|| Error::limit("fits: the array size overflows"))?;
        }
    }
    if naxis == 0 {
        product = 0;
        axes.clear();
    }
    let pcount = header.int("PCOUNT").unwrap_or(0);
    let gcount = header.int("GCOUNT").unwrap_or(1);
    if pcount < 0 || gcount <= 0 {
        return Err(Error::parse("fits: PCOUNT or GCOUNT is out of range"));
    }
    let per_sample = (bitpix.unsigned_abs()) / 8;
    let total = per_sample
        .checked_mul(
            product
                .checked_add(pcount as u64)
                .ok_or_else(|| Error::limit("fits: the data length overflows"))?,
        )
        .and_then(|bytes| bytes.checked_mul(gcount as u64))
        .ok_or_else(|| Error::limit("fits: the data length overflows"))?;
    Ok((bitpix, axes, total))
}

/// Classifies an HDU by the cards every reader looks at.
fn classify(header: &Header, index: usize) -> Result<Kind> {
    if header.has("SIMPLE") {
        if index != 0 {
            return Err(Error::parse("fits: a second primary HDU"));
        }
        if header.truth("SIMPLE") != Some(true) {
            return Err(Error::refused(
                "fits: SIMPLE must be T; SIMPLE = F is not accepted",
            ));
        }
        return Ok(Kind::Primary);
    }
    let Some(extension) = header.text("XTENSION") else {
        // No `SIMPLE` and no `XTENSION`: the file's own structure is unknown, and
        // the length of its data would be a guess.
        return Err(Error::parse(
            "fits: an HDU with neither SIMPLE nor XTENSION",
        ));
    };
    let name = extension.trim();
    Ok(if name == "IMAGE" {
        Kind::ImageExt
    } else if name == "BINTABLE" {
        Kind::BinTable
    } else if name == "TABLE" {
        Kind::AsciiTable
    } else {
        Kind::Other
    })
}

/// Walks the whole file and returns every HDU, in order.
fn scan(data: &[u8]) -> Result<Vec<Hdu>> {
    if data.len() < RECORD {
        return Err(Error::parse("fits: shorter than one 2880-byte record"));
    }
    let mut hdus: Vec<Hdu> = Vec::new();
    let mut offset = 0usize;
    while offset < data.len() {
        let header = parse_header(data, offset)?;
        let index = hdus.len();
        let kind = classify(&header, index)?;
        let (bitpix, axes, data_len) = declared_data_len(&header)?;
        let data_start = header.start + header.bytes;
        let available = (data.len() - data_start) as u64;
        if data_len > available {
            return Err(Error::parse(format!(
                "fits: HDU {index} declares {data_len} bytes of data and the file holds {available}"
            )));
        }
        let padded = data_len.div_ceil(RECORD as u64) * RECORD as u64;
        if padded > available {
            return Err(Error::parse(format!(
                "fits: HDU {index} is not padded to a whole 2880-byte record"
            )));
        }
        let end = data_start + padded as usize;
        hdus.push(Hdu {
            header,
            kind,
            bitpix,
            axes,
            data_start,
            data_len,
            end,
        });
        offset = end;
    }
    if offset != data.len() {
        return Err(Error::parse("fits: trailing bytes after the last HDU"));
    }
    Ok(hdus)
}

// ---------------------------------------------------------------------------
// Decisions
// ---------------------------------------------------------------------------

/// What to do with one HDU.
#[derive(Clone)]
enum Decision {
    /// Write it as a tile-compressed table.
    Tile,
    /// Copy it verbatim: not something this handler touches.
    Keep,
    /// Copy it verbatim, and say why if nothing at all could be written.
    Decline(String),
}

impl Decision {
    fn is_tile(&self) -> bool {
        matches!(self, Decision::Tile)
    }
}

/// True for a keyword the generated table header owns, so that it is not copied
/// from the original header a second time.
fn structural(keyword: &str) -> bool {
    const EXACT: [&str; 21] = [
        "SIMPLE", "XTENSION", "BITPIX", "NAXIS", "PCOUNT", "GCOUNT", "EXTEND", "TFIELDS", "THEAP",
        "EXTNAME", "ZIMAGE", "ZSIMPLE", "ZTENSION", "ZBITPIX", "ZNAXIS", "ZPCOUNT", "ZGCOUNT",
        "ZEXTEND", "ZCMPTYPE", "ZNAME1", "ZVAL1",
    ];
    if EXACT.contains(&keyword) {
        return true;
    }
    for prefix in ["NAXIS", "ZNAXIS", "ZTILE", "TTYPE", "TFORM"] {
        if let Some(rest) = keyword.strip_prefix(prefix) {
            if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

/// The Z-form keyword corresponding to an image's structural card.
fn z_form_keyword(keyword: &str) -> Option<String> {
    let fixed = match keyword {
        "SIMPLE" => Some("ZSIMPLE"),
        "XTENSION" => Some("ZTENSION"),
        "BITPIX" => Some("ZBITPIX"),
        "NAXIS" => Some("ZNAXIS"),
        "PCOUNT" => Some("ZPCOUNT"),
        "GCOUNT" => Some("ZGCOUNT"),
        "EXTEND" => Some("ZEXTEND"),
        _ => None,
    };
    if let Some(mapped) = fixed {
        return Some(mapped.to_string());
    }
    let suffix = keyword.strip_prefix("NAXIS")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(format!("ZNAXIS{suffix}"))
}

/// Replaces only columns 1–8 of a card; the value, comment, spacing and all
/// other bytes remain exactly as they appeared in the source.
fn renamed_card(card: &Card, keyword: &str) -> Result<[u8; CARD]> {
    if keyword.len() > 8 {
        return Err(Error::refused(format!(
            "fits: Z-form keyword {keyword} does not fit in eight columns"
        )));
    }
    let mut raw = card.raw;
    raw[..8].fill(b' ');
    raw[..keyword.len()].copy_from_slice(keyword.as_bytes());
    Ok(raw)
}

/// The decision for one image HDU, which is where every refusal lives.
fn decide_image(hdu: &Hdu) -> Decision {
    let header = &hdu.header;
    // A compressed HDU is a `BINTABLE`, so these cards on an image HDU are a
    // contradiction the convention does not describe.
    if header.has("ZIMAGE") || header.has("ZCMPTYPE") {
        return Decision::Decline("fits: an image HDU carrying Z* cards".to_string());
    }
    if header.truth("GROUPS") == Some(true) {
        return Decision::Decline("fits: random groups (GROUPS = T)".to_string());
    }
    if header.int("PCOUNT").unwrap_or(0) != 0 || header.int("GCOUNT").unwrap_or(1) != 1 {
        return Decision::Decline("fits: an image HDU with PCOUNT or GCOUNT set".to_string());
    }
    // Scaling cards are FITS decimals, not necessarily integers. Compare their
    // exact decimal values against identity without rounding through `f64`:
    // 1.00000000000000001 is not 1, and 1D-400 is not 0 even if a binary-float
    // conversion underflows. A malformed value is refused as well; treating an
    // unparsed card as the default would silently open a lossy rewrite.
    if let Some(card) = header.get("BSCALE") {
        if decimal_is_one(card.field()) != Some(true) {
            return Decision::Decline(format!(
                "fits: BSCALE is not exactly 1 (value {})",
                card.field().trim()
            ));
        }
    }
    if let Some(card) = header.get("BZERO") {
        if decimal_is_zero(card.field()) != Some(true) {
            return Decision::Decline(format!(
                "fits: BZERO is not exactly 0 (value {})",
                card.field().trim()
            ));
        }
    }
    if header.has("CHECKSUM") || header.has("DATASUM") {
        return Decision::Decline(
            "fits: the HDU carries CHECKSUM/DATASUM, which this build does not recompute"
                .to_string(),
        );
    }
    if header.count("EXTNAME") > 1 {
        return Decision::Decline("fits: more than one EXTNAME card".to_string());
    }
    if !REWRITE_BITPIX_VALUES.contains(&hdu.bitpix) {
        return Decision::Decline(format!(
            "fits: BITPIX {} is outside this build's tile-rewrite subset",
            hdu.bitpix
        ));
    }
    if hdu.axes.len() > MAX_REWRITE_NAXIS {
        return Decision::Decline(format!(
            "fits: NAXIS = {} exceeds this build's tile-rewrite limit of {}",
            hdu.axes.len(),
            MAX_REWRITE_NAXIS
        ));
    }
    if hdu.axes.contains(&0) {
        return Decision::Decline("fits: an axis of length zero".to_string());
    }
    if hdu.axes.is_empty() || hdu.data_len == 0 {
        // `NAXIS = 0`: an image HDU with no array at all. Nothing to compress.
        return Decision::Keep;
    }
    Decision::Tile
}

/// The decision for one HDU.
fn decide(hdu: &Hdu) -> Decision {
    match hdu.kind {
        Kind::Primary | Kind::ImageExt => decide_image(hdu),
        Kind::BinTable if hdu.header.truth("ZIMAGE") == Some(true) => {
            let codec = hdu.header.text("ZCMPTYPE").unwrap_or("").trim().to_string();
            if codec == GZIP_1 || codec == GZIP_2 {
                // Tier 1 measured 0.00 % on 21 720 tiles (fits-audit §8.2) and
                // does not ship, so these bytes are left exactly as they are.
                Decision::Keep
            } else {
                Decision::Decline(format!(
                    "fits: the image tiles are {codec}, which this build has no codec for"
                ))
            }
        }
        _ => Decision::Keep,
    }
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Byte-for-byte shuffle into decreasing order of significance (convention
/// §6.2, and the permutation the reference writer applies): the most significant
/// byte of every sample, in sample order, then the next byte of every sample.
fn shuffle(samples: &[u8], per_sample: usize) -> Vec<u8> {
    let count = samples.len() / per_sample;
    let mut out = vec![0u8; samples.len()];
    for byte in 0..per_sample {
        for sample in 0..count {
            out[byte * count + sample] = samples[sample * per_sample + byte];
        }
    }
    out
}

/// The inverse of [`shuffle`], used by the verifier.
fn unshuffle(bytes: &[u8], per_sample: usize) -> Vec<u8> {
    let count = bytes.len() / per_sample;
    let mut out = vec![0u8; bytes.len()];
    for byte in 0..per_sample {
        for sample in 0..count {
            out[sample * per_sample + byte] = bytes[byte * count + sample];
        }
    }
    out
}

/// One gzip member holding `payload`, written at level 9 with a zero MTIME so
/// that the same input always produces the same bytes.
fn gzip_member(payload: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(9));
    encoder.write_all(payload)?;
    Ok(encoder.finish()?)
}

/// Inflates `member` and checks that it is a gzip member (RFC 1952).
///
/// Four checks, and the fourth is the one that matters: the magic and compression
/// method at the head, the trailer's `ISIZE`, the trailer's CRC-32 recomputed
/// here with `crc32fast` — a CRC implementation the writer never calls — and only
/// then the payload. `flate2`'s decoder checks the last two as well, so a bug
/// would have to be symmetric in two independent checksums to pass both.
///
/// The declared length is a bound, not an equality: a heap may pad between tiles,
/// so trailing bytes inside a row's declared length are legal and ignored.
fn gunzip_member(member: &[u8]) -> Result<Vec<u8>> {
    // Ten bytes of header, at least one byte of DEFLATE, eight of trailer.
    if member.len() < 19 {
        return Err(Error::parse(
            "fits: a tile is too short to be a gzip member",
        ));
    }
    if member[..3] != [0x1f, 0x8b, 0x08] {
        return Err(Error::parse(
            "fits: a tile does not begin with a gzip member header",
        ));
    }
    let mut decoder = flate2::read::GzDecoder::new(member);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    let trailer = &member[member.len() - 8..];
    let crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    let size = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
    if crc != crc32fast::hash(&out) {
        return Err(Error::parse(
            "fits: a tile's gzip CRC-32 does not match what it inflates to",
        ));
    }
    if size != out.len() as u32 {
        return Err(Error::parse(
            "fits: a tile's gzip ISIZE does not match its length",
        ));
    }
    Ok(out)
}

/// Pads `out` with zeros up to the next 2880-byte boundary, measured from
/// `from`. The caller guarantees `from` is itself record-aligned.
fn pad_to_record(out: &mut Vec<u8>, from: usize) {
    let len = out.len() - from;
    let padded = len.div_ceil(RECORD) * RECORD;
    out.resize(from + padded, 0);
}

/// Appends a header: the cards, an `END` card, then spaces to a record boundary.
fn write_header(out: &mut Vec<u8>, cards: &[[u8; CARD]]) {
    for card in cards {
        out.extend_from_slice(card);
    }
    out.extend_from_slice(&assemble("END", ""));
    let from = out.len() - (cards.len() + 1) * CARD;
    let padded = (out.len() - from).div_ceil(RECORD) * RECORD;
    out.resize(from + padded, b' ');
}

/// The empty primary HDU that has to precede a compressed primary array.
fn empty_primary(out: &mut Vec<u8>) {
    let cards = [
        boolean_card("SIMPLE", true, "conforms to FITS standard"),
        numeric_card("BITPIX", 8, "array data type"),
        numeric_card("NAXIS", 0, "number of array dimensions"),
        boolean_card("EXTEND", true, ""),
    ];
    write_header(out, &cards);
}

/// Writes one image HDU as a tile-compressed `BINTABLE`.
fn write_tiled(hdu: &Hdu, data: &[u8], out: &mut Vec<u8>) -> Result<()> {
    let per_sample = (hdu.bitpix.unsigned_abs() / 8) as usize;
    let first_axis = hdu.axes[0];
    let tile_bytes = first_axis
        .checked_mul(per_sample as u64)
        .ok_or_else(|| Error::limit("fits: the tile size overflows"))?;
    let tiles: u64 = hdu
        .axes
        .iter()
        .skip(1)
        .try_fold(1u64, |total, axis| total.checked_mul(*axis))
        .ok_or_else(|| Error::limit("fits: the tile count overflows"))?;
    if tile_bytes.saturating_mul(tiles) != hdu.data_len {
        return Err(Error::internal(
            "fits: the tiling does not cover the array it was planned for",
        ));
    }
    let floating = hdu.bitpix < 0;
    let codec = if floating { GZIP_2 } else { GZIP_1 };
    let source = &data[hdu.data_start..hdu.data_start + hdu.data_len as usize];

    let mut heap: Vec<u8> = Vec::with_capacity(source.len() / 2);
    let mut rows: Vec<(i32, i32)> = Vec::with_capacity(tiles as usize);
    let tile_bytes = tile_bytes as usize;
    for index in 0..tiles as usize {
        let tile = &source[index * tile_bytes..(index + 1) * tile_bytes];
        let member = if floating {
            gzip_member(&shuffle(tile, per_sample))?
        } else {
            gzip_member(tile)?
        };
        let length = p_descriptor_value(member.len() as u64, "count")?;
        let offset = p_descriptor_value(heap.len() as u64, "heap offset")?;
        rows.push((length, offset));
        heap.extend_from_slice(&member);
    }
    let longest = rows.iter().map(|(length, _)| *length).max().unwrap_or(0);

    let mut cards: Vec<[u8; CARD]> = Vec::with_capacity(30 + hdu.header.cards.len());
    cards.push(string_card(
        "XTENSION",
        "BINTABLE",
        "binary table extension",
    ));
    cards.push(numeric_card("BITPIX", 8, "array data type"));
    cards.push(numeric_card("NAXIS", 2, "number of array dimensions"));
    cards.push(numeric_card("NAXIS1", 8, "width of table in bytes"));
    cards.push(numeric_card(
        "NAXIS2",
        tiles as i64,
        "number of rows in table",
    ));
    cards.push(numeric_card(
        "PCOUNT",
        heap.len() as i64,
        "number of group parameters",
    ));
    cards.push(numeric_card("GCOUNT", 1, "number of groups"));
    cards.push(numeric_card("TFIELDS", 1, "number of fields in each row"));
    cards.push(string_card("TTYPE1", "COMPRESSED_DATA", ""));
    cards.push(string_card("TFORM1", &format!("1PB({longest})"), ""));
    cards.push(boolean_card(
        "ZIMAGE",
        true,
        "extension contains compressed image",
    ));
    let (source_kind, z_kind) = if hdu.kind == Kind::Primary {
        ("SIMPLE", "ZSIMPLE")
    } else {
        ("XTENSION", "ZTENSION")
    };
    let kind_card = hdu.header.get(source_kind).ok_or_else(|| {
        Error::internal(format!("fits: missing {source_kind} card while writing"))
    })?;
    cards.push(renamed_card(kind_card, z_kind)?);

    for (source_keyword, z_keyword) in [("BITPIX", "ZBITPIX"), ("NAXIS", "ZNAXIS")] {
        let source_card = hdu.header.get(source_keyword).ok_or_else(|| {
            Error::internal(format!("fits: missing {source_keyword} card while writing"))
        })?;
        cards.push(renamed_card(source_card, z_keyword)?);
    }
    for index in 0..hdu.axes.len() {
        let axis = index + 1;
        let source_keyword = format!("NAXIS{axis}");
        let z_keyword = format!("ZNAXIS{axis}");
        let source_card = hdu.header.get(&source_keyword).ok_or_else(|| {
            Error::internal(format!("fits: missing {source_keyword} card while writing"))
        })?;
        cards.push(renamed_card(source_card, &z_keyword)?);
    }
    for (source_keyword, z_keyword) in [
        ("PCOUNT", "ZPCOUNT"),
        ("GCOUNT", "ZGCOUNT"),
        ("EXTEND", "ZEXTEND"),
    ] {
        if let Some(source_card) = hdu.header.get(source_keyword) {
            cards.push(renamed_card(source_card, z_keyword)?);
        }
    }
    // The tiling rule: one tile per row along the first axis.
    cards.push(numeric_card(
        "ZTILE1",
        first_axis as i64,
        "size of tiles to be compressed",
    ));
    for index in 2..=hdu.axes.len() {
        cards.push(numeric_card(
            &format!("ZTILE{index}"),
            1,
            "size of tiles to be compressed",
        ));
    }
    cards.push(string_card("ZCMPTYPE", codec, "compression algorithm"));
    if floating {
        // Quantisation off, explicitly: a reader may not assume it, and the
        // claim of losslessness has to be readable in the file.
        cards.push(string_card(
            "ZNAME1",
            "NOISEBIT",
            "floating point quantization level",
        ));
        cards.push(numeric_card(
            "ZVAL1",
            0,
            "floating point quantization level",
        ));
    }
    // Keep the HDU's own EXTNAME when it has one; otherwise name the table the
    // way the convention does.
    match hdu.header.get("EXTNAME") {
        Some(card) => cards.push(card.raw),
        None => cards.push(string_card(
            "EXTNAME",
            "COMPRESSED_IMAGE",
            "name of this binary table extension",
        )),
    }
    // Every remaining card of the original header, verbatim and in order.
    for card in &hdu.header.cards {
        if !structural(card.keyword()) {
            cards.push(card.raw);
        }
    }

    write_header(out, &cards);
    let data_start = out.len();
    for (length, offset) in &rows {
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(&offset.to_be_bytes());
    }
    out.extend_from_slice(&heap);
    pad_to_record(out, data_start);
    Ok(())
}

// ---------------------------------------------------------------------------
// The handler
// ---------------------------------------------------------------------------

pub fn optimize(data: &[u8], dst: &mut dyn Write, opts: &Options) -> Result<Cand> {
    let hdus = scan(data)?;

    if !opts.fits_tile {
        return Err(Error::refused(
            "fits: turning an image HDU into a compressed table changes what a \
             reader sees, so it is opt-in: pass --fits-tile",
        ));
    }

    let decisions: Vec<Decision> = hdus.iter().map(decide).collect();
    let tiles = decisions.iter().filter(|d| d.is_tile()).count();
    if tiles == 0 {
        // Nothing to rewrite. If an image HDU was left alone for a reason, that
        // reason is the answer; otherwise the file simply has no image to
        // compress and is as small as this handler can make it.
        if let Some(reason) = decisions.iter().find_map(|d| match d {
            Decision::Decline(reason) => Some(reason.clone()),
            _ => None,
        }) {
            return Err(Error::refused(reason));
        }
        return Ok(Cand::NoGain);
    }

    let mut out: Vec<u8> = Vec::with_capacity(data.len());
    for (index, hdu) in hdus.iter().enumerate() {
        match &decisions[index] {
            Decision::Tile => {
                // A `BINTABLE` cannot be the first HDU of a file, so a
                // compressed primary array needs an empty primary in front of it.
                if index == 0 {
                    empty_primary(&mut out);
                }
                write_tiled(hdu, data, &mut out)?;
            }
            Decision::Keep | Decision::Decline(_) => {
                out.extend_from_slice(&data[hdu.header.start..hdu.end]);
            }
        }
    }

    if out.len() >= data.len() {
        // Never a larger candidate, and never an equal one: the pipeline's own
        // rule, applied here first so that nothing is written for nothing.
        return Ok(Cand::NoGain);
    }
    dst.write_all(&out)?;
    Ok(Cand::Written(out.len() as u64))
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

/// Compares one rewritten image HDU with the table that replaced it.
fn check_tiled(a: &Hdu, original: &[u8], b: &Hdu, candidate: &[u8]) -> Result<()> {
    if b.kind != Kind::BinTable || b.header.truth("ZIMAGE") != Some(true) {
        return Err(Error::verify(
            "fits: an image HDU was not replaced by a ZIMAGE binary table",
        ));
    }
    let (required_kind, forbidden_kind) = match a.kind {
        Kind::Primary => ("ZSIMPLE", "ZTENSION"),
        Kind::ImageExt => ("ZTENSION", "ZSIMPLE"),
        _ => {
            return Err(Error::verify(
                "fits: a non-image HDU was sent to the image-table verifier",
            ));
        }
    };
    if !b.header.has(required_kind) || b.header.has(forbidden_kind) {
        return Err(Error::verify(format!(
            "fits: the compressed header must use {required_kind} and must not use {forbidden_kind}"
        )));
    }
    if b.header.has("ZSCALE") || b.header.has("ZZERO") {
        return Err(Error::verify(
            "fits: the generated table must not contain ZSCALE or ZZERO",
        ));
    }
    let codec = b.header.text("ZCMPTYPE").unwrap_or("").trim();
    let expected = if a.bitpix < 0 { GZIP_2 } else { GZIP_1 };
    if codec != expected {
        return Err(Error::verify(format!(
            "fits: tiles are {codec}, expected {expected} for BITPIX {}",
            a.bitpix
        )));
    }
    if a.bitpix < 0 {
        if b.header.count("ZNAME1") != 1
            || b.header.text("ZNAME1") != Some("NOISEBIT")
            || b.header.count("ZVAL1") != 1
            || b.header.int("ZVAL1") != Some(0)
        {
            return Err(Error::verify(
                "fits: floating-point tables require ZNAME1 = 'NOISEBIT' and ZVAL1 = 0",
            ));
        }
    } else if b.header.has("ZNAME1") || b.header.has("ZVAL1") {
        return Err(Error::verify(
            "fits: integer tables must not contain ZNAME1 or ZVAL1",
        ));
    }
    if b.header.int("ZBITPIX") != Some(a.bitpix) {
        return Err(Error::verify(format!(
            "fits: ZBITPIX {} does not carry BITPIX {}",
            b.header.int("ZBITPIX").unwrap_or(0),
            a.bitpix
        )));
    }
    if b.header.int("ZNAXIS") != Some(a.axes.len() as i64) {
        return Err(Error::verify(
            "fits: ZNAXIS does not match the original NAXIS",
        ));
    }
    for (index, axis) in a.axes.iter().enumerate() {
        let keyword = format!("ZNAXIS{}", index + 1);
        if b.header.int(&keyword) != Some(*axis as i64) {
            return Err(Error::verify(format!(
                "fits: {keyword} is not the original axis length"
            )));
        }
    }
    // The tiling rule, restated: the first axis whole, every other axis one.
    if b.header.int("ZTILE1") != Some(a.axes[0] as i64) {
        return Err(Error::verify("fits: ZTILE1 is not the whole first axis"));
    }
    for index in 2..=a.axes.len() {
        let keyword = format!("ZTILE{index}");
        if b.header.int(&keyword) != Some(1) {
            return Err(Error::verify(format!(
                "fits: {keyword} is not 1, so the tiling is not the one this handler writes"
            )));
        }
    }
    // Every source structural card is carried as its Z-form with only the
    // keyword changed. This checks the complete value and comment bytes, not
    // merely the parsed number; in particular EXTEND must survive as ZEXTEND.
    for source in &a.header.cards {
        let Some(z_keyword) = z_form_keyword(source.keyword()) else {
            continue;
        };
        let expected = renamed_card(source, &z_keyword)
            .map_err(|_| Error::verify("fits: a structural Z-form keyword is too long"))?;
        let target = b.header.get(&z_keyword).ok_or_else(|| {
            Error::verify(format!(
                "fits: {z_keyword} is missing from the compressed header"
            ))
        })?;
        if target.raw != expected {
            return Err(Error::verify(format!(
                "fits: {z_keyword} did not preserve the source card bytes"
            )));
        }
    }
    if !a.header.has("EXTEND") && b.header.has("ZEXTEND") {
        return Err(Error::verify(
            "fits: ZEXTEND was invented without a source EXTEND card",
        ));
    }
    if b.header.count("EXTNAME") != 1 {
        return Err(Error::verify(
            "fits: the compressed header must contain exactly one EXTNAME",
        ));
    }
    if let Some(source) = a.header.get("EXTNAME") {
        let target = b
            .header
            .get("EXTNAME")
            .ok_or_else(|| Error::verify("fits: EXTNAME was dropped from the compressed header"))?;
        if target.raw != source.raw {
            return Err(Error::verify("fits: EXTNAME was not copied verbatim"));
        }
    } else if b.header.text("EXTNAME") != Some("COMPRESSED_IMAGE") {
        return Err(Error::verify(
            "fits: a missing source EXTNAME must become 'COMPRESSED_IMAGE'",
        ));
    }
    // No non-structural card of the original header may be dropped, and none
    // invented; the two complete sequences must remain identical.
    let want: Vec<[u8; CARD]> = a
        .header
        .cards
        .iter()
        .filter(|card| !structural(card.keyword()))
        .map(|card| card.raw)
        .collect();
    let have: Vec<[u8; CARD]> = b
        .header
        .cards
        .iter()
        .filter(|card| !structural(card.keyword()))
        .map(|card| card.raw)
        .collect();
    if want != have {
        return Err(Error::verify(
            "fits: the cards of the original header did not survive verbatim, or the candidate \
             invented one",
        ));
    }
    // The table itself: one row per tile, each a `P`/`Q` pair into the heap.
    if b.header.int("NAXIS1") != Some(8) || b.header.int("TFIELDS") != Some(1) {
        return Err(Error::verify(
            "fits: the compressed table is not one column of `1PB`",
        ));
    }
    if b.header.int("GCOUNT") != Some(1) {
        return Err(Error::verify(
            "fits: the compressed table must have GCOUNT = 1",
        ));
    }
    if b.header.has("THEAP") {
        return Err(Error::verify(
            "fits: the generated table must not contain THEAP",
        ));
    }
    if b.header.count("TFORM1") != 1 {
        return Err(Error::verify(
            "fits: the compressed table must contain exactly one TFORM1",
        ));
    }
    if b.header.text("TTYPE1").map(str::trim) != Some("COMPRESSED_DATA") {
        return Err(Error::verify(
            "fits: the table has no COMPRESSED_DATA column",
        ));
    }
    let rows = b.header.int("NAXIS2").unwrap_or(-1);
    let expected_rows = a
        .axes
        .iter()
        .skip(1)
        .try_fold(1i64, |total, axis| total.checked_mul(*axis as i64))
        .unwrap_or(-1);
    if rows != expected_rows {
        return Err(Error::verify(format!(
            "fits: the table has {rows} rows and the array needs {expected_rows} tiles"
        )));
    }
    let rows = u64::try_from(rows)
        .ok()
        .filter(|rows| *rows > 0)
        .ok_or_else(|| Error::verify("fits: the table has no rows"))?;
    let heap_len = b.header.int("PCOUNT").unwrap_or(-1);
    let table = 8u64
        .checked_mul(rows)
        .ok_or_else(|| Error::verify("fits: the row count overflows"))?;
    // The table has to fit inside the HDU it was found in, before the heap is
    // looked for: the descriptors are read straight out of the candidate, so an
    // unchecked `table` indexes past the bytes instead of failing. Every step is
    // checked, because `NAXIS2` and `PCOUNT` are numbers a file chose.
    if heap_len < 0 || table > b.data_len {
        return Err(Error::verify("fits: the row count does not fit the data"));
    }
    let heap_len = heap_len as u64;
    if table
        .checked_add(heap_len)
        .is_none_or(|used| used > b.data_len)
    {
        return Err(Error::verify("fits: PCOUNT does not describe the heap"));
    }
    let heap_start = b.data_start + table as usize;
    let heap = &candidate[heap_start..heap_start + heap_len as usize];

    let per_sample = (a.bitpix.unsigned_abs() / 8) as usize;
    let tile_bytes = a.axes[0] as usize * per_sample;
    let mut array: Vec<u8> = Vec::with_capacity(a.data_len as usize);
    let mut longest_descriptor = 0i32;
    for row in 0..rows as usize {
        let at = b.data_start + row * 8;
        let length = i32::from_be_bytes([
            candidate[at],
            candidate[at + 1],
            candidate[at + 2],
            candidate[at + 3],
        ]);
        let offset = i32::from_be_bytes([
            candidate[at + 4],
            candidate[at + 5],
            candidate[at + 6],
            candidate[at + 7],
        ]);
        if length < 0 || offset < 0 {
            return Err(Error::verify(
                "fits: a 1P descriptor has a negative count or heap offset",
            ));
        }
        longest_descriptor = longest_descriptor.max(length);
        let length = usize::try_from(length)
            .map_err(|_| Error::verify("fits: a 1P descriptor count is out of range"))?;
        let offset = usize::try_from(offset)
            .map_err(|_| Error::verify("fits: a 1P descriptor offset is out of range"))?;
        if offset >= heap.len() || length > heap.len() - offset {
            return Err(Error::verify(
                "fits: a heap descriptor points outside the heap",
            ));
        }
        let tile = gunzip_member(&heap[offset..offset + length])
            .map_err(|e| Error::verify(format!("fits: a tile does not inflate: {e}")))?;
        if tile.len() != tile_bytes {
            return Err(Error::verify(format!(
                "fits: a tile inflates to {} bytes and the row is {tile_bytes}",
                tile.len()
            )));
        }
        if a.bitpix < 0 {
            array.extend_from_slice(&unshuffle(&tile, per_sample));
        } else {
            array.extend_from_slice(&tile);
        }
    }
    let expected_tform = format!("1PB({longest_descriptor})");
    if b.header.text("TFORM1") != Some(expected_tform.as_str()) {
        return Err(Error::verify(format!(
            "fits: TFORM1 is {:?}, expected {expected_tform}",
            b.header.text("TFORM1")
        )));
    }
    let before = &original[a.data_start..a.data_start + a.data_len as usize];
    if array != before {
        return Err(Error::verify(
            "fits: the tiles do not decompress to the original array, byte for byte",
        ));
    }
    Ok(())
}

/// Whether an empty primary HDU sits where a compressed primary array needs one.
fn is_empty_primary(hdu: &Hdu) -> bool {
    hdu.kind == Kind::Primary
        && hdu.axes.is_empty()
        && hdu.data_len == 0
        && hdu.header.truth("SIMPLE") == Some(true)
}

pub fn verify(original: &[u8], candidate: &[u8], _opts: &Options) -> Result<()> {
    let before = scan(original).map_err(|e| Error::verify(format!("original: {e}")))?;
    let after = scan(candidate).map_err(|e| Error::verify(format!("candidate: {e}")))?;

    let mut next = 0usize;
    for (index, a) in before.iter().enumerate() {
        if matches!(decide(a), Decision::Tile) {
            // A compressed primary array is preceded by an empty primary HDU.
            if index == 0 && a.kind == Kind::Primary {
                let filler = after
                    .get(next)
                    .ok_or_else(|| Error::verify("fits: the empty primary HDU is missing"))?;
                if !is_empty_primary(filler) {
                    return Err(Error::verify(
                        "fits: the HDU before a compressed primary array is not an empty primary",
                    ));
                }
                next += 1;
            }
            let b = after
                .get(next)
                .ok_or_else(|| Error::verify("fits: a rewritten HDU is missing"))?;
            check_tiled(a, original, b, candidate)?;
        } else {
            let b = after
                .get(next)
                .ok_or_else(|| Error::verify("fits: an HDU was dropped"))?;
            let a_bytes = &original[a.header.start..a.end];
            let b_bytes = &candidate[b.header.start..b.end];
            if a_bytes != b_bytes {
                return Err(Error::verify(format!(
                    "fits: HDU {index} was not copied byte for byte"
                )));
            }
        }
        next += 1;
    }
    if next != after.len() {
        return Err(Error::verify(
            "fits: the candidate has HDUs the original did not",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal FITS writer for the unit tests: one image HDU, optional extras.
    /// The integration fixtures in `tests/common` are the same idea with more
    /// shapes; this one is small enough to keep the module self-contained.
    fn image_hdu(bitpix: i64, axes: &[u64], data: &[u8], extra: &[(&str, &str)]) -> Vec<u8> {
        let mut cards: Vec<[u8; CARD]> = vec![
            boolean_card("SIMPLE", true, "conforms to FITS standard"),
            numeric_card("BITPIX", bitpix, "array data type"),
            numeric_card("NAXIS", axes.len() as i64, ""),
        ];
        for (index, axis) in axes.iter().enumerate() {
            cards.push(numeric_card(
                &format!("NAXIS{}", index + 1),
                *axis as i64,
                "",
            ));
        }
        cards.push(boolean_card("EXTEND", true, ""));
        for (keyword, value) in extra {
            cards.push(assemble(&format!("{keyword:<8}= {value}"), ""));
        }
        let mut out = Vec::new();
        write_header(&mut out, &cards);
        let start = out.len();
        out.extend_from_slice(data);
        pad_to_record(&mut out, start);
        out
    }

    fn options(fits_tile: bool) -> Options {
        Options {
            zopfli: false,
            max_pixels: u64::MAX,
            max_uncompressed: u64::MAX,
            max_ratio: u64::MAX,
            strip_metadata: false,
            nested: false,
            max_depth: 1,
            depth: 0,
            fits_tile,
            allow: super::super::Allow::default(),
        }
    }

    /// Fixture bytes that compress: the pattern repeats every four bytes, so a
    /// page of samples is compressible enough to beat the 2880-byte records the
    /// candidate is padded to. A pattern that repeats every byte would let gzip
    /// win on the padding rather than on the data.
    fn sample(bitpix: i64, rows: u64, columns: u64) -> Vec<u8> {
        let per = (bitpix.unsigned_abs() / 8) as usize;
        let len = rows as usize * columns as usize * per;
        (0..len).map(|i| ((i / 4) % 251) as u8).collect()
    }

    fn optimize_primary_fixture(
        bitpix: i64,
        axes: &[u64],
        data: &[u8],
        extra: &[(&str, &str)],
    ) -> (Vec<u8>, Vec<u8>, Options) {
        let original = image_hdu(bitpix, axes, data, extra);
        let opts = options(true);
        let mut candidate = Vec::new();
        optimize(&original, &mut candidate, &opts).expect("the fixture should be rewritten");
        (original, candidate, opts)
    }

    fn image_extension(bitpix: i64, axes: &[u64], data: &[u8], extra: &[(&str, &str)]) -> Vec<u8> {
        let mut cards = vec![
            string_card("XTENSION", "IMAGE", "image extension"),
            numeric_card("BITPIX", bitpix, "array data type"),
            numeric_card("NAXIS", axes.len() as i64, "number of array dimensions"),
        ];
        for (index, axis) in axes.iter().enumerate() {
            cards.push(numeric_card(
                &format!("NAXIS{}", index + 1),
                *axis as i64,
                "",
            ));
        }
        cards.push(numeric_card("PCOUNT", 0, "number of group parameters"));
        cards.push(numeric_card("GCOUNT", 1, "number of groups"));
        for (keyword, value) in extra {
            cards.push(assemble(&format!("{keyword:<8}= {value}"), ""));
        }
        let mut out = Vec::new();
        write_header(&mut out, &cards);
        let start = out.len();
        out.extend_from_slice(data);
        pad_to_record(&mut out, start);
        out
    }

    fn optimize_extension_fixture(
        bitpix: i64,
        axes: &[u64],
        data: &[u8],
        extra: &[(&str, &str)],
    ) -> (Vec<u8>, Vec<u8>, Options) {
        let mut original = Vec::new();
        empty_primary(&mut original);
        original.extend_from_slice(&image_extension(bitpix, axes, data, extra));
        let opts = options(true);
        let mut candidate = Vec::new();
        optimize(&original, &mut candidate, &opts).expect("the fixture should be rewritten");
        (original, candidate, opts)
    }

    fn replace_candidate_card(
        candidate: &mut [u8],
        hdu_index: usize,
        keyword: &str,
        replacement: [u8; CARD],
    ) {
        let hdus = scan(candidate).expect("the unmodified candidate parses");
        let header = &hdus[hdu_index].header;
        let card_index = header
            .cards
            .iter()
            .position(|card| card.keyword() == keyword)
            .unwrap_or_else(|| panic!("missing candidate card {keyword}"));
        let offset = header.start + card_index * CARD;
        candidate[offset..offset + CARD].copy_from_slice(&replacement);
    }

    fn insert_candidate_card(candidate: &mut [u8], hdu_index: usize, card: [u8; CARD]) {
        let hdus = scan(candidate).expect("the unmodified candidate parses");
        let header = &hdus[hdu_index].header;
        let end = (header.start..header.start + header.bytes)
            .step_by(CARD)
            .find(|offset| candidate[*offset..*offset + 8] == *b"END     ")
            .expect("the candidate header has END");
        assert!(
            end + 2 * CARD <= header.start + header.bytes,
            "fixture header has room for one inserted card"
        );
        candidate[end..end + CARD].copy_from_slice(&card);
        candidate[end + CARD..end + 2 * CARD].copy_from_slice(&assemble("END", ""));
    }

    fn assert_verify_rejects(original: &[u8], candidate: &[u8], opts: &Options, reason: &str) {
        let error =
            verify(original, candidate, opts).expect_err("the corrupted candidate must fail");
        assert!(
            error.to_string().contains(reason),
            "expected refusal mentioning {reason:?}, got {error}"
        );
    }

    #[test]
    fn an_image_hdu_becomes_a_compressed_table_that_round_trips() {
        // 100 rows of 200 int16 samples: large enough that the tiles outweigh the
        // table header, which is the case the handler exists for.
        let data = sample(16, 100, 200);
        let original = image_hdu(16, &[200, 100], &data, &[]);
        let opts = options(true);
        let mut candidate = Vec::new();
        let cand = optimize(&original, &mut candidate, &opts).expect("a gain is expected");
        assert!(matches!(cand, Cand::Written(_)));
        assert!(candidate.len() < original.len());
        verify(&original, &candidate, &opts).expect("the round trip has to hold");
    }

    #[test]
    fn parser_accepts_all_standard_bitpix_values_and_rank_999() {
        for bitpix in BITPIX_VALUES {
            let width = (bitpix.unsigned_abs() / 8) as usize;
            let original = image_hdu(bitpix, &[1], &vec![0; width], &[]);
            let parsed = scan(&original).expect("all six standard BITPIX values remain parseable");
            assert_eq!(parsed[0].bitpix, bitpix);
            assert_eq!(parsed[0].axes, vec![1]);
        }

        let axes = vec![1; MAX_NAXIS as usize];
        let original = image_hdu(16, &axes, &[0, 0], &[]);
        let parsed = scan(&original).expect("the parser retains the standard rank limit");
        assert_eq!(parsed[0].axes.len(), MAX_NAXIS as usize);
        assert!(matches!(
            decide_image(&parsed[0]),
            Decision::Decline(reason) if reason == "fits: NAXIS = 999 exceeds this build's tile-rewrite limit of 6"
        ));
    }

    #[test]
    fn bitpix_64_and_rank_7_parse_but_are_declined_for_rewriting() {
        let cases: [(i64, Vec<u64>, Vec<u8>, &str); 2] = [
            (
                64,
                vec![2],
                vec![0; 16],
                "fits: BITPIX 64 is outside this build's tile-rewrite subset",
            ),
            (
                16,
                vec![2, 1, 1, 1, 1, 1, 1],
                vec![0; 4],
                "fits: NAXIS = 7 exceeds this build's tile-rewrite limit of 6",
            ),
        ];
        for (bitpix, axes, data, reason) in cases {
            let original = image_hdu(bitpix, &axes, &data, &[]);
            let parsed = scan(&original).expect("valid-but-unsupported FITS remains parseable");
            assert!(matches!(
                decide_image(&parsed[0]),
                Decision::Decline(actual) if actual == reason
            ));
            let mut output = Vec::new();
            let error = optimize(&original, &mut output, &options(true))
                .expect_err("unsupported rewrite subset is a refusal");
            assert!(error.to_string().contains(reason), "{error}");
            assert!(output.is_empty(), "a declined HDU must not produce output");
        }
    }

    #[test]
    fn rank_six_is_rewritten_and_passes_repository_verification() {
        let axes = [1_048_576, 1, 1, 1, 1, 1];
        let data = sample(16, 1, axes[0]);
        let original = image_hdu(16, &axes, &data, &[]);
        let opts = options(true);
        let mut candidate = Vec::new();
        let result = optimize(&original, &mut candidate, &opts).expect("rank six is supported");
        assert!(matches!(result, Cand::Written(_)));
        assert!(candidate.len() < original.len());
        verify(&original, &candidate, &opts).expect("rank-six candidate round-trips internally");
    }

    #[test]
    fn znaxis100_still_fails_the_keyword_field_defense_in_depth_check() {
        let original = image_hdu(16, &[1; 100], &[0, 0], &[]);
        let parsed = scan(&original).expect("rank 100 remains parseable by the FITS reader");
        assert_eq!(z_form_keyword("NAXIS100").as_deref(), Some("ZNAXIS100"));
        let card = parsed[0].header.get("NAXIS100").expect("NAXIS100 card");
        let error = renamed_card(card, "ZNAXIS100").expect_err("nine characters do not fit");
        assert!(error
            .to_string()
            .contains("ZNAXIS100 does not fit in eight columns"));
    }

    #[test]
    fn a_float_array_is_shuffled_and_round_trips() {
        let data = sample(-32, 60, 80);
        let original = image_hdu(-32, &[80, 60], &data, &[]);
        let opts = options(true);
        let mut candidate = Vec::new();
        optimize(&original, &mut candidate, &opts).expect("a gain is expected");
        verify(&original, &candidate, &opts).expect("the round trip has to hold");
        // The candidate declares GZIP_2 and says quantisation is off.
        let text = String::from_utf8_lossy(&candidate);
        assert!(text.contains("GZIP_2"));
        assert!(text.contains("NOISEBIT"));
    }

    #[test]
    fn without_the_flag_nothing_is_written() {
        let original = image_hdu(16, &[4, 4], &sample(16, 4, 4), &[]);
        let opts = options(false);
        let mut candidate = Vec::new();
        let err = optimize(&original, &mut candidate, &opts).expect_err("opt-in");
        assert!(err.to_string().contains("--fits-tile"), "{err}");
        assert!(candidate.is_empty());
    }

    #[test]
    fn the_shuffle_is_the_permutation_the_convention_names() {
        // Two 32-bit samples, most significant byte first: the reference
        // writer's own output for these eight bytes is the expectation.
        let samples = [0x01, 0x02, 0x03, 0x04, 0x0a, 0x0b, 0x0c, 0x0d];
        assert_eq!(
            shuffle(&samples, 4),
            vec![0x01, 0x0a, 0x02, 0x0b, 0x03, 0x0c, 0x04, 0x0d]
        );
        assert_eq!(unshuffle(&shuffle(&samples, 4), 4), samples.to_vec());
    }

    #[test]
    fn a_candidate_that_would_grow_is_discarded() {
        // A 2×2 int16 image: the table header and its padding outweigh the four
        // samples, so the honest answer is "no gain", not a larger file.
        let original = image_hdu(16, &[2, 2], &sample(16, 2, 2), &[]);
        let opts = options(true);
        let mut candidate = Vec::new();
        let cand = optimize(&original, &mut candidate, &opts).expect("not an error");
        assert!(
            matches!(cand, Cand::NoGain),
            "a growing candidate is refused"
        );
        assert!(candidate.is_empty());
    }

    #[test]
    fn a_checksum_keeps_the_hdu_verbatim() {
        let data = sample(16, 100, 200);
        let original = image_hdu(
            16,
            &[200, 100],
            &data,
            &[("CHECKSUM", "'ABCDEFGH'"), ("DATASUM", "12345")],
        );
        let opts = options(true);
        let mut candidate = Vec::new();
        let err = optimize(&original, &mut candidate, &opts)
            .expect_err("a checksum this build cannot recompute is a refusal");
        assert!(err.to_string().contains("CHECKSUM"), "{err}");
    }

    #[test]
    fn a_legal_random_groups_primary_is_refused() {
        let axis2 = 4096u64;
        let pcount = 17u64;
        let gcount = 3u64;
        let data_len = 2 * gcount * (pcount + axis2);
        let data = vec![0x5a; data_len as usize];
        let original = image_hdu(
            16,
            &[0, axis2],
            &data,
            &[("GROUPS", "T"), ("PCOUNT", "17"), ("GCOUNT", "3")],
        );
        let opts = options(true);
        let mut candidate = Vec::new();
        let err = optimize(&original, &mut candidate, &opts).expect_err("random groups");
        assert!(err.to_string().contains("GROUPS"), "{err}");
        assert!(candidate.is_empty());
    }

    #[test]
    fn a_scaled_image_is_refused() {
        let original = image_hdu(16, &[8, 4], &sample(16, 4, 8), &[("BZERO", "32768")]);
        let opts = options(true);
        let mut candidate = Vec::new();
        let err = optimize(&original, &mut candidate, &opts).expect_err("a scaled image");
        assert!(err.to_string().contains("BZERO"), "{err}");
    }

    #[test]
    fn a_zero_scale_is_refused() {
        let original = image_hdu(16, &[8, 4], &sample(16, 4, 8), &[("BSCALE", "0")]);
        let opts = options(true);
        let mut candidate = Vec::new();
        let err = optimize(&original, &mut candidate, &opts).expect_err("BSCALE = 0");
        assert!(err.to_string().contains("BSCALE"), "{err}");
    }

    #[test]
    fn a_truncated_file_is_a_parse_failure_not_a_rewrite() {
        let original = image_hdu(16, &[8, 8], &sample(16, 8, 8), &[]);
        let short = &original[..original.len() - 100];
        let opts = options(true);
        let mut candidate = Vec::new();
        assert!(optimize(short, &mut candidate, &opts).is_err());
        assert!(candidate.is_empty());
    }

    #[test]
    fn a_header_lookup_finds_the_first_card_of_its_keyword() {
        // `get` is what every decision in this file asks, and what decides
        // whether an `EXTNAME` survives or a `PCOUNT` gains a `ZPCOUNT`: the
        // first card wins, a keyword that is absent is absent, and a repeated
        // keyword is counted rather than hidden.
        let extra = [("EXTNAME", "'SCI'"), ("PCOUNT", "0"), ("GCOUNT", "1")];
        let original = image_hdu(16, &[4, 2], &sample(16, 2, 4), &extra);
        let header = parse_header(&original, 0).expect("the header parses");
        let name = header.get("EXTNAME").expect("EXTNAME is in the header");
        assert_eq!(name.keyword(), "EXTNAME");
        assert_eq!(name.text(), Some("SCI"));
        assert_eq!(header.text("EXTNAME"), Some("SCI"));
        assert!(header.has("PCOUNT"));
        assert_eq!(header.int("PCOUNT"), Some(0));
        assert!(!header.has("ZTILE1"));
        assert!(header.get("ZTILE1").is_none());
        assert_eq!(header.int("ZTILE1"), None);
        assert_eq!(header.count("NAXIS"), 1);
        assert_eq!(header.count("ZTILE1"), 0);
    }

    #[test]
    fn one_p_descriptor_limit_is_the_exact_signed_32_bit_maximum() {
        let maximum = i32::MAX as u64;
        assert_eq!(MAX_P_DESCRIPTOR_VALUE, maximum);
        assert_eq!(p_descriptor_value(0, "count").unwrap(), 0);
        assert_eq!(p_descriptor_value(maximum, "count").unwrap(), i32::MAX);
        assert_eq!(
            p_descriptor_value(maximum, "heap offset").unwrap(),
            i32::MAX
        );
        let first_unrepresentable = maximum.checked_add(1).unwrap();
        for field in ["count", "heap offset"] {
            let error = p_descriptor_value(first_unrepresentable, field)
                .expect_err("2^31 cannot be represented by a signed P descriptor");
            assert!(error.to_string().contains("1P descriptor"), "{error}");
            assert!(error.to_string().contains("2147483647"), "{error}");
        }
    }

    #[test]
    fn generated_table_cards_match_the_primary_and_float_extension_contracts() {
        let int_data = sample(16, 120, 256);
        let (int_original, int_candidate, opts) =
            optimize_primary_fixture(16, &[256, 120], &int_data, &[]);
        let int_hdus = scan(&int_candidate).expect("integer candidate parses");
        let int_table = &int_hdus[1];
        assert_eq!(int_table.header.truth("ZSIMPLE"), Some(true));
        assert!(!int_table.header.has("ZTENSION"));
        assert_eq!(int_table.header.truth("ZEXTEND"), Some(true));
        assert_eq!(int_table.header.text("EXTNAME"), Some("COMPRESSED_IMAGE"));
        assert!(int_table
            .header
            .text("TFORM1")
            .is_some_and(|form| { form.starts_with("1PB(") && form.ends_with(')') }));
        assert_eq!(int_table.header.int("GCOUNT"), Some(1));
        assert!(!int_table.header.has("THEAP"));
        assert!(!int_table.header.has("ZSCALE"));
        assert!(!int_table.header.has("ZZERO"));
        verify(&int_original, &int_candidate, &opts).expect("integer generated cards verify");

        let float_data = sample(-32, 64, 128);
        let (float_original, float_candidate, opts) =
            optimize_extension_fixture(-32, &[128, 64], &float_data, &[("EXTNAME", "'SCI'")]);
        let float_hdus = scan(&float_candidate).expect("float candidate parses");
        let float_table = &float_hdus[1];
        assert_eq!(float_table.header.text("ZTENSION"), Some("IMAGE"));
        assert!(!float_table.header.has("ZSIMPLE"));
        assert_eq!(float_table.header.text("ZNAME1"), Some("NOISEBIT"));
        assert_eq!(float_table.header.int("ZVAL1"), Some(0));
        assert_eq!(float_table.header.text("EXTNAME"), Some("SCI"));
        assert_eq!(float_table.header.int("GCOUNT"), Some(1));
        assert!(!float_table.header.has("THEAP"));
        assert!(!float_table.header.has("ZSCALE"));
        assert!(!float_table.header.has("ZZERO"));
        verify(&float_original, &float_candidate, &opts).expect("float generated cards verify");
    }

    #[test]
    fn verify_rejects_a_wrong_tform1() {
        let data = sample(16, 120, 256);
        let (original, mut candidate, opts) = optimize_primary_fixture(16, &[256, 120], &data, &[]);
        replace_candidate_card(
            &mut candidate,
            1,
            "TFORM1",
            string_card("TFORM1", "1QB(999)", ""),
        );
        assert_verify_rejects(&original, &candidate, &opts, "TFORM1");
    }

    #[test]
    fn verify_rejects_a_primary_table_with_the_wrong_z_kind_role() {
        let data = sample(16, 120, 256);
        let (original, mut candidate, opts) = optimize_primary_fixture(16, &[256, 120], &data, &[]);
        insert_candidate_card(
            &mut candidate,
            1,
            string_card("ZTENSION", "IMAGE", "wrong role for a primary image"),
        );
        assert_verify_rejects(&original, &candidate, &opts, "ZTENSION");
    }

    #[test]
    fn verify_rejects_an_image_extension_table_with_the_wrong_z_kind_role() {
        let data = sample(-32, 64, 128);
        let (original, mut candidate, opts) =
            optimize_extension_fixture(-32, &[128, 64], &data, &[("EXTNAME", "'SCI'")]);
        insert_candidate_card(
            &mut candidate,
            1,
            boolean_card("ZSIMPLE", true, "wrong role"),
        );
        assert_verify_rejects(&original, &candidate, &opts, "ZSIMPLE");
    }

    #[test]
    fn verify_rejects_a_changed_zextend_card() {
        let data = sample(16, 120, 256);
        let (original, mut candidate, opts) = optimize_primary_fixture(16, &[256, 120], &data, &[]);
        replace_candidate_card(
            &mut candidate,
            1,
            "ZEXTEND",
            boolean_card("ZEXTEND", false, "changed"),
        );
        assert_verify_rejects(&original, &candidate, &opts, "ZEXTEND");
    }

    #[test]
    fn verify_rejects_changed_float_quantization_cards() {
        let data = sample(-32, 64, 128);
        let (original, candidate, opts) = optimize_primary_fixture(-32, &[128, 64], &data, &[]);
        for (keyword, replacement) in [
            ("ZNAME1", string_card("ZNAME1", "BLOCKED", "changed")),
            ("ZVAL1", numeric_card("ZVAL1", 1, "changed")),
        ] {
            let mut corrupted = candidate.clone();
            replace_candidate_card(&mut corrupted, 1, keyword, replacement);
            assert_verify_rejects(
                &original,
                &corrupted,
                &opts,
                "floating-point tables require ZNAME1",
            );
        }
    }

    #[test]
    fn verify_rejects_an_invented_or_changed_extname() {
        let data = sample(16, 120, 256);
        let (original, mut candidate, opts) = optimize_primary_fixture(16, &[256, 120], &data, &[]);
        replace_candidate_card(
            &mut candidate,
            1,
            "EXTNAME",
            string_card("EXTNAME", "OTHER", "changed default"),
        );
        assert_verify_rejects(&original, &candidate, &opts, "EXTNAME");
    }

    #[test]
    fn verify_rejects_zscale_and_zzero_cards() {
        let data = sample(-32, 64, 128);
        let (original, candidate, opts) = optimize_primary_fixture(-32, &[128, 64], &data, &[]);
        for (keyword, card) in [
            ("ZSCALE", numeric_card("ZSCALE", 1, "must not be generated")),
            ("ZZERO", numeric_card("ZZERO", 0, "must not be generated")),
        ] {
            let mut corrupted = candidate.clone();
            insert_candidate_card(&mut corrupted, 1, card);
            assert_verify_rejects(&original, &corrupted, &opts, keyword);
        }
    }

    #[test]
    fn verify_rejects_an_explicit_theap_card() {
        let data = sample(16, 120, 256);
        let (original, candidate, opts) = optimize_primary_fixture(16, &[256, 120], &data, &[]);
        let rows = scan(&candidate).unwrap()[1].header.int("NAXIS2").unwrap();
        let mut corrupted = candidate;
        insert_candidate_card(
            &mut corrupted,
            1,
            numeric_card("THEAP", rows * 8, "explicit default heap offset"),
        );
        assert_verify_rejects(&original, &corrupted, &opts, "THEAP");
    }

    #[test]
    fn verify_rejects_a_nonidentity_gcount_without_a_large_fixture() {
        let axes = [1024u64, 10];
        let data = vec![0u8; (axes[0] * axes[1] * 2) as usize];
        let (original, mut candidate, opts) = optimize_primary_fixture(16, &axes, &data, &[]);
        let hdus = scan(&candidate).expect("candidate parses before GCOUNT corruption");
        let table = &hdus[1];
        let single_group_bytes =
            8 * table.header.int("NAXIS2").unwrap() + table.header.int("PCOUNT").unwrap();
        assert!(
            single_group_bytes * 2 <= RECORD as i64,
            "corruption should remain parseable"
        );
        replace_candidate_card(
            &mut candidate,
            1,
            "GCOUNT",
            numeric_card("GCOUNT", 2, "corrupted for verifier regression"),
        );
        assert!(
            scan(&candidate).is_ok(),
            "the mutated file is still structurally walkable"
        );
        assert_verify_rejects(&original, &candidate, &opts, "GCOUNT");
    }

    #[test]
    fn verify_rejects_negative_signed_p_descriptor_fields() {
        let data = sample(16, 120, 256);
        let (original, candidate, opts) = optimize_primary_fixture(16, &[256, 120], &data, &[]);
        let hdus = scan(&candidate).expect("candidate parses before descriptor corruption");
        let descriptor = hdus[1].data_start;
        for offset in [0usize, 4usize] {
            let mut corrupted = candidate.clone();
            corrupted[descriptor + offset..descriptor + offset + 4]
                .copy_from_slice(&i32::MIN.to_be_bytes());
            assert_verify_rejects(
                &original,
                &corrupted,
                &opts,
                "negative count or heap offset",
            );
        }
    }

    #[test]
    fn cards_parse_their_values() {
        let card = Card {
            raw: assemble("BITPIX  =                  -32", ""),
        };
        assert_eq!(card.keyword(), "BITPIX");
        assert_eq!(card.int(), Some(-32));
        let card = Card {
            raw: assemble("ZIMAGE  =                    T", ""),
        };
        assert_eq!(card.truth(), Some(true));
        let card = Card {
            raw: assemble("ZCMPTYPE= 'GZIP_2'", ""),
        };
        assert_eq!(card.text(), Some("GZIP_2"));
    }

    #[test]
    fn a_padded_long_codec_string_uses_the_complete_card_value_area() {
        let card = Card {
            raw: string_card("ZCMPTYPE", "HCOMPRESS_1         ", ""),
        };
        assert_eq!(card.text(), Some("HCOMPRESS_1         "));
    }

    #[test]
    fn quoted_text_stops_before_a_comment_containing_quotes_and_slashes() {
        let card = Card {
            raw: string_card(
                "OBJECT",
                "science/field",
                "comment has 'quoted text' / note",
            ),
        };
        assert_eq!(card.text(), Some("science/field"));
    }
}
