//! Fixture builders shared by the integration tests.
//!
//! Fixtures are generated in code rather than committed as binaries: each one is
//! described by what it contains, so a failing test points at a known structure.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// A temporary directory removed when the guard is dropped.
pub struct Workspace {
    pub path: PathBuf,
}

impl Workspace {
    pub fn new(label: &str) -> Workspace {
        let unique = format!(
            "tzim-it-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).expect("create workspace");
        Workspace { path }
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    pub fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.file(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&path, bytes).expect("write fixture");
        path
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub fn chunk(typ: &[u8; 4], data: &[u8]) -> Vec<u8> {
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

pub struct PngSpec {
    pub width: u32,
    pub height: u32,
    pub filter: u8,
    pub level: u32,
    pub extra_chunks: Vec<(String, Vec<u8>)>,
    /// Adds an `acTL` chunk, turning the file into an APNG.
    pub apng: bool,
}

impl Default for PngSpec {
    fn default() -> Self {
        PngSpec {
            width: 32,
            height: 32,
            filter: 0,
            level: 6,
            extra_chunks: Vec::new(),
            apng: false,
        }
    }
}

/// Builds a truecolor+alpha, 8-bit, non-interlaced PNG.
pub fn build_png(spec: &PngSpec) -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&spec.width.to_be_bytes());
    ihdr.extend_from_slice(&spec.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);

    let mut raw = Vec::new();
    for y in 0..spec.height as usize {
        raw.push(spec.filter);
        for x in 0..spec.width as usize {
            // A gradient plus structure so filtering has something to work with.
            raw.push(((x * 255) / spec.width as usize) as u8);
            raw.push(((y * 255) / spec.height as usize) as u8);
            raw.push(((x + y) % 251) as u8);
            raw.push(0xff);
        }
    }

    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(spec.level));
    encoder.write_all(&raw).unwrap();
    let idat = encoder.finish().unwrap();

    let mut out = PNG_SIGNATURE.to_vec();
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));
    for (typ, data) in &spec.extra_chunks {
        let mut t = [0u8; 4];
        t.copy_from_slice(typ.as_bytes());
        out.extend_from_slice(&chunk(&t, data));
    }
    if spec.apng {
        out.extend_from_slice(&chunk(b"acTL", &actl_payload(1)));
        out.extend_from_slice(&chunk(b"fcTL", &fctl_payload(0, spec.width, spec.height)));
    }
    out.extend_from_slice(&chunk(b"IDAT", &idat));
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
}

/// Builds a gzip member, optionally with FNAME and FEXTRA fields.
pub fn build_gzip(
    payload: &[u8],
    name: Option<&[u8]>,
    extra: Option<&[u8]>,
    xfl: u8,
    level: u32,
) -> Vec<u8> {
    let mut flags = 0u8;
    if name.is_some() {
        flags |= 0x08;
    }
    if extra.is_some() {
        flags |= 0x04;
    }
    // MTIME is deliberately non-zero: preserving it is part of the contract.
    let mut out = vec![0x1f, 0x8b, 0x08, flags, 0x12, 0x34, 0x56, 0x78, xfl, 0x03];
    if let Some(x) = extra {
        out.extend_from_slice(&(x.len() as u16).to_le_bytes());
        out.extend_from_slice(x);
    }
    if let Some(n) = name {
        out.extend_from_slice(n);
        out.push(0);
    }
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(level));
    encoder.write_all(payload).unwrap();
    out.extend_from_slice(&encoder.finish().unwrap());
    out.extend_from_slice(&crc32fast::hash(payload).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out
}

/// Splits a PNG into `(type, payload)` pairs, validating framing and CRCs.
pub fn png_chunks(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    if data.len() < 8 || data[..8] != PNG_SIGNATURE {
        return Err("bad signature".into());
    }
    let mut out = Vec::new();
    let mut pos = 8;
    while pos < data.len() {
        if data.len() - pos < 12 {
            return Err("truncated".into());
        }
        let len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let end = pos + 12 + len;
        if end > data.len() {
            return Err("truncated".into());
        }
        let typ = String::from_utf8_lossy(&data[pos + 4..pos + 8]).into_owned();
        let payload = data[pos + 8..pos + 8 + len].to_vec();
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&data[pos + 4..pos + 8]);
        hasher.update(&payload);
        let stored = u32::from_be_bytes([data[end - 4], data[end - 3], data[end - 2], data[end - 1]]);
        if hasher.finalize() != stored {
            return Err(format!("bad CRC in {typ}"));
        }
        out.push((typ, payload));
        pos = end;
    }
    Ok(out)
}

/// Decodes all pixels of a PNG (identity transformations).
pub fn decode_pixels(data: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("image too large")?;
    let mut buffer = vec![0u8; size];
    let info = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
    buffer.truncate(info.buffer_size());
    Ok((info.width, info.height, buffer))
}

/// Runs the binary and returns (exit code, stdout, stderr).
pub fn run(args: &[&str], cwd: &Path) -> (i32, String, String) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tzim"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("failed to run tzim");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Runs the binary with a path argument that may not be valid UTF-8.
pub fn run_os(args: &[std::ffi::OsString], cwd: &Path) -> (i32, String, String) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tzim"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("failed to run tzim");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Number of `"type":"file"` records in NDJSON output.
pub fn json_file_records(stdout: &str) -> usize {
    stdout
        .lines()
        .filter(|l| l.contains("\"type\":\"file\""))
        .count()
}

// ---------------------------------------------------------------------------
// ZIP fixtures
// ---------------------------------------------------------------------------

pub const ZIP_LOCAL_SIG: u32 = 0x0403_4b50;
pub const ZIP_CENTRAL_SIG: u32 = 0x0201_4b50;
pub const ZIP_EOCD_SIG: u32 = 0x0605_4b50;
pub const ZIP_DESCRIPTOR_SIG: u32 = 0x0807_4b50;

pub struct ZipEntrySpec {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    /// 0 = store, 8 = deflate.
    pub method: u16,
    /// Deflate level used for the fixture, so a rewrite has room to improve.
    pub level: u32,
    /// Sets general purpose bit 3 and writes a data descriptor.
    pub descriptor: bool,
    /// Whether the descriptor carries its optional signature.
    pub descriptor_signature: bool,
    pub local_extra: Vec<u8>,
    pub central_extra: Vec<u8>,
    pub comment: Vec<u8>,
}

impl ZipEntrySpec {
    pub fn deflated(name: &str, data: &[u8], level: u32) -> ZipEntrySpec {
        ZipEntrySpec {
            name: name.as_bytes().to_vec(),
            data: data.to_vec(),
            method: 8,
            level,
            descriptor: false,
            descriptor_signature: true,
            local_extra: Vec::new(),
            central_extra: Vec::new(),
            comment: Vec::new(),
        }
    }

    pub fn stored(name: &str, data: &[u8]) -> ZipEntrySpec {
        ZipEntrySpec {
            method: 0,
            ..ZipEntrySpec::deflated(name, data, 0)
        }
    }
}

fn deflate_bytes(data: &[u8], level: u32) -> Vec<u8> {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(level));
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// Builds a ZIP archive. `flags_extra` is OR-ed into every entry's flags, which
/// lets a test create an "encrypted" archive without a real cipher.
pub fn build_zip(entries: &[ZipEntrySpec], archive_comment: &[u8]) -> Vec<u8> {
    build_zip_with_flags(entries, archive_comment, 0)
}

pub fn build_zip_with_flags(
    entries: &[ZipEntrySpec],
    archive_comment: &[u8],
    flags_extra: u16,
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();

    for spec in entries {
        let crc = crc32fast::hash(&spec.data);
        let payload = if spec.method == 8 {
            deflate_bytes(&spec.data, spec.level)
        } else {
            spec.data.clone()
        };

        let mut flags = flags_extra;
        if spec.descriptor {
            flags |= 0x0008;
        }

        let local_offset = out.len() as u32;
        out.extend_from_slice(&ZIP_LOCAL_SIG.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&spec.method.to_le_bytes());
        out.extend_from_slice(&0x6000u16.to_le_bytes()); // time
        out.extend_from_slice(&0x5a21u16.to_le_bytes()); // date (2025-01-01)
        out.extend_from_slice(&crc.to_le_bytes());
        let (stored_comp, stored_uncomp) = if spec.descriptor {
            (0u32, 0u32)
        } else {
            (payload.len() as u32, spec.data.len() as u32)
        };
        out.extend_from_slice(&stored_comp.to_le_bytes());
        out.extend_from_slice(&stored_uncomp.to_le_bytes());
        out.extend_from_slice(&(spec.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(spec.local_extra.len() as u16).to_le_bytes());
        out.extend_from_slice(&spec.name);
        out.extend_from_slice(&spec.local_extra);
        out.extend_from_slice(&payload);

        if spec.descriptor {
            if spec.descriptor_signature {
                out.extend_from_slice(&ZIP_DESCRIPTOR_SIG.to_le_bytes());
            }
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&(spec.data.len() as u32).to_le_bytes());
        }

        central.extend_from_slice(&ZIP_CENTRAL_SIG.to_le_bytes());
        central.extend_from_slice(&0x031eu16.to_le_bytes()); // made by: unix, 3.0
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&flags.to_le_bytes());
        central.extend_from_slice(&spec.method.to_le_bytes());
        central.extend_from_slice(&0x6000u16.to_le_bytes());
        central.extend_from_slice(&0x5a21u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        central.extend_from_slice(&(spec.data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(spec.name.len() as u16).to_le_bytes());
        central.extend_from_slice(&(spec.central_extra.len() as u16).to_le_bytes());
        central.extend_from_slice(&(spec.comment.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // disk start
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0x81a4_0000u32.to_le_bytes()); // -rw-r--r--
        central.extend_from_slice(&local_offset.to_le_bytes());
        central.extend_from_slice(&spec.name);
        central.extend_from_slice(&spec.central_extra);
        central.extend_from_slice(&spec.comment);
    }

    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);

    out.extend_from_slice(&ZIP_EOCD_SIG.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&(archive_comment.len() as u16).to_le_bytes());
    out.extend_from_slice(archive_comment);
    out
}

#[derive(Debug, Clone)]
pub struct ParsedZipEntry {
    pub name: Vec<u8>,
    pub method: u16,
    pub flags: u16,
    pub crc: u32,
    pub comp_size: u32,
    pub uncomp_size: u32,
    pub local_extra: Vec<u8>,
    pub central_extra: Vec<u8>,
    pub comment: Vec<u8>,
    pub local_offset: u32,
}

fn le16(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([d[at], d[at + 1]])
}

fn le32(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
}

impl ParsedZipEntry {
    /// The stored bytes of this entry, located through the local header.
    pub fn payload<'a>(&self, archive: &'a [u8]) -> &'a [u8] {
        let at = self.local_offset as usize;
        let name_len = le16(archive, at + 26) as usize;
        let extra_len = le16(archive, at + 28) as usize;
        let start = at + 30 + name_len + extra_len;
        &archive[start..start + self.comp_size as usize]
    }

    pub fn decompressed(&self, archive: &[u8]) -> Vec<u8> {
        let payload = self.payload(archive);
        if self.method == 8 {
            let mut decoder = flate2::read::DeflateDecoder::new(payload);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out).unwrap();
            out
        } else {
            payload.to_vec()
        }
    }
}

/// Parses an archive through its central directory, validating what it can.
pub fn parse_zip(d: &[u8]) -> Vec<ParsedZipEntry> {
    let eocd = (0..=d.len().saturating_sub(22))
        .rev()
        .find(|&at| le32(d, at) == ZIP_EOCD_SIG)
        .expect("no end-of-central-directory record");
    let count = le16(d, eocd + 10);
    let cd_offset = le32(d, eocd + 16) as usize;

    let mut entries = Vec::with_capacity(count as usize);
    let mut at = cd_offset;
    for _ in 0..count {
        assert_eq!(le32(d, at), ZIP_CENTRAL_SIG, "bad central signature");
        let name_len = le16(d, at + 28) as usize;
        let extra_len = le16(d, at + 30) as usize;
        let comment_len = le16(d, at + 32) as usize;
        let name = d[at + 46..at + 46 + name_len].to_vec();
        let central_extra = d[at + 46 + name_len..at + 46 + name_len + extra_len].to_vec();
        let comment = d[at + 46 + name_len + extra_len..at + 46 + name_len + extra_len + comment_len]
            .to_vec();
        let local_offset = le32(d, at + 42);
        let lh = local_offset as usize;
        assert_eq!(le32(d, lh), ZIP_LOCAL_SIG, "bad local signature");
        let l_name = le16(d, lh + 26) as usize;
        let l_extra = le16(d, lh + 28) as usize;
        let local_extra = d[lh + 30 + l_name..lh + 30 + l_name + l_extra].to_vec();

        entries.push(ParsedZipEntry {
            name,
            method: le16(d, at + 10),
            flags: le16(d, at + 8),
            crc: le32(d, at + 16),
            comp_size: le32(d, at + 20),
            uncomp_size: le32(d, at + 24),
            local_extra,
            central_extra,
            comment,
            local_offset,
        });
        at += 46 + name_len + extra_len + comment_len;
    }
    entries
}

/// The archive comment, as stored in the end-of-central-directory record.
pub fn zip_archive_comment(d: &[u8]) -> Vec<u8> {
    let eocd = (0..=d.len().saturating_sub(22))
        .rev()
        .find(|&at| le32(d, at) == ZIP_EOCD_SIG)
        .expect("no end-of-central-directory record");
    let len = le16(d, eocd + 20) as usize;
    d[eocd + 22..eocd + 22 + len].to_vec()
}

// ---------------------------------------------------------------------------
// ICO fixtures
// ---------------------------------------------------------------------------

pub struct IcoEntrySpec {
    /// Width in pixels (0 means 256, as the format encodes it).
    pub width: u8,
    pub height: u8,
    pub bit_count: u16,
    pub payload: Vec<u8>,
}

/// Builds an ICO file: a directory followed by the images, in order.
pub fn build_ico(kind: u16, entries: &[IcoEntrySpec]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());

    let mut offset = 6 + 16 * entries.len();
    let mut body = Vec::new();
    for e in entries {
        out.push(e.width);
        out.push(e.height);
        out.push(0); // palette colours
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // planes / hotspot x
        out.extend_from_slice(&e.bit_count.to_le_bytes()); // bit count / hotspot y
        out.extend_from_slice(&(e.payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += e.payload.len();
        body.extend_from_slice(&e.payload);
    }
    out.extend_from_slice(&body);
    out
}

/// A minimal BMP-in-ICO payload (the header is all that matters to the parser).
pub fn bmp_icon_payload(size: usize) -> Vec<u8> {
    let mut data = vec![0x28, 0, 0, 0]; // BITMAPINFOHEADER size
    data.extend(std::iter::repeat_n(0u8, size.saturating_sub(4)));
    data
}

/// Extracts `(directory_entry_bytes, payload)` pairs from an ICO file.
pub fn parse_ico(data: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let count = u16::from_le_bytes([data[4], data[5]]) as usize;
    let mut out = Vec::new();
    for i in 0..count {
        let at = 6 + i * 16;
        let record = data[at..at + 16].to_vec();
        let size = u32::from_le_bytes([record[8], record[9], record[10], record[11]]) as usize;
        let offset = u32::from_le_bytes([record[12], record[13], record[14], record[15]]) as usize;
        out.push((record, data[offset..offset + size].to_vec()));
    }
    out
}

fn actl_payload(frames: u32) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&frames.to_be_bytes());
    data.extend_from_slice(&0u32.to_be_bytes()); // plays: infinite
    data
}

fn fctl_payload(sequence: u32, width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&sequence.to_be_bytes());
    data.extend_from_slice(&width.to_be_bytes());
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&0u32.to_be_bytes()); // x offset
    data.extend_from_slice(&0u32.to_be_bytes()); // y offset
    data.extend_from_slice(&1u16.to_be_bytes()); // delay numerator
    data.extend_from_slice(&10u16.to_be_bytes()); // delay denominator
    data.push(0); // dispose: none
    data.push(0); // blend: source
    data
}

fn frame_stream(width: u32, height: u32, seed: u8, level: u32) -> Vec<u8> {
    let mut raw = Vec::new();
    for y in 0..height as usize {
        raw.push(0u8); // filter: none, which leaves the most for the encoder
        for x in 0..width as usize {
            let base = ((x + y + seed as usize) % 64) as u8;
            raw.extend_from_slice(&[base, base.wrapping_mul(2), seed, 0xff]);
        }
    }
    let mut encoder =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(level));
    encoder.write_all(&raw).unwrap();
    encoder.finish().unwrap()
}

/// Builds a valid APNG: `frames` full-size frames. Frame 0 is the default image
/// (IDAT); later frames are `fcTL` + `fdAT`, and every second frame is split
/// across two `fdAT` chunks so the re-splitting logic gets exercised.
pub fn build_apng(width: u32, height: u32, frames: usize, level: u32) -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);

    let mut out = PNG_SIGNATURE.to_vec();
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));
    out.extend_from_slice(&chunk(b"acTL", &actl_payload(frames as u32)));
    out.extend_from_slice(&chunk(b"fcTL", &fctl_payload(0, width, height)));
    out.extend_from_slice(&chunk(b"IDAT", &frame_stream(width, height, 0, level)));

    let mut sequence = 1u32;
    for index in 1..frames {
        out.extend_from_slice(&chunk(b"fcTL", &fctl_payload(sequence, width, height)));
        sequence += 1;
        let stream = frame_stream(width, height, index as u8, level);
        if index % 2 == 0 {
            let half = stream.len() / 2;
            for piece in [&stream[..half], &stream[half..]] {
                let mut payload = sequence.to_be_bytes().to_vec();
                payload.extend_from_slice(piece);
                out.extend_from_slice(&chunk(b"fdAT", &payload));
                sequence += 1;
            }
        } else {
            let mut payload = sequence.to_be_bytes().to_vec();
            payload.extend_from_slice(&stream);
            out.extend_from_slice(&chunk(b"fdAT", &payload));
            sequence += 1;
        }
    }
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
}

/// Splits a PNG into `(type, payload)` pairs without validating CRCs.
pub fn png_chunks_lossy(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut pos = 8;
    while pos + 12 <= data.len() {
        let len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        if pos + 12 + len > data.len() {
            break;
        }
        let typ = String::from_utf8_lossy(&data[pos + 4..pos + 8]).into_owned();
        out.push((typ, data[pos + 8..pos + 8 + len].to_vec()));
        pos += 12 + len;
        if out.last().map(|(t, _)| t == "IEND").unwrap_or(false) {
            break;
        }
    }
    out
}
