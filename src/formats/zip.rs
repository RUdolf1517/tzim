//! ZIP (APPNOTE 6.3.x) — v0.2.
//!
//! The archive is rewritten surgically: every structure is copied and only the
//! fields that must change are patched in place. Names, extra fields, comments,
//! flags, attributes, timestamps, compression methods and the entry order are
//! therefore preserved byte for byte, not reconstructed.
//!
//! Only DEFLATE entries are recompressed. A stored entry stays stored, which is
//! what keeps ODF/EPUB (`mimetype` must be stored) valid.
//!
//! `--strip-metadata` removes the archive comment, every entry comment and every
//! DOS timestamp, and nothing else: extra fields stay because some of them are
//! structure (`ZIP64`, the Unicode path) rather than decoration, and a rule that
//! guessed which was which would eventually delete a file's real name.
//!
//! Verification re-parses the candidate with the same strict reader and compares
//! every preserved field, then decompresses each entry from both files and
//! compares the payloads. The writer and the verifier agree on the file format,
//! which is the contract; neither can "confirm itself" on a corrupted output,
//! because a broken structure fails to parse at all.

use std::io::{self, Read, Write};

use crate::error::{Error, Result};

use super::{Cand, Options};

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const EOCD_SIG: u32 = 0x0605_4b50;
const DESCRIPTOR_SIG: u32 = 0x0807_4b50;

const FLAG_ENCRYPTED: u16 = 0x0001;
const FLAG_DESCRIPTOR: u16 = 0x0008;
const FLAG_STRONG_ENCRYPTION: u16 = 0x0040;
const FLAG_MASKED_HEADERS: u16 = 0x2000;

const METHOD_STORE: u16 = 0;
const METHOD_DEFLATE: u16 = 8;

const LOCAL_LEN: usize = 30;
const CENTRAL_LEN: usize = 46;
const EOCD_LEN: usize = 22;

/// Offset of the CRC-32 field inside a local header.
const LOCAL_CRC_OFFSET: usize = 14;
/// Offset of the compressed-size field inside a local header.
const LOCAL_COMP_OFFSET: usize = 18;
/// Offset of the uncompressed-size field inside a local header.
const LOCAL_UNCOMP_OFFSET: usize = 22;
/// Offset of the CRC-32 field inside a central directory record.
const CENTRAL_CRC_OFFSET: usize = 16;
/// Offset of the compressed-size field inside a central directory record.
const CENTRAL_COMP_OFFSET: usize = 20;
/// Offset of the uncompressed-size field inside a central directory record.
const CENTRAL_UNCOMP_OFFSET: usize = 24;
/// Offset of the local-header-offset field inside a central directory record.
const CENTRAL_LOCAL_OFFSET: usize = 42;

const MAX_ENTRIES: usize = 65_535;

// ---------------------------------------------------------------------------
// Small readers
// ---------------------------------------------------------------------------

fn read_u16(d: &[u8], at: usize) -> Result<u16> {
    let s = d
        .get(at..at + 2)
        .ok_or_else(|| Error::parse("zip: unexpected end of file"))?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn read_u32(d: &[u8], at: usize) -> Result<u32> {
    let s = d
        .get(at..at + 4)
        .ok_or_else(|| Error::parse("zip: unexpected end of file"))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u32_at(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

struct Descriptor {
    has_signature: bool,
}

struct Entry<'a> {
    /// The complete central directory record, patchable in place.
    central: &'a [u8],
    name: &'a [u8],
    central_extra: &'a [u8],
    comment: &'a [u8],
    version_made_by: u16,
    version_needed: u16,
    flags: u16,
    method: u16,
    crc: u32,
    comp_size: u32,
    uncomp_size: u32,
    local_offset: usize,

    /// The complete local header (signature, fixed fields, name, extra).
    local: &'a [u8],
    local_extra: &'a [u8],
    data_offset: usize,
    data_len: usize,
    descriptor: Option<Descriptor>,
    /// Bytes between this entry's end and the next structure. Only an APK
    /// signing block is allowed to create one, and it causes a refusal.
    gap_after: usize,
}

impl Entry<'_> {
    fn has_descriptor(&self) -> bool {
        self.flags & FLAG_DESCRIPTOR != 0
    }
}

struct Archive<'a> {
    eocd: &'a [u8],
    comment: &'a [u8],
    cd_offset: usize,
    entries: Vec<Entry<'a>>,
    /// Entry indices in physical (local header offset) order.
    physical: Vec<usize>,
}

fn find_eocd(d: &[u8]) -> Result<usize> {
    if d.len() < EOCD_LEN {
        return Err(Error::parse("zip: file is too short to be an archive"));
    }
    let lowest = d.len().saturating_sub(EOCD_LEN + u16::MAX as usize);
    let mut at = d.len() - EOCD_LEN;
    loop {
        if u32_at(d, at) == EOCD_SIG {
            let comment_len = read_u16(d, at + 20)? as usize;
            if at + EOCD_LEN + comment_len == d.len() {
                return Ok(at);
            }
        }
        if at == lowest {
            break;
        }
        at -= 1;
    }
    Err(Error::parse(
        "zip: no end-of-central-directory record found; the archive is truncated or not a ZIP file",
    ))
}

fn parse_archive(d: &[u8]) -> Result<Archive<'_>> {
    let eocd_offset = find_eocd(d)?;
    let eocd = &d[eocd_offset..];

    let disk = read_u16(d, eocd_offset + 4)?;
    let cd_disk = read_u16(d, eocd_offset + 6)?;
    let entries_this_disk = read_u16(d, eocd_offset + 8)?;
    let entries_total = read_u16(d, eocd_offset + 10)?;
    let cd_size = read_u32(d, eocd_offset + 12)?;
    let cd_offset = read_u32(d, eocd_offset + 16)?;
    let comment_len = read_u16(d, eocd_offset + 20)? as usize;
    let comment = &d[eocd_offset + EOCD_LEN..eocd_offset + EOCD_LEN + comment_len];

    if disk != 0 || cd_disk != 0 {
        return Err(Error::refused("zip: multi-disk archives are not supported"));
    }
    if entries_total == u16::MAX
        || cd_offset == u32::MAX
        || cd_size == u32::MAX
        || entries_this_disk == u16::MAX
    {
        return Err(Error::refused(
            "zip: ZIP64 archives are not supported in this version",
        ));
    }
    if entries_this_disk != entries_total {
        return Err(Error::parse(
            "zip: end-of-central-directory entry counts disagree",
        ));
    }
    if cd_offset as usize + cd_size as usize != eocd_offset {
        return Err(Error::parse(
            "zip: central directory size or offset is inconsistent",
        ));
    }
    if entries_total as usize > MAX_ENTRIES {
        return Err(Error::limit("zip: too many entries"));
    }

    let mut entries: Vec<Entry<'_>> = Vec::with_capacity(entries_total as usize);
    let mut at = cd_offset as usize;
    for _ in 0..entries_total {
        if read_u32(d, at)? != CENTRAL_SIG {
            return Err(Error::parse("zip: bad central directory signature"));
        }
        let version_made_by = read_u16(d, at + 4)?;
        let version_needed = read_u16(d, at + 6)?;
        let flags = read_u16(d, at + 8)?;
        let method = read_u16(d, at + 10)?;
        let crc = read_u32(d, at + 16)?;
        let comp_size = read_u32(d, at + 20)?;
        let uncomp_size = read_u32(d, at + 24)?;
        let name_len = read_u16(d, at + 28)? as usize;
        let extra_len = read_u16(d, at + 30)? as usize;
        let entry_comment_len = read_u16(d, at + 32)? as usize;
        let local_offset = read_u32(d, at + 42)? as usize;

        let total = CENTRAL_LEN + name_len + extra_len + entry_comment_len;
        let end = at
            .checked_add(total)
            .ok_or_else(|| Error::parse("zip: central directory offset overflows"))?;
        let record = d
            .get(at..end)
            .ok_or_else(|| Error::parse("zip: truncated central directory record"))?;
        let name = &record[CENTRAL_LEN..CENTRAL_LEN + name_len];
        let central_extra = &record[CENTRAL_LEN + name_len..CENTRAL_LEN + name_len + extra_len];
        let entry_comment = &record[CENTRAL_LEN + name_len + extra_len..];

        if name.is_empty() {
            return Err(Error::parse("zip: an entry has an empty name"));
        }
        if flags & (FLAG_ENCRYPTED | FLAG_STRONG_ENCRYPTION | FLAG_MASKED_HEADERS) != 0 {
            return Err(Error::refused("zip: encrypted archives are not supported"));
        }
        if method != METHOD_STORE && method != METHOD_DEFLATE {
            return Err(Error::refused(format!(
                "zip: unsupported compression method {method} (only store and deflate)"
            )));
        }
        if comp_size == u32::MAX || uncomp_size == u32::MAX || local_offset == u32::MAX as usize {
            return Err(Error::refused(
                "zip: ZIP64 archives are not supported in this version",
            ));
        }
        if has_zip64_extra(central_extra) {
            return Err(Error::refused(
                "zip: ZIP64 archives are not supported in this version",
            ));
        }

        entries.push(Entry {
            central: record,
            name,
            central_extra,
            comment: entry_comment,
            version_made_by,
            version_needed,
            flags,
            method,
            crc,
            comp_size,
            uncomp_size,
            local_offset,
            local: &[],
            local_extra: &[],
            data_offset: 0,
            data_len: 0,
            descriptor: None,
            gap_after: 0,
        });

        at += total;
    }
    if at != eocd_offset {
        return Err(Error::parse(
            "zip: central directory does not end where the end record begins",
        ));
    }

    // Local headers must appear in ascending order and cover the file from
    // offset zero up to the central directory, with no gaps we cannot explain.
    let mut physical: Vec<usize> = (0..entries.len()).collect();
    physical.sort_by_key(|&i| entries[i].local_offset);
    for pair in physical.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if entries[a].local_offset >= entries[b].local_offset {
            return Err(Error::parse("zip: two entries share a local header offset"));
        }
    }
    if let Some(&first) = physical.first() {
        if entries[first].local_offset != 0 {
            return Err(Error::refused(
                "zip: data before the first entry (self-extracting stub) is not supported",
            ));
        }
    }

    // The structure that follows each entry: the next local header, or the
    // central directory for the last one.
    let mut next_after = vec![cd_offset as usize; entries.len()];
    for pair in physical.windows(2) {
        next_after[pair[0]] = entries[pair[1]].local_offset;
    }

    for i in 0..entries.len() {
        let next = next_after[i];
        fill_local(d, &mut entries[i], next)?;
    }

    if entries.iter().any(|e| e.gap_after > 0) {
        if cd_offset as usize >= 16
            && &d[cd_offset as usize - 16..cd_offset as usize] == b"APK Sig Block 42"
        {
            return Err(Error::refused(
                "zip: signed APK (APK signing block present) — recompression would invalidate the signature",
            ));
        }
        return Err(Error::parse(
            "zip: unexpected bytes between an entry and the next structure",
        ));
    }

    Ok(Archive {
        eocd,
        comment,
        cd_offset: cd_offset as usize,
        entries,
        physical,
    })
}

fn has_zip64_extra(extra: &[u8]) -> bool {
    let mut at = 0;
    while at + 4 <= extra.len() {
        let tag = u16::from_le_bytes([extra[at], extra[at + 1]]);
        let size = u16::from_le_bytes([extra[at + 2], extra[at + 3]]) as usize;
        if tag == 0x0001 {
            return true;
        }
        at += 4 + size;
    }
    false
}

fn fill_local<'a>(d: &'a [u8], e: &mut Entry<'a>, next: usize) -> Result<()> {
    let at = e.local_offset;
    if read_u32(d, at)? != LOCAL_SIG {
        return Err(Error::parse(
            "zip: a local header offset does not point at a local header",
        ));
    }
    let flags = read_u16(d, at + 6)?;
    let method = read_u16(d, at + 8)?;
    let crc = read_u32(d, at + 14)?;
    let comp_size = read_u32(d, at + 18)?;
    let uncomp_size = read_u32(d, at + 22)?;
    let name_len = read_u16(d, at + 26)? as usize;
    let extra_len = read_u16(d, at + 28)? as usize;

    if flags != e.flags || method != e.method {
        return Err(Error::parse(
            "zip: local header disagrees with the central directory",
        ));
    }

    let header_end = at + LOCAL_LEN + name_len + extra_len;
    let header = d
        .get(at..header_end)
        .ok_or_else(|| Error::parse("zip: truncated local header"))?;
    let name = &header[LOCAL_LEN..LOCAL_LEN + name_len];
    if name != e.name {
        return Err(Error::parse(
            "zip: local header name disagrees with the central directory",
        ));
    }
    if has_zip64_extra(&header[LOCAL_LEN + name_len..]) {
        return Err(Error::parse(
            "zip: ZIP64 archives are not supported in this version",
        ));
    }

    let data_end = header_end
        .checked_add(e.comp_size as usize)
        .ok_or_else(|| Error::parse("zip: entry size overflows"))?;
    if data_end > next || header_end > next {
        return Err(Error::parse("zip: entry data overlaps the next structure"));
    }

    let descriptor = if e.has_descriptor() {
        let tail = next - data_end;
        let descriptor = match tail {
            12 => Descriptor {
                has_signature: false,
            },
            16 => {
                if read_u32(d, data_end)? != DESCRIPTOR_SIG {
                    return Err(Error::parse("zip: malformed data descriptor"));
                }
                Descriptor {
                    has_signature: true,
                }
            }
            _ => {
                return Err(Error::parse(
                    "zip: unexpected bytes between an entry and the next structure",
                ))
            }
        };
        // The descriptor repeats the sizes; they must agree with the central
        // directory, otherwise the archive is inconsistent.
        let base = data_end + if descriptor.has_signature { 4 } else { 0 };
        let desc_crc = read_u32(d, base)?;
        let desc_comp = read_u32(d, base + 4)?;
        let desc_uncomp = read_u32(d, base + 8)?;
        if desc_crc != e.crc || desc_comp != e.comp_size || desc_uncomp != e.uncomp_size {
            return Err(Error::parse(
                "zip: data descriptor disagrees with the central directory",
            ));
        }
        Some(descriptor)
    } else {
        if crc != e.crc || comp_size != e.comp_size || uncomp_size != e.uncomp_size {
            return Err(Error::parse(
                "zip: local header sizes disagree with the central directory",
            ));
        }
        // A gap is recorded rather than rejected here: the only legitimate one
        // is an APK signing block, which the caller turns into a refusal.
        e.gap_after = next - data_end;
        None
    };

    e.local = header;
    e.local_extra = &header[LOCAL_LEN + name_len..];
    e.data_offset = header_end;
    e.data_len = e.comp_size as usize;
    e.descriptor = descriptor;
    Ok(())
}

// ---------------------------------------------------------------------------
// Refusals that are not about structure
// ---------------------------------------------------------------------------

fn ascii_lower(b: u8) -> u8 {
    b.to_ascii_lowercase()
}

fn starts_with_ignore_case(name: &[u8], prefix: &str) -> bool {
    let p = prefix.as_bytes();
    name.len() >= p.len()
        && name[..p.len()]
            .iter()
            .map(|b| ascii_lower(*b))
            .eq(p.iter().map(|b| ascii_lower(*b)))
}

fn ends_with_ignore_case(name: &[u8], suffix: &str) -> bool {
    let s = suffix.as_bytes();
    name.len() >= s.len()
        && name[name.len() - s.len()..]
            .iter()
            .map(|b| ascii_lower(*b))
            .eq(s.iter().map(|b| ascii_lower(*b)))
}

/// Archive-level reasons to refuse, before any byte is written.
fn deny_reason(archive: &Archive<'_>, d: &[u8]) -> Option<String> {
    if archive.cd_offset >= 16
        && &d[archive.cd_offset - 16..archive.cd_offset] == b"APK Sig Block 42"
    {
        return Some(
            "zip: signed APK (APK signing block present) — recompression would invalidate the signature"
                .to_string(),
        );
    }

    for e in &archive.entries {
        let name = e.name;
        if starts_with_ignore_case(name, "META-INF/") {
            let tail = &name["META-INF/".len()..];
            if tail.iter().all(|b| *b != b'/')
                && (ends_with_ignore_case(tail, ".sf")
                    || ends_with_ignore_case(tail, ".rsa")
                    || ends_with_ignore_case(tail, ".dsa")
                    || ends_with_ignore_case(tail, ".ec")
                    || ends_with_ignore_case(tail, "documentsignatures.xml")
                    || ends_with_ignore_case(tail, "macrosignatures.xml"))
            {
                return Some(format!(
                    "zip: signed archive (`{}` present) — recompression would invalidate the signature",
                    String::from_utf8_lossy(name)
                ));
            }
        }
        if starts_with_ignore_case(name, "_xmlsignatures/") {
            return Some(
                "zip: signed OOXML package (`_xmlsignatures/`) — recompression would invalidate the signature"
                    .to_string(),
            );
        }
        if starts_with_ignore_case(name, "package/services/digital-signature/") {
            return Some(
                "zip: signed OPC package (digital signatures) — recompression would invalidate the signature"
                    .to_string(),
            );
        }
    }
    None
}

fn check_limits(archive: &Archive<'_>, opts: &Options) -> Result<()> {
    let mut uncompressed: u64 = 0;
    let mut compressed: u64 = 0;
    for e in &archive.entries {
        uncompressed = uncompressed.saturating_add(e.uncomp_size as u64);
        compressed = compressed.saturating_add(e.comp_size as u64);
    }
    if uncompressed > opts.max_uncompressed {
        return Err(Error::limit(format!(
            "zip: {uncompressed} bytes uncompressed exceeds --max-uncompressed ({})",
            opts.max_uncompressed
        )));
    }
    if compressed > 0 && uncompressed / compressed.max(1) > opts.max_ratio {
        return Err(Error::limit(format!(
            "zip: compression ratio {}:1 exceeds --max-ratio ({}:1); refusing a possible bomb",
            uncompressed / compressed.max(1),
            opts.max_ratio
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Recompression
// ---------------------------------------------------------------------------

/// Counts bytes and CRC-32 while data flows through, so an entry never has to be
/// materialised in memory to be validated.
struct CrcCounter<R> {
    inner: R,
    len: u64,
    hasher: crc32fast::Hasher,
}

impl<R: Read> CrcCounter<R> {
    fn new(inner: R) -> Self {
        CrcCounter {
            inner,
            len: 0,
            hasher: crc32fast::Hasher::new(),
        }
    }

    fn finish(self) -> (u64, u32) {
        (self.len, self.hasher.finalize())
    }
}

impl<R: Read> Read for CrcCounter<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hasher.update(&buf[..n]);
        self.len += n as u64;
        Ok(n)
    }
}

/// Recompresses a raw DEFLATE stream and reports the uncompressed length and
/// CRC-32 it produced, so the caller can validate the entry.
fn recompress(compressed: &[u8], zopfli: bool) -> Result<(Vec<u8>, u64, u32)> {
    let decoder = flate2::read::DeflateDecoder::new(compressed);

    if zopfli {
        #[cfg(feature = "zopfli")]
        {
            let mut source = CrcCounter::new(decoder);
            let mut out = Vec::new();
            zopfli::compress(
                zopfli::Options::default(),
                zopfli::Format::Deflate,
                &mut source,
                &mut out,
            )
            .map_err(|e| Error::Internal(format!("zopfli failed: {e}")))?;
            let (len, crc) = source.finish();
            return Ok((out, len, crc));
        }
        #[cfg(not(feature = "zopfli"))]
        {
            let _ = zopfli;
        }
    }

    let mut source = CrcCounter::new(decoder);
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(9));
    io::copy(&mut source, &mut encoder)?;
    let out = encoder.finish()?;
    let (len, crc) = source.finish();
    Ok((out, len, crc))
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

pub fn optimize(data: &[u8], dst: &mut dyn Write, opts: &Options) -> Result<Cand> {
    let archive = parse_archive(data)?;
    check_limits(&archive, opts)?;
    if let Some(reason) = deny_reason(&archive, data) {
        return Err(Error::refused(reason));
    }

    // Validate every entry's checksum before touching anything: a corrupt
    // archive is refused, not rewritten.
    for e in &archive.entries {
        if e.method == METHOD_STORE {
            let payload = &data[e.data_offset..e.data_offset + e.data_len];
            if payload.len() as u64 != e.uncomp_size as u64 {
                return Err(Error::parse("zip: stored entry size mismatch"));
            }
            if crc32fast::hash(payload) != e.crc {
                return Err(Error::parse(
                    "zip: stored entry CRC mismatch; the archive is corrupt",
                ));
            }
        }
    }

    let rebuilt = rebuild(data, &archive, opts)?;
    match rebuilt {
        // Zeroing timestamps removes metadata without removing bytes, so with the
        // flag set an equal-size rewrite is still the requested result.
        Some(out) if out.len() < data.len() || (opts.strip_metadata && out.len() == data.len()) => {
            let len = out.len() as u64;
            dst.write_all(&out)?;
            Ok(Cand::Written(len))
        }
        _ => Ok(Cand::NoGain),
    }
}

fn rebuild(d: &[u8], a: &Archive<'_>, opts: &Options) -> Result<Option<Vec<u8>>> {
    let strip = opts.strip_metadata;
    let count = a.entries.len();
    let mut out: Vec<u8> = Vec::with_capacity(d.len());
    let mut new_offsets = vec![0u32; count];
    let mut new_comp = vec![0u32; count];
    let mut new_crc = vec![0u32; count];
    let mut new_uncomp = vec![0u32; count];
    let mut changed = false;

    if strip {
        // An archive that is already bare is not rewritten for its own sake.
        if !a.comment.is_empty() {
            changed = true;
        }
        for e in &a.entries {
            if !e.comment.is_empty() || read_u16(e.local, 10)? != 0 || read_u16(e.local, 12)? != 0 {
                changed = true;
                break;
            }
        }
    }

    for &index in &a.physical {
        let e = &a.entries[index];
        let offset = u32::try_from(out.len())
            .map_err(|_| Error::limit("zip: rewritten archive would exceed 4 GiB"))?;
        new_offsets[index] = offset;

        let original = &d[e.data_offset..e.data_offset + e.data_len];
        let mut payload: Vec<u8> = original.to_vec();
        let mut crc = e.crc;
        let mut uncomp = e.uncomp_size;

        if e.method == METHOD_DEFLATE {
            // With nesting enabled the entry has to be decompressed anyway, so
            // the child file inside it can be optimized first.
            let nested = if opts.nested {
                match inflate_entry(original, e) {
                    Ok(child) => nested_child(&child, opts).unwrap_or(None),
                    Err(_) => None,
                }
            } else {
                None
            };

            let compressed = match &nested {
                Some((child, child_crc, child_len)) => {
                    let mut encoder =
                        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(9));
                    encoder.write_all(child)?;
                    let bytes = encoder.finish()?;
                    crc = *child_crc;
                    uncomp = *child_len;
                    bytes
                }
                None => {
                    let (bytes, len, c) = recompress(original, opts.zopfli)?;
                    if len != e.uncomp_size as u64 || c != e.crc {
                        return Err(Error::parse(format!(
                            "zip: entry `{}` fails its checksum; the archive is corrupt",
                            String::from_utf8_lossy(e.name)
                        )));
                    }
                    bytes
                }
            };

            // Use the new payload only if the entry itself got smaller; otherwise
            // the entry is kept bit for bit, including its checksum.
            if compressed.len() < original.len() {
                payload = compressed;
            } else {
                crc = e.crc;
                uncomp = e.uncomp_size;
            }
        }

        if payload != original || crc != e.crc || uncomp != e.uncomp_size {
            changed = true;
        }
        new_crc[index] = crc;
        new_uncomp[index] = uncomp;
        new_comp[index] = u32::try_from(payload.len())
            .map_err(|_| Error::limit("zip: entry would exceed 4 GiB"))?;

        let mut header = e.local.to_vec();
        if strip {
            header[10..14].fill(0);
        }
        if !e.has_descriptor() {
            header[LOCAL_CRC_OFFSET..LOCAL_CRC_OFFSET + 4].copy_from_slice(&crc.to_le_bytes());
            header[LOCAL_COMP_OFFSET..LOCAL_COMP_OFFSET + 4]
                .copy_from_slice(&new_comp[index].to_le_bytes());
            header[LOCAL_UNCOMP_OFFSET..LOCAL_UNCOMP_OFFSET + 4]
                .copy_from_slice(&uncomp.to_le_bytes());
        }
        out.extend_from_slice(&header);
        out.extend_from_slice(&payload);

        if let Some(descriptor) = &e.descriptor {
            if descriptor.has_signature {
                out.extend_from_slice(&DESCRIPTOR_SIG.to_le_bytes());
            }
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&new_comp[index].to_le_bytes());
            out.extend_from_slice(&uncomp.to_le_bytes());
        }
    }

    let cd_offset = u32::try_from(out.len())
        .map_err(|_| Error::limit("zip: rewritten archive would exceed 4 GiB"))?;
    for (index, e) in a.entries.iter().enumerate() {
        let mut record = e.central.to_vec();
        if strip {
            record[12..16].fill(0);
            record.truncate(CENTRAL_LEN + e.name.len() + e.central_extra.len());
            record[32..34].copy_from_slice(&0u16.to_le_bytes());
        }
        record[CENTRAL_CRC_OFFSET..CENTRAL_CRC_OFFSET + 4]
            .copy_from_slice(&new_crc[index].to_le_bytes());
        record[CENTRAL_COMP_OFFSET..CENTRAL_COMP_OFFSET + 4]
            .copy_from_slice(&new_comp[index].to_le_bytes());
        record[CENTRAL_UNCOMP_OFFSET..CENTRAL_UNCOMP_OFFSET + 4]
            .copy_from_slice(&new_uncomp[index].to_le_bytes());
        record[CENTRAL_LOCAL_OFFSET..CENTRAL_LOCAL_OFFSET + 4]
            .copy_from_slice(&new_offsets[index].to_le_bytes());
        out.extend_from_slice(&record);
    }
    let cd_size = u32::try_from(out.len() - cd_offset as usize)
        .map_err(|_| Error::limit("zip: central directory would exceed 4 GiB"))?;

    let mut eocd = if strip {
        a.eocd[..EOCD_LEN].to_vec()
    } else {
        a.eocd.to_vec()
    };
    if strip {
        eocd[20..22].copy_from_slice(&0u16.to_le_bytes());
    }
    eocd[12..16].copy_from_slice(&cd_size.to_le_bytes());
    eocd[16..20].copy_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&eocd);

    if changed {
        Ok(Some(out))
    } else {
        Ok(None)
    }
}

/// Decompresses a DEFLATE entry completely and validates its checksum.
fn inflate_entry(compressed: &[u8], e: &Entry<'_>) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::DeflateDecoder::new(compressed);
    let mut out = Vec::with_capacity(e.uncomp_size as usize);
    decoder.read_to_end(&mut out)?;
    if out.len() as u64 != e.uncomp_size as u64 || crc32fast::hash(&out) != e.crc {
        return Err(Error::parse(format!(
            "zip: entry `{}` fails its checksum; the archive is corrupt",
            String::from_utf8_lossy(e.name)
        )));
    }
    Ok(out)
}

/// Optimizes a file stored inside an entry, if its content is a supported format.
///
/// Returns the new content with its CRC and length only when the result is
/// strictly smaller **and** passes the child's own contract. Any other outcome —
/// including every error — means "keep the original bytes": a nested failure must
/// never fail the parent archive.
fn nested_child(child: &[u8], opts: &Options) -> Result<Option<(Vec<u8>, u32, u32)>> {
    if opts.depth >= opts.max_depth {
        return Ok(None);
    }
    // Defence in depth, and it cannot fire from `optimize`: `check_limits` has
    // already refused the archive when the sum of the declared uncompressed
    // sizes exceeds `--max-uncompressed`, and `inflate_entry` refuses an entry
    // whose data disagrees with its own declared size — so a child handed to
    // this function is never larger than the budget (#49, T-NEST-04). The
    // budget is a limit on the archive, not a running total shared by children;
    // there is no "remaining budget" to test.
    if child.len() as u64 > opts.max_uncompressed {
        return Ok(None);
    }
    let format = match crate::detect::detect(child) {
        Some(f) if opts.allow.permits(f) => f,
        _ => return Ok(None),
    };
    let deeper = Options {
        depth: opts.depth + 1,
        ..*opts
    };
    let mut candidate = Vec::new();
    if super::optimize(format, child, &mut candidate, &deeper).is_err() {
        return Ok(None);
    }
    if candidate.is_empty() || candidate.len() >= child.len() {
        return Ok(None);
    }
    if super::verify(format, child, &candidate, &deeper).is_err() {
        return Ok(None);
    }
    let crc = crc32fast::hash(&candidate);
    let len = u32::try_from(candidate.len())
        .map_err(|_| Error::limit("zip: nested entry would exceed 4 GiB"))?;
    Ok(Some((candidate, crc, len)))
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

/// Zeroes the fields a rewrite is allowed to change, so the rest can be compared
/// byte for byte.
/// Masks the fields a rewrite may change. `checksum` is set when a nested
/// rewrite legitimately replaced the content, which also changes the stored CRC
/// and the uncompressed size.
fn mask_local(header: &[u8], checksum: bool, strip: bool) -> Vec<u8> {
    let mut copy = header.to_vec();
    if strip && copy.len() >= 14 {
        // DOS time and date, removed on request and therefore not compared.
        copy[10..14].fill(0);
    }
    if copy.len() >= LOCAL_COMP_OFFSET + 4 {
        copy[LOCAL_COMP_OFFSET..LOCAL_COMP_OFFSET + 4].fill(0);
    }
    if checksum && copy.len() >= LOCAL_UNCOMP_OFFSET + 4 {
        copy[LOCAL_CRC_OFFSET..LOCAL_CRC_OFFSET + 4].fill(0);
        copy[LOCAL_UNCOMP_OFFSET..LOCAL_UNCOMP_OFFSET + 4].fill(0);
    }
    copy
}

fn mask_central(record: &[u8], checksum: bool, strip: bool) -> Vec<u8> {
    let mut copy = record.to_vec();
    if strip && copy.len() >= CENTRAL_LEN {
        let name_len = u16::from_le_bytes([copy[28], copy[29]]) as usize;
        let extra_len = u16::from_le_bytes([copy[30], copy[31]]) as usize;
        let keep = CENTRAL_LEN + name_len + extra_len;
        if keep <= copy.len() {
            copy.truncate(keep);
        }
        copy[12..16].fill(0);
        copy[32..34].fill(0);
    }
    if copy.len() >= CENTRAL_LOCAL_OFFSET + 4 {
        copy[CENTRAL_COMP_OFFSET..CENTRAL_COMP_OFFSET + 4].fill(0);
        copy[CENTRAL_LOCAL_OFFSET..CENTRAL_LOCAL_OFFSET + 4].fill(0);
    }
    if checksum && copy.len() >= CENTRAL_UNCOMP_OFFSET + 4 {
        copy[CENTRAL_CRC_OFFSET..CENTRAL_CRC_OFFSET + 4].fill(0);
        copy[CENTRAL_UNCOMP_OFFSET..CENTRAL_UNCOMP_OFFSET + 4].fill(0);
    }
    copy
}

pub fn verify(a: &[u8], b: &[u8], opts: &Options) -> Result<()> {
    let pa = parse_archive(a).map_err(|e| Error::verify(format!("original: {e}")))?;
    let pb = parse_archive(b).map_err(|e| Error::verify(format!("candidate: {e}")))?;
    let strip = opts.strip_metadata;

    if pa.entries.len() != pb.entries.len() {
        return Err(Error::verify("entry count changed"));
    }
    if strip {
        if !pb.comment.is_empty() {
            return Err(Error::verify("archive comment survived --strip-metadata"));
        }
    } else if pa.comment != pb.comment {
        return Err(Error::verify("archive comment was not preserved"));
    }

    for (index, (ea, eb)) in pa.entries.iter().zip(pb.entries.iter()).enumerate() {
        let label = String::from_utf8_lossy(ea.name).into_owned();
        if ea.name != eb.name {
            return Err(Error::verify(format!("entry {index}: name changed")));
        }
        if ea.method != eb.method {
            return Err(Error::verify(format!(
                "entry `{label}`: compression method changed"
            )));
        }
        if ea.flags != eb.flags {
            return Err(Error::verify(format!("entry `{label}`: flags changed")));
        }
        if ea.crc != eb.crc || ea.uncomp_size != eb.uncomp_size {
            // Only a nested rewrite may change these, and only if the child is a
            // supported format that got smaller and still satisfies its contract.
            let data_a = &a[ea.data_offset..ea.data_offset + ea.data_len];
            let data_b = &b[eb.data_offset..eb.data_offset + eb.data_len];
            verify_nested_change(data_a, data_b, ea.method, &label, opts)?;
        }
        if ea.version_made_by != eb.version_made_by || ea.version_needed != eb.version_needed {
            return Err(Error::verify(format!(
                "entry `{label}`: version fields changed"
            )));
        }
        if ea.central_extra != eb.central_extra || ea.local_extra != eb.local_extra {
            return Err(Error::verify(format!(
                "entry `{label}`: extra fields changed"
            )));
        }
        if strip {
            if !eb.comment.is_empty() {
                return Err(Error::verify(format!(
                    "entry `{label}`: comment survived --strip-metadata"
                )));
            }
            if read_u16(eb.local, 10)? != 0
                || read_u16(eb.local, 12)? != 0
                || read_u16(eb.central, 12)? != 0
                || read_u16(eb.central, 14)? != 0
            {
                return Err(Error::verify(format!(
                    "entry `{label}`: timestamp survived --strip-metadata"
                )));
            }
        } else if ea.comment != eb.comment {
            return Err(Error::verify(format!("entry `{label}`: comment changed")));
        }
        let checksum_changed = ea.crc != eb.crc || ea.uncomp_size != eb.uncomp_size;
        if mask_local(ea.local, checksum_changed, strip)
            != mask_local(eb.local, checksum_changed, strip)
        {
            return Err(Error::verify(format!(
                "entry `{label}`: local header changed outside the allowed fields"
            )));
        }
        if mask_central(ea.central, checksum_changed, strip)
            != mask_central(eb.central, checksum_changed, strip)
        {
            return Err(Error::verify(format!(
                "entry `{label}`: central directory record changed outside the allowed fields"
            )));
        }
        if ea.descriptor.is_some() != eb.descriptor.is_some() {
            return Err(Error::verify(format!(
                "entry `{label}`: data descriptor was added or removed"
            )));
        }

        if ea.crc == eb.crc && ea.uncomp_size == eb.uncomp_size {
            let data_a = &a[ea.data_offset..ea.data_offset + ea.data_len];
            let data_b = &b[eb.data_offset..eb.data_offset + eb.data_len];
            compare_payload(data_a, data_b, ea.method, &label)?;
        }
    }

    Ok(())
}

/// A nested entry's checksum may change only when the content itself is a
/// supported format that got strictly smaller and still verifies.
fn verify_nested_change(
    data_a: &[u8],
    data_b: &[u8],
    method: u16,
    label: &str,
    opts: &Options,
) -> Result<()> {
    if method != METHOD_DEFLATE {
        return Err(Error::verify(format!(
            "entry `{label}`: checksum changed but the entry is not deflated"
        )));
    }
    let child_a = inflate_raw(data_a).map_err(|e| {
        Error::verify(format!(
            "entry `{label}`: original content does not inflate: {e}"
        ))
    })?;
    let child_b = inflate_raw(data_b).map_err(|e| {
        Error::verify(format!(
            "entry `{label}`: candidate content does not inflate: {e}"
        ))
    })?;
    if child_b.len() >= child_a.len() {
        return Err(Error::verify(format!(
            "entry `{label}`: content changed but did not get smaller"
        )));
    }
    let format = crate::detect::detect(&child_a).ok_or_else(|| {
        Error::verify(format!(
            "entry `{label}`: content changed but is not a supported format"
        ))
    })?;
    super::verify(format, &child_a, &child_b, opts)
        .map_err(|e| Error::verify(format!("entry `{label}`: nested content: {e}")))
}

fn inflate_raw(compressed: &[u8]) -> io::Result<Vec<u8>> {
    let mut decoder = flate2::read::DeflateDecoder::new(compressed);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn compare_payload(data_a: &[u8], data_b: &[u8], method: u16, label: &str) -> Result<()> {
    match method {
        METHOD_STORE => {
            if data_a != data_b {
                return Err(Error::verify(format!(
                    "entry `{label}`: stored data changed"
                )));
            }
        }
        _ => {
            let mut ra = flate2::read::DeflateDecoder::new(data_a);
            let mut rb = flate2::read::DeflateDecoder::new(data_b);
            let mut buf_a = [0u8; 64 * 1024];
            let mut buf_b = [0u8; 64 * 1024];
            loop {
                let na = read_fully(&mut ra, &mut buf_a)?;
                let nb = read_fully(&mut rb, &mut buf_b)?;
                if na != nb || buf_a[..na] != buf_b[..nb] {
                    return Err(Error::verify(format!(
                        "entry `{label}`: decompressed payload differs"
                    )));
                }
                if na == 0 {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn read_fully(reader: &mut impl Read, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) => return Err(Error::Io(e)),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_matching_ignores_case() {
        assert!(ends_with_ignore_case(b"META-INF/FOO.SF", ".sf"));
        assert!(starts_with_ignore_case(b"meta-inf/foo.rsa", "META-INF/"));
        assert!(!ends_with_ignore_case(b"META-INF/MANIFEST.MF", ".sf"));
        assert!(!starts_with_ignore_case(b"other/", "META-INF/"));
    }

    #[test]
    fn zip64_extra_is_detected() {
        let mut extra = Vec::new();
        extra.extend_from_slice(&0x5455u16.to_le_bytes()); // extended timestamp
        extra.extend_from_slice(&5u16.to_le_bytes());
        extra.extend_from_slice(&[0, 0, 0, 0, 0]);
        assert!(!has_zip64_extra(&extra));

        let mut with_zip64 = extra.clone();
        with_zip64.extend_from_slice(&0x0001u16.to_le_bytes());
        with_zip64.extend_from_slice(&8u16.to_le_bytes());
        with_zip64.extend_from_slice(&[0; 8]);
        assert!(has_zip64_extra(&with_zip64));
    }

    #[test]
    fn masking_keeps_preserved_bytes() {
        let mut local = vec![0u8; LOCAL_LEN];
        local[LOCAL_COMP_OFFSET] = 0xaa;
        let masked = mask_local(&local, false, false);
        assert_eq!(masked[LOCAL_COMP_OFFSET], 0);
        assert_eq!(local[LOCAL_COMP_OFFSET], 0xaa);
    }

    #[test]
    fn short_input_is_refused() {
        assert!(parse_archive(b"PK\x05\x06").is_err());
        assert!(find_eocd(&[]).is_err());
    }

    #[test]
    fn eocd_without_entries_parses() {
        let mut d = Vec::new();
        d.extend_from_slice(&EOCD_SIG.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        let archive = parse_archive(&d).unwrap();
        assert!(archive.entries.is_empty());
    }
}
