//! TIFF, classic baseline with strips — v0.7.
//!
//! The file is rebuilt rather than patched: a TIFF stores absolute offsets, so
//! shrinking a strip moves everything after it. The rebuild is deterministic —
//! header, IFD, out-of-line values, strips — which is what makes verification
//! possible: both files are parsed, every entry is compared as a resolved value
//! rather than as bytes at a fixed position, and every strip is decoded and
//! compared byte for byte.
//!
//! The accepted subset is deliberately narrow, and each exclusion is a refusal
//! with a reason rather than a guess:
//!
//! - classic TIFF only (`42`, not BigTIFF `43`), one IFD, strips rather than
//!   tiles, interleaved samples, no predictor;
//! - `Compression` 1 (none), 8 or 32946 (Deflate). LZW is refused because this
//!   build has no LZW encoder, and a strip that does not decode is not a file;
//! - every out-of-line value belongs to a tag known to carry plain data. An
//!   unknown out-of-line tag may hold an absolute offset that the rebuild would
//!   invalidate, so the file is refused instead. An unknown tag whose value fits
//!   in the entry's four value bytes is copied as it is — it never moves.
//!
//! `--strip-metadata` drops the textual tags listed in `is_stripped` and nothing
//! else; the colour and geometry tags always stay.

use std::io::{Read, Write};

use crate::error::{Error, Result};

use super::{Cand, Options};

const MAGIC_CLASSIC: u16 = 42;

const TYPE_BYTE: u16 = 1;
const TYPE_ASCII: u16 = 2;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_RATIONAL: u16 = 5;
const TYPE_SBYTE: u16 = 6;
const TYPE_UNDEFINED: u16 = 7;
const TYPE_SSHORT: u16 = 8;
const TYPE_SLONG: u16 = 9;
const TYPE_SRATIONAL: u16 = 10;
const TYPE_FLOAT: u16 = 11;
const TYPE_DOUBLE: u16 = 12;

const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_STRIP_OFFSETS: u16 = 273;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_ROWS_PER_STRIP: u16 = 278;
const TAG_STRIP_BYTE_COUNTS: u16 = 279;
const TAG_PLANAR_CONFIG: u16 = 284;
const TAG_PREDICTOR: u16 = 317;
const TAG_TILE_WIDTH: u16 = 322;
const TAG_TILE_LENGTH: u16 = 323;
const TAG_TILE_OFFSETS: u16 = 324;
const TAG_TILE_BYTE_COUNTS: u16 = 325;
const TAG_SUB_IFDS: u16 = 330;
const TAG_EXIF_IFD: u16 = 34665;
const TAG_GPS_IFD: u16 = 34853;
const TAG_INTEROP_IFD: u16 = 40965;
const TAG_JPEG_OFFSET: u16 = 513;

const COMPRESSION_NONE: u16 = 1;
const COMPRESSION_DEFLATE: u16 = 8;
const COMPRESSION_ADOBE_DEFLATE: u16 = 32946;

/// Tags whose out-of-line value is plain data: moving it cannot invalidate
/// anything, because nothing in the file points into the middle of it.
///
/// The list is the union of the baseline tags, the common private ones and the
/// GeoTIFF data tags. Anything else with an out-of-line value is refused —
/// including `SubIFDs`, `ExifIFD`, `GPSIFD`, the tile tags and the JPEG-offset
/// tags, which carry absolute offsets by definition.
const DATA_TAGS: &[u16] = &[
    254, // NewSubfileType
    255, // SubfileType
    256, 257, 258, 259, // geometry and encoding
    262, 263, 266, 269, 270, 271, 272, // photometric and textual
    273, 274, 277, 278, 279, // layout and orientation
    280, 281, 282, 283, 284, 286, 287, // ranges and resolution
    292, 293, 296, 297, // shape and page number
    305, 306, 315, 316, 317, 318, 319, 320, // software, artist, predictor, colour
    338, 339, 340, 341, 342, 343, 344, 347, // extra samples, sample format, transfer
    530, 531, 532,   // subsampling
    700,   // XMP packet
    33432, // Copyright
    33550, 33922, 34735, 34736, 34737, // GeoTIFF data
    33723, 34377, 34675, 37510, // IPTC, Photoshop, ICC, UserComment
    37724, // ImageSourceData (Photoshop layers)
    40091, 40092, 40093, 40094, 40095, // Windows XP tags
    42112, 42113, // GDAL metadata and nodata
];

/// Textual and descriptive tags removed by `--strip-metadata`.
pub fn is_stripped(tag: u16) -> bool {
    matches!(
        tag,
        269 | 270
            | 271
            | 272
            | 305
            | 306
            | 315
            | 316
            | 333
            | 33432
            | 33723
            | 34377
            | 37510
            | 37724
            | 40091
            | 40092
            | 40093
            | 40094
            | 40095
            | 42112
    )
}

fn survives(tag: u16, strip: bool) -> bool {
    !(strip && is_stripped(tag))
}

fn type_size(typ: u16) -> Option<usize> {
    match typ {
        TYPE_BYTE | TYPE_ASCII | TYPE_SBYTE | TYPE_UNDEFINED => Some(1),
        TYPE_SHORT | TYPE_SSHORT => Some(2),
        TYPE_LONG | TYPE_SLONG | TYPE_FLOAT => Some(4),
        TYPE_RATIONAL | TYPE_SRATIONAL | TYPE_DOUBLE => Some(8),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

struct Entry<'a> {
    tag: u16,
    typ: u16,
    count: u32,
    /// Resolved value bytes, whether they were inline or elsewhere in the file.
    value: &'a [u8],
}

struct Tiff<'a> {
    data: &'a [u8],
    /// True for big-endian files.
    big: bool,
    /// Where the header says the IFD lives. Real files put it after the image
    /// data as often as before it, so this cannot be assumed.
    ifd: usize,
    entries: Vec<Entry<'a>>,
}

impl<'a> Tiff<'a> {
    fn u16_at(&self, at: usize) -> Result<u16> {
        let s = self
            .data
            .get(at..at + 2)
            .ok_or_else(|| Error::parse("tiff: read past the end of the file"))?;
        Ok(if self.big {
            u16::from_be_bytes([s[0], s[1]])
        } else {
            u16::from_le_bytes([s[0], s[1]])
        })
    }

    fn u32_at(&self, at: usize) -> Result<u32> {
        let s = self
            .data
            .get(at..at + 4)
            .ok_or_else(|| Error::parse("tiff: read past the end of the file"))?;
        Ok(if self.big {
            u32::from_be_bytes([s[0], s[1], s[2], s[3]])
        } else {
            u32::from_le_bytes([s[0], s[1], s[2], s[3]])
        })
    }

    fn entry(&self, tag: u16) -> Option<&Entry<'a>> {
        self.entries.iter().find(|e| e.tag == tag)
    }

    /// Reads a SHORT or LONG array, which is what every numeric tag we use is.
    fn numbers(&self, entry: &Entry<'a>) -> Result<Vec<u32>> {
        let size = type_size(entry.typ).ok_or_else(|| {
            Error::refused(format!("tiff: tag {} uses type {}", entry.tag, entry.typ))
        })?;
        let count = entry.count as usize;
        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            let at = index * size;
            match entry.typ {
                TYPE_SHORT => {
                    let s = entry
                        .value
                        .get(at..at + 2)
                        .ok_or_else(|| Error::parse("tiff: short value runs past its data"))?;
                    out.push(if self.big {
                        u16::from_be_bytes([s[0], s[1]]) as u32
                    } else {
                        u16::from_le_bytes([s[0], s[1]]) as u32
                    });
                }
                TYPE_LONG => out.push(self.u32_array_at(entry, at)?),
                other => {
                    return Err(Error::refused(format!(
                        "tiff: tag {} has type {other}, which this build does not read",
                        entry.tag
                    )))
                }
            }
        }
        Ok(out)
    }

    fn u32_array_at(&self, entry: &Entry<'a>, at: usize) -> Result<u32> {
        let s = entry
            .value
            .get(at..at + 4)
            .ok_or_else(|| Error::parse("tiff: long value runs past its data"))?;
        Ok(if self.big {
            u32::from_be_bytes([s[0], s[1], s[2], s[3]])
        } else {
            u32::from_le_bytes([s[0], s[1], s[2], s[3]])
        })
    }

    fn scalar(&self, tag: u16, default: Option<u32>) -> Result<u32> {
        match self.entry(tag) {
            Some(entry) => {
                let values = self.numbers(entry)?;
                match values.first() {
                    Some(v) => Ok(*v),
                    None => Err(Error::parse(format!("tiff: tag {tag} has no value"))),
                }
            }
            None => default.ok_or_else(|| Error::parse(format!("tiff: tag {tag} is missing"))),
        }
    }
}

fn parse(data: &[u8]) -> Result<Tiff<'_>> {
    if data.len() < 8 {
        return Err(Error::parse("tiff: file is shorter than a header"));
    }
    let big = match &data[..2] {
        [0x4d, 0x4d] => true,
        [0x49, 0x49] => false,
        _ => return Err(Error::parse("tiff: unknown byte order marker")),
    };
    let mut tiff = Tiff {
        data,
        big,
        ifd: 0,
        entries: Vec::new(),
    };

    let magic = tiff.u16_at(2)?;
    if magic != MAGIC_CLASSIC {
        return Err(Error::refused(if magic == 43 {
            "tiff: BigTIFF is not supported"
        } else {
            "tiff: not a TIFF file (bad magic number)"
        }));
    }

    let ifd = tiff.u32_at(4)? as usize;
    if ifd < 8 || ifd + 2 > data.len() {
        return Err(Error::parse("tiff: the IFD lies outside the file"));
    }
    tiff.ifd = ifd;
    let count = tiff.u16_at(ifd)? as usize;
    if count == 0 {
        return Err(Error::refused("tiff: an empty IFD has nothing to rewrite"));
    }
    let entries_at = ifd + 2;
    let next_at = entries_at + count * 12;
    if next_at + 4 > data.len() {
        return Err(Error::parse("tiff: the IFD runs past the end of the file"));
    }
    let next = tiff.u32_at(next_at)?;
    if next != 0 {
        return Err(Error::refused(
            "tiff: more than one IFD (multi-page or multi-image) is not supported",
        ));
    }

    let mut previous = 0u16;
    for index in 0..count {
        let at = entries_at + index * 12;
        let raw = &data[at..at + 12];
        let tag = tiff.u16_at(at)?;
        let typ = tiff.u16_at(at + 2)?;
        let count = tiff.u32_at(at + 4)?;
        if tag <= previous && index > 0 {
            return Err(Error::refused(
                "tiff: IFD entries are not in ascending tag order",
            ));
        }
        previous = tag;

        let size = type_size(typ).ok_or_else(|| {
            Error::refused(format!(
                "tiff: tag {tag} uses type {typ}, which is not part of classic TIFF"
            ))
        })?;
        let len = size
            .checked_mul(count as usize)
            .ok_or_else(|| Error::parse("tiff: an entry's value length overflows"))?;
        let value = if len <= 4 {
            &raw[8..8 + len]
        } else {
            let offset = tiff.u32_at(at + 8)? as usize;
            data.get(offset..offset + len)
                .ok_or_else(|| Error::parse(format!("tiff: tag {tag} points outside the file")))?
        };
        tiff.entries.push(Entry {
            tag,
            typ,
            count,
            value,
        });
    }

    Ok(tiff)
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

struct Plan<'a> {
    tiff: &'a Tiff<'a>,
    compression: u16,
    strips: Vec<&'a [u8]>,
    /// Filled in when the file is rebuilt.
    new_compression: u16,
}

/// Checks everything the contract requires before a single byte is written.
fn plan<'a>(tiff: &'a Tiff<'a>) -> Result<Plan<'a>> {
    for tag in [
        TAG_TILE_WIDTH,
        TAG_TILE_LENGTH,
        TAG_TILE_OFFSETS,
        TAG_TILE_BYTE_COUNTS,
    ] {
        if tiff.entry(tag).is_some() {
            return Err(Error::refused(
                "tiff: tiled layout is not supported; only strips are rewritten",
            ));
        }
    }
    for tag in [
        TAG_SUB_IFDS,
        TAG_EXIF_IFD,
        TAG_GPS_IFD,
        TAG_INTEROP_IFD,
        TAG_JPEG_OFFSET,
    ] {
        if tiff.entry(tag).is_some() {
            return Err(Error::refused(format!(
                "tiff: tag {tag} carries an offset to another structure, which a rebuild would invalidate"
            )));
        }
    }

    let planar = tiff.scalar(TAG_PLANAR_CONFIG, Some(1))?;
    if planar != 1 {
        return Err(Error::refused(
            "tiff: separate planar configuration is not supported",
        ));
    }
    let predictor = tiff.scalar(TAG_PREDICTOR, Some(1))?;
    if predictor != 1 {
        return Err(Error::refused(format!(
            "tiff: predictor {predictor} is not supported; re-predicting a strip would change what its bytes mean"
        )));
    }

    let compression = tiff.scalar(TAG_COMPRESSION, Some(COMPRESSION_NONE as u32))? as u16;
    if !matches!(
        compression,
        COMPRESSION_NONE | COMPRESSION_DEFLATE | COMPRESSION_ADOBE_DEFLATE
    ) {
        return Err(Error::refused(format!(
            "tiff: compression {compression} is not rewritten by this build (only none and Deflate are)"
        )));
    }

    // Every out-of-line value must belong to a tag we know to be plain data.
    for entry in &tiff.entries {
        let size = type_size(entry.typ).unwrap_or(0);
        let len = size * entry.count as usize;
        if len > 4 && !DATA_TAGS.contains(&entry.tag) {
            return Err(Error::refused(format!(
                "tiff: tag {} has an out-of-line value this build cannot place safely",
                entry.tag
            )));
        }
    }

    let offsets = tiff.numbers(
        tiff.entry(TAG_STRIP_OFFSETS)
            .ok_or_else(|| Error::parse("tiff: StripOffsets is missing"))?,
    )?;
    let counts = tiff.numbers(
        tiff.entry(TAG_STRIP_BYTE_COUNTS)
            .ok_or_else(|| Error::parse("tiff: StripByteCounts is missing"))?,
    )?;
    if offsets.is_empty() || offsets.len() != counts.len() {
        return Err(Error::parse(
            "tiff: StripOffsets and StripByteCounts disagree on the strip count",
        ));
    }

    let mut strips = Vec::with_capacity(offsets.len());
    for (start, len) in offsets.iter().zip(counts.iter()) {
        let start = *start as usize;
        let len = *len as usize;
        let end = start
            .checked_add(len)
            .ok_or_else(|| Error::parse("tiff: a strip extent overflows"))?;
        let strip = tiff
            .data
            .get(start..end)
            .ok_or_else(|| Error::parse("tiff: a strip lies outside the file"))?;
        strips.push(strip);
    }

    // Anything outside the structures we re-place must be padding. A non-zero
    // byte we cannot account for is data we would silently drop.
    ensure_nothing_else(tiff, &offsets, &counts)?;

    Ok(Plan {
        tiff,
        compression,
        strips,
        new_compression: COMPRESSION_DEFLATE,
    })
}

/// Rejects files with non-zero bytes outside the IFD, the entries' values and
/// the strips: those bytes would be lost by the rebuild.
fn ensure_nothing_else(tiff: &Tiff<'_>, offsets: &[u32], counts: &[u32]) -> Result<()> {
    let mut ranges: Vec<(usize, usize)> = vec![(0, 8)]; // the header is always ours
    let ifd_len = 2 + tiff.entries.len() * 12 + 4;
    ranges.push((tiff.ifd, tiff.ifd + ifd_len));
    for entry in &tiff.entries {
        let size = type_size(entry.typ).unwrap_or(0);
        let len = size * entry.count as usize;
        if len > 4 {
            let offset = entry.value.as_ptr() as usize - tiff.data.as_ptr() as usize;
            ranges.push((offset, offset + len));
        }
    }
    for (start, len) in offsets.iter().zip(counts.iter()) {
        ranges.push((*start as usize, *start as usize + *len as usize));
    }
    ranges.sort_unstable();

    let mut covered_to = 0usize;
    for (start, end) in ranges {
        if start > covered_to {
            let gap = &tiff.data[covered_to..start];
            if gap.iter().any(|b| *b != 0) {
                return Err(Error::refused(
                    "tiff: the file contains data outside the IFD and the strips; a rebuild would lose it",
                ));
            }
        }
        covered_to = covered_to.max(end);
    }
    if covered_to < tiff.data.len() && tiff.data[covered_to..].iter().any(|b| *b != 0) {
        return Err(Error::refused(
            "tiff: trailing data after the last strip; a rebuild would lose it",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Compression
// ---------------------------------------------------------------------------

fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| Error::parse(format!("tiff: a Deflate strip does not decode: {e}")))?;
    if decoder.total_in() != data.len() as u64 {
        return Err(Error::parse(
            "tiff: trailing bytes after a Deflate strip; the file is corrupt",
        ));
    }
    Ok(out)
}

fn deflate(data: &[u8], zopfli: bool) -> Result<Vec<u8>> {
    if zopfli {
        #[cfg(feature = "zopfli")]
        {
            let mut raw = Vec::new();
            zopfli::compress(
                zopfli::Options::default(),
                zopfli::Format::Deflate,
                data,
                &mut raw,
            )
            .map_err(|e| Error::Internal(format!("zopfli failed: {e}")))?;
            // A TIFF strip is a zlib stream, and the encoder writes raw
            // DEFLATE, so the two-byte header and the Adler-32 trailer are
            // added here. Verification inflates every strip again, so a
            // wrapper mistake cannot reach the file.
            let mut out = Vec::with_capacity(raw.len() + 6);
            out.extend_from_slice(&[0x78, 0xda]);
            out.extend_from_slice(&raw);
            out.extend_from_slice(&adler32(data).to_be_bytes());
            return Ok(out);
        }
        #[cfg(not(feature = "zopfli"))]
        {
            let _ = zopfli;
        }
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

/// Adler-32, as the zlib trailer requires. Not a compressor: the build has a
/// crate for that, and this is fifteen lines of checksum.
#[cfg(feature = "zopfli")]
fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for byte in data {
        a = (a + *byte as u32) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

/// The decoded bytes of a strip, whatever its compression.
fn decode_strip(data: &[u8], compression: u16) -> Result<Vec<u8>> {
    match compression {
        COMPRESSION_NONE => Ok(data.to_vec()),
        _ => inflate(data),
    }
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

fn align2(value: usize) -> usize {
    value + (value & 1)
}

fn write_u16(out: &mut [u8], at: usize, value: u16, big: bool) {
    let bytes = if big {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    out[at..at + 2].copy_from_slice(&bytes);
}

fn write_u32(out: &mut [u8], at: usize, value: u32, big: bool) {
    let bytes = if big {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    out[at..at + 4].copy_from_slice(&bytes);
}

fn encode_numbers(values: &[u32], typ: u16, big: bool) -> Result<Vec<u8>> {
    let size =
        type_size(typ).ok_or_else(|| Error::internal(format!("tiff: cannot encode type {typ}")))?;
    let mut out = Vec::with_capacity(size * values.len());
    for value in values {
        match typ {
            TYPE_SHORT => {
                let small = u16::try_from(*value).map_err(|_| {
                    Error::refused(
                        "tiff: a rewritten value does not fit in the entry's SHORT field",
                    )
                })?;
                out.extend_from_slice(&if big {
                    small.to_be_bytes()
                } else {
                    small.to_le_bytes()
                });
            }
            TYPE_LONG => out.extend_from_slice(&if big {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            }),
            other => {
                return Err(Error::internal(format!(
                    "tiff: cannot encode numeric type {other}"
                )))
            }
        }
    }
    Ok(out)
}

struct OutEntry {
    tag: u16,
    typ: u16,
    count: u32,
    /// Value bytes; written inline when they fit in the entry's four value bytes.
    value: Vec<u8>,
}

fn build(plan: &Plan<'_>, strips: &[Vec<u8>], opts: &Options) -> Result<Vec<u8>> {
    let tiff = plan.tiff;
    let big = tiff.big;

    // Entries that are written, minus the metadata the user asked to drop.
    let kept: Vec<&Entry<'_>> = tiff
        .entries
        .iter()
        .filter(|e| survives(e.tag, opts.strip_metadata))
        .collect();
    if kept.len() > u16::MAX as usize {
        return Err(Error::refused(
            "tiff: the IFD has too many entries to rewrite",
        ));
    }

    let ifd_len = 2 + kept.len() * 12 + 4;
    let mut out = vec![0u8; 8 + ifd_len];

    // Header: same byte order, same magic, the IFD immediately after it.
    out[..2].copy_from_slice(if big { b"MM" } else { b"II" });
    write_u16(&mut out, 2, MAGIC_CLASSIC, big);
    write_u32(&mut out, 4, 8, big);
    write_u16(&mut out, 8, kept.len() as u16, big);

    let mut entries: Vec<OutEntry> = Vec::with_capacity(kept.len());
    for entry in &kept {
        let typ = entry.typ;
        let mut count = entry.count;
        let value = if entry.tag == TAG_STRIP_OFFSETS {
            // Placeholder: the offsets are known only after the strips are placed.
            let size = type_size(typ).unwrap_or(0);
            count = strips.len() as u32;
            vec![0u8; size * strips.len()]
        } else if entry.tag == TAG_STRIP_BYTE_COUNTS {
            let sizes: Vec<u32> = strips
                .iter()
                .map(|s| {
                    u32::try_from(s.len()).map_err(|_| {
                        Error::limit("tiff: a rewritten strip exceeds the format limit")
                    })
                })
                .collect::<Result<Vec<u32>>>()?;
            count = strips.len() as u32;
            encode_numbers(&sizes, typ, big)?
        } else if entry.tag == TAG_COMPRESSION {
            if typ != TYPE_SHORT || count != 1 {
                return Err(Error::refused(
                    "tiff: the Compression entry has an unexpected type or count",
                ));
            }
            encode_numbers(&[plan.new_compression as u32], typ, big)?
        } else {
            entry.value.to_vec()
        };
        entries.push(OutEntry {
            tag: entry.tag,
            typ,
            count,
            value,
        });
    }

    // Reserve space for the out-of-line values, in entry order.
    let mut slots: Vec<Option<usize>> = Vec::with_capacity(entries.len());
    for entry in &entries {
        if entry.value.len() > 4 {
            let at = align2(out.len());
            out.resize(at + entry.value.len(), 0);
            out[at..at + entry.value.len()].copy_from_slice(&entry.value);
            slots.push(Some(at));
        } else {
            slots.push(None);
        }
    }

    // Strips, two-byte aligned like every other value.
    let mut new_offsets: Vec<u32> = Vec::with_capacity(strips.len());
    for strip in strips {
        let at = align2(out.len());
        out.resize(at, 0);
        out.extend_from_slice(strip);
        new_offsets.push(u32::try_from(at).map_err(|_| {
            Error::limit("tiff: the rewritten file would exceed the 4 GiB TIFF limit")
        })?);
    }

    // Now the offsets are known: fill the array where it lives.
    let index = entries
        .iter()
        .position(|e| e.tag == TAG_STRIP_OFFSETS)
        .ok_or_else(|| Error::internal("tiff: StripOffsets vanished during the rebuild"))?;
    let encoded = encode_numbers(&new_offsets, entries[index].typ, big)?;
    match slots[index] {
        Some(at) => out[at..at + encoded.len()].copy_from_slice(&encoded),
        None => entries[index].value = encoded,
    }

    // Write the entries themselves.
    for (index, entry) in entries.iter().enumerate() {
        let at = 10 + index * 12;
        write_u16(&mut out, at, entry.tag, big);
        write_u16(&mut out, at + 2, entry.typ, big);
        write_u32(&mut out, at + 4, entry.count, big);
        match slots[index] {
            Some(slot) => {
                let slot = u32::try_from(slot)
                    .map_err(|_| Error::limit("tiff: the rewritten file exceeds 4 GiB"))?;
                write_u32(&mut out, at + 8, slot, big);
            }
            None => {
                let len = entry.value.len();
                out[at + 8..at + 8 + len].copy_from_slice(&entry.value);
            }
        }
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

pub fn optimize(input: &[u8], out: &mut dyn Write, opts: &Options) -> Result<Cand> {
    let tiff = parse(input)?;
    let plan = plan(&tiff)?;

    let expected = expected_strip_lengths(&tiff)?;
    let mut strips: Vec<Vec<u8>> = Vec::with_capacity(plan.strips.len());
    let mut any_encoded = false;

    for (index, strip) in plan.strips.iter().enumerate() {
        let decoded = decode_strip(strip, plan.compression)?;
        if let Some(Some(want)) = expected.get(index) {
            if decoded.len() != *want {
                return Err(Error::refused(format!(
                    "tiff: strip {index} decodes to {} bytes, expected {want}; the file is left alone",
                    decoded.len()
                )));
            }
        }
        let encoded = deflate(&decoded, opts.zopfli)?;
        // A strip that would grow keeps its original encoding — unless the file
        // is being converted, in which case every strip must be Deflate or the
        // Compression tag would lie.
        let chosen = if plan.compression == COMPRESSION_NONE || encoded.len() < strip.len() {
            encoded
        } else {
            strip.to_vec()
        };
        any_encoded |= chosen.len() < strip.len();
        strips.push(chosen);
    }

    if plan.compression == plan.new_compression && !any_encoded && !opts.strip_metadata {
        return Ok(Cand::NoGain);
    }

    let rebuilt = build(&plan, &strips, opts)?;
    if rebuilt.len() >= input.len() {
        return Ok(Cand::NoGain);
    }
    out.write_all(&rebuilt)?;
    Ok(Cand::Written(rebuilt.len() as u64))
}

/// The length each strip must decode to, from the geometry tags.
///
/// `None` for the last strip when the geometry cannot be expressed exactly; the
/// check is skipped then rather than guessed.
fn expected_strip_lengths(tiff: &Tiff<'_>) -> Result<Vec<Option<usize>>> {
    let width = tiff.scalar(TAG_IMAGE_WIDTH, None)? as usize;
    let length = tiff.scalar(TAG_IMAGE_LENGTH, None)? as usize;
    // A grayscale file may omit SamplesPerPixel; the default is one sample.
    let samples = tiff.scalar(TAG_SAMPLES_PER_PIXEL, Some(1))? as usize;
    let bits = tiff.numbers(
        tiff.entry(TAG_BITS_PER_SAMPLE)
            .ok_or_else(|| Error::parse("tiff: BitsPerSample is missing"))?,
    )?;
    let rows_per_strip = tiff.scalar(TAG_ROWS_PER_STRIP, Some(length as u32))? as usize;

    let bits_per_pixel: usize = if bits.len() == 1 {
        bits[0] as usize * samples
    } else {
        bits.iter().map(|b| *b as usize).sum()
    };
    if bits_per_pixel == 0 || rows_per_strip == 0 || samples == 0 {
        return Err(Error::parse("tiff: degenerate image geometry"));
    }
    let row_bytes = (width * bits_per_pixel).div_ceil(8);
    let strip_count = tiff
        .numbers(
            tiff.entry(TAG_STRIP_OFFSETS)
                .ok_or_else(|| Error::parse("tiff: StripOffsets is missing"))?,
        )?
        .len();

    let mut out = Vec::with_capacity(strip_count);
    for index in 0..strip_count {
        let first_row = index * rows_per_strip;
        if first_row >= length {
            out.push(None);
            continue;
        }
        let rows = rows_per_strip.min(length - first_row);
        out.push(Some(rows * row_bytes));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

fn number_of(value: &[u8], typ: u16, big: bool) -> Option<u32> {
    match typ {
        TYPE_SHORT if value.len() >= 2 => Some(if big {
            u16::from_be_bytes([value[0], value[1]]) as u32
        } else {
            u16::from_le_bytes([value[0], value[1]]) as u32
        }),
        TYPE_LONG if value.len() >= 4 => Some(if big {
            u32::from_be_bytes([value[0], value[1], value[2], value[3]])
        } else {
            u32::from_le_bytes([value[0], value[1], value[2], value[3]])
        }),
        _ => None,
    }
}

pub fn verify(original: &[u8], candidate: &[u8], opts: &Options) -> Result<()> {
    let before = parse(original).map_err(|e| Error::verify(format!("original: {e}")))?;
    let after = parse(candidate).map_err(|e| Error::verify(format!("candidate: {e}")))?;

    let wanted: Vec<&Entry<'_>> = before
        .entries
        .iter()
        .filter(|e| survives(e.tag, opts.strip_metadata))
        .collect();
    if wanted.len() != after.entries.len() {
        return Err(Error::verify("tiff: the entry count changed"));
    }

    for (a, b) in wanted.iter().zip(after.entries.iter()) {
        if a.tag != b.tag || a.typ != b.typ {
            return Err(Error::verify(format!(
                "tiff: entry {} became {} with type {}",
                a.tag, b.tag, b.typ
            )));
        }
        match a.tag {
            TAG_STRIP_OFFSETS | TAG_STRIP_BYTE_COUNTS => {
                if a.count as usize != after_strip_count(&after)? {
                    return Err(Error::verify(format!(
                        "tiff: the strip count of tag {} changed",
                        a.tag
                    )));
                }
            }
            TAG_COMPRESSION => {
                let was = number_of(a.value, a.typ, before.big)
                    .ok_or_else(|| Error::verify("tiff: unreadable Compression value"))?;
                let now = number_of(b.value, b.typ, after.big)
                    .ok_or_else(|| Error::verify("tiff: unreadable Compression value"))?;
                let allowed = was == now
                    || (was == COMPRESSION_NONE as u32 && now == COMPRESSION_DEFLATE as u32)
                    || (was == COMPRESSION_ADOBE_DEFLATE as u32
                        && now == COMPRESSION_DEFLATE as u32);
                if !allowed {
                    return Err(Error::verify(format!(
                        "tiff: compression changed from {was} to {now}"
                    )));
                }
            }
            _ => {
                if a.count != b.count || a.value != b.value {
                    return Err(Error::verify(format!(
                        "tiff: the value of tag {} was not preserved",
                        a.tag
                    )));
                }
            }
        }
    }

    // Content: every strip decoded and compared byte for byte.
    let plan_a = plan(&before).map_err(|e| Error::verify(format!("original: {e}")))?;
    let plan_b = plan(&after).map_err(|e| Error::verify(format!("candidate: {e}")))?;
    if plan_a.strips.len() != plan_b.strips.len() {
        return Err(Error::verify("tiff: the strip count changed"));
    }
    for (index, (sa, sb)) in plan_a.strips.iter().zip(plan_b.strips.iter()).enumerate() {
        let da = decode_strip(sa, plan_a.compression)
            .map_err(|e| Error::verify(format!("original strip {index}: {e}")))?;
        let db = decode_strip(sb, plan_b.compression)
            .map_err(|e| Error::verify(format!("candidate strip {index}: {e}")))?;
        if da != db {
            return Err(Error::verify(format!(
                "tiff: strip {index} decodes to different bytes"
            )));
        }
    }
    Ok(())
}

fn after_strip_count(tiff: &Tiff<'_>) -> Result<usize> {
    Ok(tiff
        .numbers(
            tiff.entry(TAG_STRIP_OFFSETS)
                .ok_or_else(|| Error::verify("tiff: StripOffsets vanished"))?,
        )?
        .len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_sizes_are_known() {
        assert_eq!(type_size(TYPE_SHORT), Some(2));
        assert_eq!(type_size(TYPE_LONG), Some(4));
        assert_eq!(type_size(TYPE_DOUBLE), Some(8));
        assert_eq!(type_size(13), None, "IFD is a BigTIFF type");
    }

    #[test]
    fn alignment_is_word_based() {
        assert_eq!(align2(8), 8);
        assert_eq!(align2(9), 10);
        assert_eq!(align2(0), 0);
    }

    #[test]
    fn metadata_tags_are_the_textual_ones() {
        assert!(is_stripped(270)); // ImageDescription
        assert!(is_stripped(305)); // Software
        assert!(is_stripped(306)); // DateTime
        assert!(is_stripped(33432)); // Copyright
        assert!(!is_stripped(TAG_IMAGE_WIDTH));
        assert!(!is_stripped(34675), "ICC changes how pixels are read");
        assert!(!is_stripped(320), "the colour map is content, not metadata");
    }

    #[cfg(feature = "zopfli")]
    #[test]
    fn adler32_matches_the_reference_value() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(b""), 1);
    }

    #[test]
    fn short_signed_reading_is_refused_not_guessed() {
        let tiff = Tiff {
            data: &[0, 0, 0, 0, 0, 0, 0, 0],
            big: false,
            ifd: 8,
            entries: Vec::new(),
        };
        let entry = Entry {
            tag: 1,
            typ: TYPE_SBYTE,
            count: 1,
            value: &[0],
        };
        assert!(tiff.numbers(&entry).is_err());
    }
}
