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
    /// The scanline filter to write, 0 to 4. It is applied to the data: a row
    /// declared as `Sub` but written unfiltered is a broken file, and a fixture
    /// builder that can produce one is worse than none.
    pub filter: u8,
    pub level: u32,
    pub extra_chunks: Vec<(String, Vec<u8>)>,
    /// Adds an `acTL` chunk, turning the file into an APNG.
    pub apng: bool,
    /// Writes `IHDR` interlace method 1 and an Adam7 stream (#49).
    pub interlaced: bool,
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
            interlaced: false,
        }
    }
}

/// The pixel at (x, y) of the fixture image: a gradient plus structure, so that
/// filtering and interlacing have something to get wrong.
fn png_pixel(x: usize, y: usize, width: usize, height: usize) -> [u8; 4] {
    [
        ((x * 255) / width) as u8,
        ((y * 255) / height) as u8,
        ((x + y) % 251) as u8,
        0xff,
    ]
}

/// Applies one PNG scanline filter to `cur`, using `prev` as the row above and
/// `bpp` as the distance back within the row (PNG spec §9.2).
fn png_filter(filter: u8, cur: &[u8], prev: &[u8], bpp: usize, out: &mut Vec<u8>) {
    for i in 0..cur.len() {
        let x = cur[i];
        let a = if i >= bpp { cur[i - bpp] } else { 0 };
        let b = prev[i];
        let c = if i >= bpp { prev[i - bpp] } else { 0 };
        let value = match filter {
            0 => x,
            1 => x.wrapping_sub(a),
            2 => x.wrapping_sub(b),
            3 => x.wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8),
            4 => {
                let p = i16::from(a) + i16::from(b) - i16::from(c);
                let (pa, pb, pc) = (
                    (p - i16::from(a)).abs(),
                    (p - i16::from(b)).abs(),
                    (p - i16::from(c)).abs(),
                );
                let predictor = if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                };
                x.wrapping_sub(predictor)
            }
            other => panic!("PngSpec.filter {other} is not a PNG filter"),
        };
        out.push(value);
    }
}

/// The seven Adam7 passes: (x start, y start, x step, y step).
const ADAM7: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// Builds a truecolor+alpha, 8-bit PNG, interlaced or not.
pub fn build_png(spec: &PngSpec) -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&spec.width.to_be_bytes());
    ihdr.extend_from_slice(&spec.height.to_be_bytes());
    let interlace = u8::from(spec.interlaced);
    ihdr.extend_from_slice(&[8, 6, 0, 0, interlace]);

    let (width, height) = (spec.width as usize, spec.height as usize);
    // Every pass is written as a small image of its own, with its own previous
    // row for the filter — which is exactly what the format says.
    let mut raw = Vec::new();
    if spec.interlaced {
        for (xs, ys, xstep, ystep) in ADAM7 {
            // A pass exists only when it contains a pixel: Adam7's first pass
            // starts at (0,0) and every later one can fall outside a small
            // image.
            if width <= xs || height <= ys {
                continue;
            }
            let pass_width = (width - xs).div_ceil(xstep);
            let pass_height = (height - ys).div_ceil(ystep);
            let mut prev = vec![0u8; pass_width * 4];
            let mut cur = vec![0u8; pass_width * 4];
            for row in 0..pass_height {
                let y = ys + row * ystep;
                for column in 0..pass_width {
                    let x = xs + column * xstep;
                    cur[column * 4..column * 4 + 4]
                        .copy_from_slice(&png_pixel(x, y, width, height));
                }
                raw.push(spec.filter);
                png_filter(spec.filter, &cur, &prev, 4, &mut raw);
                prev.copy_from_slice(&cur);
            }
        }
    } else {
        let mut prev = vec![0u8; width * 4];
        let mut cur = vec![0u8; width * 4];
        for y in 0..height {
            for x in 0..width {
                cur[x * 4..x * 4 + 4].copy_from_slice(&png_pixel(x, y, width, height));
            }
            raw.push(spec.filter);
            png_filter(spec.filter, &cur, &prev, 4, &mut raw);
            prev.copy_from_slice(&cur);
        }
    }

    let mut encoder =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(spec.level));
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
        let len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let end = pos + 12 + len;
        if end > data.len() {
            return Err("truncated".into());
        }
        let typ = String::from_utf8_lossy(&data[pos + 4..pos + 8]).into_owned();
        let payload = data[pos + 8..pos + 8 + len].to_vec();
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&data[pos + 4..pos + 8]);
        hasher.update(&payload);
        let stored =
            u32::from_be_bytes([data[end - 4], data[end - 3], data[end - 2], data[end - 1]]);
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
        let comment = d
            [at + 46 + name_len + extra_len..at + 46 + name_len + extra_len + comment_len]
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
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(level));
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
        let len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
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

// ---------------------------------------------------------------------------
// JPEG fixtures
// ---------------------------------------------------------------------------

/// `(size, bits)` of a coefficient, exactly as T.81 defines the pair.
fn jpeg_magnitude(value: i32) -> (u32, u32) {
    let size = 32 - value.unsigned_abs().leading_zeros();
    let bits = if value < 0 {
        ((value - 1) as u32) & ((1u32 << size) - 1)
    } else {
        value as u32
    };
    (size, bits)
}

fn jpeg_segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.push(0xff);
    out.push(marker);
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
}

/// Bit writer for the fixture encoder: byte stuffing and restart markers
/// included, so fixtures exercise the same edge cases as real files.
struct JpegBits {
    out: Vec<u8>,
    byte: u8,
    filled: u32,
}

impl JpegBits {
    fn new() -> JpegBits {
        JpegBits {
            out: Vec::new(),
            byte: 0,
            filled: 0,
        }
    }

    fn put(&mut self, value: u32, size: u32) {
        for i in (0..size).rev() {
            self.bit((value >> i) & 1 == 1);
        }
    }

    fn bit(&mut self, value: bool) {
        self.byte = (self.byte << 1) | value as u8;
        self.filled += 1;
        if self.filled == 8 {
            self.flush_byte();
        }
    }

    fn flush_byte(&mut self) {
        self.out.push(self.byte);
        if self.byte == 0xff {
            self.out.push(0x00);
        }
        self.byte = 0;
        self.filled = 0;
    }

    fn finish(&mut self) {
        if self.filled > 0 {
            let pad = 8 - self.filled;
            self.byte = (self.byte << pad) | ((1u8 << pad) - 1);
            self.flush_byte();
        }
    }

    fn restart(&mut self, index: u8) {
        self.finish();
        self.out.push(0xff);
        self.out.push(0xd0 + index);
    }
}

/// A baseline JPEG (SOF0, 8-bit, one interleaved scan) whose Huffman tables are
/// deliberately wasteful: every symbol that occurs gets an eight-bit code. A
/// correct optimizer therefore always finds something to save.
///
/// `components` is 1 (grayscale) or 3 (YCbCr, all 1x1 sampling).
pub fn build_jpeg(width: u16, height: u16, components: usize) -> Vec<u8> {
    build_jpeg_with(width, height, components, b"tzim-test-exif-payload", b"", 0)
}

/// A baseline JPEG that uses restart markers every `interval` MCUs.
pub fn build_jpeg_with_restart(
    width: u16,
    height: u16,
    components: usize,
    interval: u16,
) -> Vec<u8> {
    build_jpeg_with(
        width,
        height,
        components,
        b"tzim-test-exif-payload",
        b"",
        interval,
    )
}

/// Same as [`build_jpeg`] but with bytes appended after `EOI`, which a rewrite
/// must preserve.
pub fn build_jpeg_with_trailing(
    width: u16,
    height: u16,
    components: usize,
    trailing: &[u8],
) -> Vec<u8> {
    build_jpeg_with(
        width,
        height,
        components,
        b"tzim-test-exif-payload",
        trailing,
        0,
    )
}

fn build_jpeg_with(
    width: u16,
    height: u16,
    components: usize,
    app1: &[u8],
    trailing: &[u8],
    restart_interval: u16,
) -> Vec<u8> {
    assert!(components == 1 || components == 3, "1 or 3 components only");
    assert!(width > 0 && height > 0);
    let block_cols = (width as usize).div_ceil(8);
    let block_rows = (height as usize).div_ceil(8);
    let blocks = block_cols * block_rows;

    let mut planes: Vec<Vec<i16>> = Vec::with_capacity(components);
    for component in 0..components {
        let mut plane = vec![0i16; blocks * 64];
        for block in 0..blocks {
            let base = block * 64;
            let dc = ((block * 37 + component * 11) % 41) as i16 - 20;
            plane[base] = dc;
            if block % 3 == 0 {
                plane[base + 1] = dc % 7;
            }
            if block % 5 == 0 {
                plane[base + 9] = -(dc % 5);
            }
            if block % 7 == 0 {
                plane[base + 20] = (dc % 3) + 1;
            }
        }
        planes.push(plane);
    }

    // First pass: which symbols occur at all?
    let mut dc_used = [false; 256];
    let mut ac_used = [false; 256];
    for plane in &planes {
        let mut predict = 0i32;
        for block in 0..blocks {
            // Restart intervals reset the DC predictor, which changes the
            // categories that occur; the statistics must model that.
            if restart_interval > 0 && block > 0 && block % restart_interval as usize == 0 {
                predict = 0;
            }
            let base = block * 64;
            let difference = plane[base] as i32 - predict;
            predict = plane[base] as i32;
            dc_used[jpeg_magnitude(difference).0 as usize] = true;
            let mut run = 0usize;
            for k in 1..64 {
                let value = plane[base + k];
                if value == 0 {
                    run += 1;
                    continue;
                }
                while run > 15 {
                    ac_used[0xf0] = true;
                    run -= 16;
                }
                let (size, _) = jpeg_magnitude(value as i32);
                ac_used[(run << 4) | size as usize] = true;
                run = 0;
            }
            if run > 0 {
                ac_used[0] = true;
            }
        }
    }

    // Every used symbol becomes an eight-bit code, in symbol order.
    let dc_symbols: Vec<u8> = (0..256).filter(|&s| dc_used[s]).map(|s| s as u8).collect();
    let ac_symbols: Vec<u8> = (0..256).filter(|&s| ac_used[s]).map(|s| s as u8).collect();
    assert!(dc_symbols.len() < 256 && ac_symbols.len() < 256);

    let mut out = Vec::new();
    out.extend_from_slice(&[0xff, 0xd8]); // SOI

    jpeg_segment(
        &mut out,
        0xe0,
        b"JFIF\0\x01\x01\x00\x00\x01\x00\x01\x00\x00",
    );
    let mut exif = Vec::from(&b"Exif\0\0"[..]);
    exif.extend_from_slice(app1);
    jpeg_segment(&mut out, 0xe1, &exif);

    let mut dqt = vec![0u8]; // table 0, all-ones quantization
    dqt.extend_from_slice(&[1u8; 64]);
    jpeg_segment(&mut out, 0xdb, &dqt);

    let mut sof = Vec::new();
    sof.push(8); // precision
    sof.extend_from_slice(&height.to_be_bytes());
    sof.extend_from_slice(&width.to_be_bytes());
    sof.push(components as u8);
    for component in 0..components {
        sof.push(component as u8 + 1);
        sof.push(0x11); // 1x1 sampling
        sof.push(0); // quantization table 0
    }
    jpeg_segment(&mut out, 0xc0, &sof);

    let mut dht = Vec::new();
    dht.push(0x00); // DC table 0
    let mut dc_counts = [0u8; 16];
    dc_counts[7] = dc_symbols.len() as u8;
    dht.extend_from_slice(&dc_counts);
    dht.extend_from_slice(&dc_symbols);
    dht.push(0x10); // AC table 0
    let mut ac_counts = [0u8; 16];
    ac_counts[7] = ac_symbols.len() as u8;
    dht.extend_from_slice(&ac_counts);
    dht.extend_from_slice(&ac_symbols);
    jpeg_segment(&mut out, 0xc4, &dht);

    jpeg_segment(&mut out, 0xfe, b"tzim fixture comment");

    if restart_interval > 0 {
        jpeg_segment(&mut out, 0xdd, &restart_interval.to_be_bytes());
    }

    let mut sos = Vec::new();
    sos.push(components as u8);
    for component in 0..components {
        sos.push(component as u8 + 1);
        sos.push(0x00); // DC table 0, AC table 0
    }
    sos.push(0); // Ss
    sos.push(63); // Se
    sos.push(0); // Ah/Al
    jpeg_segment(&mut out, 0xda, &sos);

    // Entropy-coded data: eight-bit codes for every symbol.
    let dc_code = |symbol: u8| dc_symbols.iter().position(|&s| s == symbol).unwrap() as u32;
    let ac_code = |symbol: u8| ac_symbols.iter().position(|&s| s == symbol).unwrap() as u32;
    let mut bits = JpegBits::new();
    let mut predicts = vec![0i32; components];
    let mut restart_index = 0u8;
    for block in 0..blocks {
        if restart_interval > 0 && block > 0 && block % restart_interval as usize == 0 {
            bits.restart(restart_index);
            restart_index = (restart_index + 1) & 7;
            for predict in predicts.iter_mut() {
                *predict = 0;
            }
        }
        for component in 0..components {
            let base = block * 64;
            let plane = &planes[component];
            let difference = plane[base] as i32 - predicts[component];
            predicts[component] = plane[base] as i32;
            let (size, value) = jpeg_magnitude(difference);
            bits.put(dc_code(size as u8), 8);
            if size > 0 {
                bits.put(value, size);
            }
            let mut run = 0usize;
            for k in 1..64 {
                let coefficient = plane[base + k];
                if coefficient == 0 {
                    run += 1;
                    continue;
                }
                while run > 15 {
                    bits.put(ac_code(0xf0), 8);
                    run -= 16;
                }
                let (size, value) = jpeg_magnitude(coefficient as i32);
                bits.put(ac_code(((run << 4) | size as usize) as u8), 8);
                bits.put(value, size);
                run = 0;
            }
            if run > 0 {
                bits.put(ac_code(0x00), 8);
            }
        }
    }
    bits.finish();
    out.extend_from_slice(&bits.out);

    out.extend_from_slice(&[0xff, 0xd9]); // EOI
    out.extend_from_slice(trailing);
    out
}

/// Replaces the frame-header marker (`FF C0`) with another SOF marker, which is
/// enough to describe a progressive or arithmetic file for the refusal paths.
pub fn jpeg_with_sof_marker(jpeg: &[u8], marker: u8) -> Vec<u8> {
    let mut out = jpeg.to_vec();
    for i in 0..out.len().saturating_sub(1) {
        if out[i] == 0xff && out[i + 1] == 0xc0 {
            out[i + 1] = marker;
            return out;
        }
    }
    panic!("no SOF0 marker in the fixture");
}

/// Changes the sample precision byte of the frame header.
pub fn jpeg_with_precision(jpeg: &[u8], precision: u8) -> Vec<u8> {
    let mut out = jpeg.to_vec();
    for i in 0..out.len().saturating_sub(1) {
        if out[i] == 0xff && out[i + 1] == 0xc0 {
            out[i + 4] = precision;
            return out;
        }
    }
    panic!("no SOF0 marker in the fixture");
}

// ---------------------------------------------------------------------------
// TIFF fixtures
// ---------------------------------------------------------------------------

/// A classic TIFF built from a description, so a failing test names a structure.
pub struct TiffSpec {
    pub big_endian: bool,
    pub width: u32,
    pub height: u32,
    pub samples_per_pixel: u16,
    pub bits_per_sample: u16,
    pub photometric: u16,
    /// `FillOrder` (266); `None` leaves the tag out, which means 1.
    pub fill_order: Option<u16>,
    /// 1 = none, 8 = Deflate (the strips are zlib-wrapped when read).
    pub compression: u16,
    pub rows_per_strip: u32,
    pub planar: u16,
    pub predictor: Option<u16>,
    /// Extra ASCII tags, e.g. `(315, "artist")`.
    pub extra_ascii: Vec<(u16, String)>,
    pub extra_short: Vec<(u16, u16)>,
    /// Tiles instead of strips: the layout the handler refuses.
    pub tiled: bool,
    /// Raw strip bytes; generated from the geometry when empty.
    pub strips: Vec<Vec<u8>>,
}

impl Default for TiffSpec {
    fn default() -> TiffSpec {
        TiffSpec {
            big_endian: false,
            width: 64,
            height: 64,
            samples_per_pixel: 1,
            bits_per_sample: 8,
            photometric: 1,
            fill_order: None,
            compression: 1,
            rows_per_strip: 8,
            planar: 1,
            predictor: None,
            extra_ascii: Vec::new(),
            extra_short: Vec::new(),
            tiled: false,
            strips: Vec::new(),
        }
    }
}

fn tiff_w16(out: &mut [u8], at: usize, value: u16, big: bool) {
    let bytes = if big {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    out[at..at + 2].copy_from_slice(&bytes);
}

fn tiff_w32(out: &mut [u8], at: usize, value: u32, big: bool) {
    let bytes = if big {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    out[at..at + 4].copy_from_slice(&bytes);
}

/// A zlib stream, which is what a Deflate TIFF strip is (raw DEFLATE is not).
fn tiff_zlib(data: &[u8], level: u32) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(level));
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn tiff_numbers(values: &[u32], typ: u16, big: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for value in values {
        if typ == 3 {
            let small = *value as u16;
            out.extend_from_slice(&if big {
                small.to_be_bytes()
            } else {
                small.to_le_bytes()
            });
        } else {
            out.extend_from_slice(&if big {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            });
        }
    }
    out
}

struct TiffEntry {
    tag: u16,
    typ: u16,
    count: u32,
    raw: Vec<u8>,
}

fn tiff_row_bytes(spec: &TiffSpec) -> usize {
    let bits =
        spec.width as usize * spec.samples_per_pixel as usize * spec.bits_per_sample as usize;
    bits.div_ceil(8)
}

/// Smooth, repeating bytes: compressible, but not a constant.
fn tiff_strip_bytes(spec: &TiffSpec, rows: u32, first_row: u32) -> Vec<u8> {
    let row = tiff_row_bytes(spec);
    let mut out = Vec::with_capacity(row * rows as usize);
    for r in 0..rows {
        for b in 0..row {
            let v = ((b / 12) as u32 + (first_row + r) / 4) % 8;
            out.push((v * 17) as u8);
        }
    }
    out
}

/// Builds a classic TIFF: header, IFD, out-of-line values, strips, 2-byte aligned.
pub fn build_tiff(spec: &TiffSpec) -> Vec<u8> {
    let big = spec.big_endian;
    assert!(
        matches!(spec.bits_per_sample, 8 | 16),
        "fixtures use 8 or 16 bits"
    );

    let mut strips: Vec<Vec<u8>> = Vec::new();
    if spec.strips.is_empty() {
        let mut first_row = 0u32;
        while first_row < spec.height {
            let rows = spec.rows_per_strip.min(spec.height - first_row);
            strips.push(tiff_strip_bytes(spec, rows, first_row));
            first_row += rows;
        }
    } else {
        strips = spec.strips.clone();
    }
    if spec.compression != 1 {
        strips = strips.iter().map(|s| tiff_zlib(s, 9)).collect();
    }

    let mut entries = vec![
        TiffEntry {
            tag: 256,
            typ: 4,
            count: 1,
            raw: tiff_numbers(&[spec.width], 4, big),
        },
        TiffEntry {
            tag: 257,
            typ: 4,
            count: 1,
            raw: tiff_numbers(&[spec.height], 4, big),
        },
        TiffEntry {
            tag: 258,
            typ: 3,
            count: spec.samples_per_pixel as u32,
            raw: tiff_numbers(
                &vec![spec.bits_per_sample as u32; spec.samples_per_pixel as usize],
                3,
                big,
            ),
        },
        TiffEntry {
            tag: 259,
            typ: 3,
            count: 1,
            raw: tiff_numbers(&[spec.compression as u32], 3, big),
        },
        TiffEntry {
            tag: 262,
            typ: 3,
            count: 1,
            raw: tiff_numbers(&[spec.photometric as u32], 3, big),
        },
    ];
    if let Some(fill_order) = spec.fill_order {
        entries.push(TiffEntry {
            tag: 266,
            typ: 3,
            count: 1,
            raw: tiff_numbers(&[fill_order as u32], 3, big),
        });
    }
    entries.push(TiffEntry {
        tag: 277,
        typ: 3,
        count: 1,
        raw: tiff_numbers(&[spec.samples_per_pixel as u32], 3, big),
    });
    entries.push(TiffEntry {
        tag: 284,
        typ: 3,
        count: 1,
        raw: tiff_numbers(&[spec.planar as u32], 3, big),
    });
    if let Some(predictor) = spec.predictor {
        entries.push(TiffEntry {
            tag: 317,
            typ: 3,
            count: 1,
            raw: tiff_numbers(&[predictor as u32], 3, big),
        });
    }
    for (tag, value) in &spec.extra_short {
        entries.push(TiffEntry {
            tag: *tag,
            typ: 3,
            count: 1,
            raw: tiff_numbers(&[*value as u32], 3, big),
        });
    }
    for (tag, value) in &spec.extra_ascii {
        let mut raw = value.as_bytes().to_vec();
        raw.push(0);
        entries.push(TiffEntry {
            tag: *tag,
            typ: 2,
            count: raw.len() as u32,
            raw,
        });
    }

    let sizes: Vec<u32> = strips.iter().map(|s| s.len() as u32).collect();
    if spec.tiled {
        entries.push(TiffEntry {
            tag: 322,
            typ: 4,
            count: 1,
            raw: tiff_numbers(&[spec.width], 4, big),
        });
        entries.push(TiffEntry {
            tag: 323,
            typ: 4,
            count: 1,
            raw: tiff_numbers(&[spec.rows_per_strip], 4, big),
        });
        entries.push(TiffEntry {
            tag: 324,
            typ: 4,
            count: strips.len() as u32,
            raw: tiff_numbers(&vec![0; strips.len()], 4, big),
        });
        entries.push(TiffEntry {
            tag: 325,
            typ: 4,
            count: strips.len() as u32,
            raw: tiff_numbers(&sizes, 4, big),
        });
    } else {
        entries.push(TiffEntry {
            tag: 273,
            typ: 4,
            count: strips.len() as u32,
            raw: tiff_numbers(&vec![0; strips.len()], 4, big),
        });
        entries.push(TiffEntry {
            tag: 278,
            typ: 4,
            count: 1,
            raw: tiff_numbers(&[spec.rows_per_strip], 4, big),
        });
        entries.push(TiffEntry {
            tag: 279,
            typ: 4,
            count: strips.len() as u32,
            raw: tiff_numbers(&sizes, 4, big),
        });
    }

    entries.sort_by_key(|e| e.tag);

    let mut out = vec![0u8; 8 + 2 + entries.len() * 12 + 4];
    let mut slots: Vec<Option<usize>> = Vec::with_capacity(entries.len());
    for entry in &entries {
        if entry.raw.len() > 4 {
            let at = out.len() + (out.len() & 1);
            out.resize(at + entry.raw.len(), 0);
            out[at..at + entry.raw.len()].copy_from_slice(&entry.raw);
            slots.push(Some(at));
        } else {
            slots.push(None);
        }
    }

    let mut offsets: Vec<u32> = Vec::with_capacity(strips.len());
    for strip in &strips {
        let at = out.len() + (out.len() & 1);
        out.resize(at, 0);
        out.extend_from_slice(strip);
        offsets.push(at as u32);
    }

    out[..2].copy_from_slice(if big { b"MM" } else { b"II" });
    tiff_w16(&mut out, 2, 42, big);
    tiff_w32(&mut out, 4, 8, big);
    tiff_w16(&mut out, 8, entries.len() as u16, big);
    for (index, entry) in entries.iter().enumerate() {
        let at = 10 + index * 12;
        tiff_w16(&mut out, at, entry.tag, big);
        tiff_w16(&mut out, at + 2, entry.typ, big);
        tiff_w32(&mut out, at + 4, entry.count, big);
        match slots[index] {
            Some(slot) => tiff_w32(&mut out, at + 8, slot as u32, big),
            None => out[at + 8..at + 8 + entry.raw.len()].copy_from_slice(&entry.raw),
        }
    }

    // The offsets are known only now, so the array is filled last.
    let offsets_tag = if spec.tiled { 324 } else { 273 };
    let index = entries.iter().position(|e| e.tag == offsets_tag).unwrap();
    let encoded = tiff_numbers(&offsets, entries[index].typ, big);
    match slots[index] {
        Some(slot) => out[slot..slot + encoded.len()].copy_from_slice(&encoded),
        None => out[10 + index * 12 + 8..][..encoded.len()].copy_from_slice(&encoded),
    }

    out
}

/// A classic header with no IFD body, for structural refusal tests.
pub fn tiff_header(big_endian: bool, magic: u16) -> Vec<u8> {
    let mut out = vec![0u8; 8];
    out[..2].copy_from_slice(if big_endian { b"MM" } else { b"II" });
    tiff_w16(&mut out, 2, magic, big_endian);
    tiff_w32(&mut out, 4, 8, big_endian);
    out
}

/// Every IFD entry as `(tag, type, count, resolved value bytes)`.
pub fn parse_tiff_tags(data: &[u8]) -> Vec<(u16, u16, u32, Vec<u8>)> {
    assert!(data.len() >= 8, "not a TIFF");
    let big = &data[..2] == b"MM";
    let read16 = |at: usize| -> u16 {
        if big {
            u16::from_be_bytes([data[at], data[at + 1]])
        } else {
            u16::from_le_bytes([data[at], data[at + 1]])
        }
    };
    let read32 = |at: usize| -> u32 {
        if big {
            u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
        } else {
            u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
        }
    };
    let ifd = read32(4) as usize;
    let count = read16(ifd) as usize;
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let at = ifd + 2 + index * 12;
        let tag = read16(at);
        let typ = read16(at + 2);
        let how_many = read32(at + 4);
        let size = match typ {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            other => panic!("fixture uses unexpected type {other}"),
        };
        let len = size * how_many as usize;
        let value = if len <= 4 {
            data[at + 8..at + 8 + len].to_vec()
        } else {
            let offset = read32(at + 8) as usize;
            data[offset..offset + len].to_vec()
        };
        out.push((tag, typ, how_many, value));
    }
    out
}

pub fn tiff_tag(data: &[u8], tag: u16) -> Option<Vec<u8>> {
    parse_tiff_tags(data)
        .into_iter()
        .find(|(t, _, _, _)| *t == tag)
        .map(|(_, _, _, value)| value)
}

pub fn tiff_compression(data: &[u8]) -> u16 {
    let raw = tiff_tag(data, 259).expect("Compression tag");
    match &data[..2] {
        b"MM" => u16::from_be_bytes([raw[0], raw[1]]),
        _ => u16::from_le_bytes([raw[0], raw[1]]),
    }
}

/// The strips, decoded: the independent check that the pixels survived.
pub fn tiff_strips(data: &[u8]) -> Vec<Vec<u8>> {
    let offsets = tiff_tag(data, 273).expect("StripOffsets");
    let sizes = tiff_tag(data, 279).expect("StripByteCounts");
    let big = &data[..2] == b"MM";
    let number = |raw: &[u8], index: usize| -> u32 {
        let at = index * 4;
        if big {
            u32::from_be_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]])
        } else {
            u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]])
        }
    };
    let mut out = Vec::new();
    for index in 0..offsets.len() / 4 {
        let start = number(&offsets, index) as usize;
        let len = number(&sizes, index) as usize;
        let body = &data[start..start + len];
        if tiff_compression(data) == 1 {
            out.push(body.to_vec());
        } else {
            let mut inflated = Vec::new();
            flate2::read::ZlibDecoder::new(body)
                .read_to_end(&mut inflated)
                .expect("a Deflate strip decodes");
            out.push(inflated);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// zlib (RFC 1950)
// ---------------------------------------------------------------------------

/// A zlib stream containing `payload`, produced at `level`.
pub fn build_zlib(payload: &[u8], level: u32) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(level));
    encoder.write_all(payload).unwrap();
    encoder.finish().unwrap()
}

/// The decompressed payload of a zlib stream.
pub fn zlib_payload(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .expect("decode zlib");
    out
}

/// Rewrites the two header bytes to advertise a window of `2^(cinfo + 8)` bytes,
/// keeping every other bit and recomputing the check value. The DEFLATE body is
/// untouched, which is exactly the shape of a stream written with a small window
/// by an encoder that chose one.
pub fn zlib_with_window(data: &[u8], cinfo: u8) -> Vec<u8> {
    let mut out = data.to_vec();
    let cmf = (cinfo << 4) | 8;
    // FLEVEL and FDICT are copied from the original; only CINFO changes.
    let mut flg = out[1] & 0xe0;
    while !((u16::from(cmf) << 8) | u16::from(flg)).is_multiple_of(31) {
        flg += 1;
        assert!(flg < 0xe0, "no valid FLG found");
    }
    out[0] = cmf;
    out[1] = flg;
    out
}

// ---------------------------------------------------------------------------
// FITS
// ---------------------------------------------------------------------------
//
// The writer here is hand-rolled rather than borrowed from the handler: a
// fixture built by the code under test proves nothing about it. Every HDU is a
// sequence of 2880-byte records, the header ends at an `END` card and the data
// is padded with zeros, which are the three properties the handler relies on.
// The reader is hand-rolled for the same reason: it inflates the tiles with
// `flate2` directly, so a test that compares the array does not go through the
// handler's own decompressor.

/// The record size every FITS file is built from.
pub const FITS_RECORD: usize = 2880;

/// One 80-character card, padded with spaces.
pub fn fits_card_raw(text: &str) -> Vec<u8> {
    let mut card = vec![b' '; 80];
    let bytes = text.as_bytes();
    let take = bytes.len().min(80);
    card[..take].copy_from_slice(&bytes[..take]);
    card
}

/// A value card: `KEYWORD = value`, the value right-justified in its 20 columns.
pub fn fits_card(keyword: &str, value: &str) -> String {
    format!("{keyword:<8}= {value:>20}")
}

/// A character-string card: `KEYWORD = 'value'`.
pub fn fits_string_card(keyword: &str, value: &str) -> String {
    format!("{keyword:<8}= '{value}'")
}

/// A logical card: `KEYWORD = T` or `F`.
pub fn fits_bool_card(keyword: &str, value: bool) -> String {
    fits_card(keyword, if value { "T" } else { "F" })
}

/// Pads `out` with zeros to a whole number of records.
pub fn fits_pad(out: &mut Vec<u8>) {
    let rem = out.len() % FITS_RECORD;
    if rem != 0 {
        out.resize(out.len() + (FITS_RECORD - rem), 0);
    }
}

/// One HDU: the cards, an `END` card, spaces to a record boundary, the data,
/// zeros to a record boundary.
pub fn fits_hdu(cards: &[String], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for card in cards {
        out.extend_from_slice(&fits_card_raw(card));
    }
    out.extend_from_slice(&fits_card_raw("END"));
    let rem = out.len() % FITS_RECORD;
    if rem != 0 {
        out.resize(out.len() + (FITS_RECORD - rem), b' ');
    }
    out.extend_from_slice(data);
    fits_pad(&mut out);
    out
}

/// Bytes per sample for a `BITPIX` value.
pub fn fits_bytes_per_sample(bitpix: i64) -> usize {
    (bitpix.unsigned_abs() / 8) as usize
}

/// The data an image of the given shape holds, filled with a repeating pattern
/// so that a fixture is genuinely compressible.
///
/// The pattern repeats every four bytes rather than every byte: a FITS fixture
/// is padded to 2880-byte records, so an image whose tiles compress to less than
/// a record's worth of saving would not shrink at all and a test asserting a
/// gain would be measuring the padding instead of the handler.
pub fn fits_samples(bitpix: i64, axis: &[usize]) -> Vec<u8> {
    let count = axis.iter().product::<usize>() * fits_bytes_per_sample(bitpix);
    (0..count).map(|i| ((i / 4) % 251) as u8).collect()
}

/// A primary image HDU. `axis` is `(NAXIS1, NAXIS2, …)`.
pub fn fits_primary_image(bitpix: i64, axis: &[usize], data: &[u8]) -> Vec<u8> {
    let mut cards = vec![
        fits_bool_card("SIMPLE", true),
        fits_card("BITPIX", &bitpix.to_string()),
        fits_card("NAXIS", &axis.len().to_string()),
    ];
    for (index, length) in axis.iter().enumerate() {
        cards.push(fits_card(
            &format!("NAXIS{}", index + 1),
            &length.to_string(),
        ));
    }
    cards.push(fits_bool_card("EXTEND", true));
    fits_hdu(&cards, data)
}

/// An `IMAGE` extension HDU. It carries the `PCOUNT`/`GCOUNT` pair the standard
/// requires of an extension and an image does not.
pub fn fits_image_ext(bitpix: i64, axis: &[usize], data: &[u8], extra: &[String]) -> Vec<u8> {
    let mut cards = vec![
        fits_string_card("XTENSION", "IMAGE"),
        fits_card("BITPIX", &bitpix.to_string()),
        fits_card("NAXIS", &axis.len().to_string()),
    ];
    for (index, length) in axis.iter().enumerate() {
        cards.push(fits_card(
            &format!("NAXIS{}", index + 1),
            &length.to_string(),
        ));
    }
    cards.push(fits_card("PCOUNT", "0"));
    cards.push(fits_card("GCOUNT", "1"));
    cards.extend_from_slice(extra);
    fits_hdu(&cards, data)
}

/// A `BINTABLE` that is not a compressed image: the shape a handler must copy
/// and never touch.
pub fn fits_bin_table(rows: usize, row_bytes: usize, heap: &[u8]) -> Vec<u8> {
    let cards = vec![
        fits_string_card("XTENSION", "BINTABLE"),
        fits_card("BITPIX", "8"),
        fits_card("NAXIS", "2"),
        fits_card("NAXIS1", &row_bytes.to_string()),
        fits_card("NAXIS2", &rows.to_string()),
        fits_card("PCOUNT", &heap.len().to_string()),
        fits_card("GCOUNT", "1"),
        fits_card("TFIELDS", "1"),
        fits_string_card("TTYPE1", "VALUE"),
        fits_string_card("TFORM1", "1PB"),
    ];
    let mut data = vec![0u8; rows * row_bytes];
    data.extend_from_slice(heap);
    fits_hdu(&cards, &data)
}

/// A non-standard extension, the shape the measurement lost five HST files to:
/// an unknown `XTENSION` whose data is copied verbatim or the file is damaged.
pub fn fits_nonstandard(name: &str, data: &[u8]) -> Vec<u8> {
    let cards = vec![
        fits_string_card("XTENSION", name),
        fits_card("BITPIX", "8"),
        fits_card("NAXIS", "1"),
        fits_card("NAXIS1", &data.len().to_string()),
        fits_card("PCOUNT", "0"),
        fits_card("GCOUNT", "1"),
    ];
    fits_hdu(&cards, data)
}

/// The cards of a `ZIMAGE` table, i.e. a file that is already compressed, as
/// `fpack` writes it. `codec` is `GZIP_1`, `GZIP_2`, `RICE_1`, …
pub fn fits_compressed_image(
    bitpix: i64,
    axis: &[usize],
    codec: &str,
    tiles: &[Vec<u8>],
) -> Vec<u8> {
    let mut heap = Vec::new();
    let mut descriptors = Vec::new();
    for tile in tiles {
        descriptors.extend_from_slice(&(tile.len() as u32).to_be_bytes());
        descriptors.extend_from_slice(&(heap.len() as u32).to_be_bytes());
        heap.extend_from_slice(tile);
    }
    let longest = tiles.iter().map(Vec::len).max().unwrap_or(0);
    let mut cards = vec![
        fits_string_card("XTENSION", "BINTABLE"),
        fits_card("BITPIX", "8"),
        fits_card("NAXIS", "2"),
        fits_card("NAXIS1", "8"),
        fits_card("NAXIS2", &tiles.len().to_string()),
        fits_card("PCOUNT", &heap.len().to_string()),
        fits_card("GCOUNT", "1"),
        fits_card("TFIELDS", "1"),
        fits_string_card("TTYPE1", "COMPRESSED_DATA"),
        fits_string_card("TFORM1", &format!("1PB({longest})")),
        fits_bool_card("ZIMAGE", true),
        fits_string_card("ZTENSION", "IMAGE"),
        fits_card("ZBITPIX", &bitpix.to_string()),
        fits_card("ZNAXIS", &axis.len().to_string()),
    ];
    for (index, length) in axis.iter().enumerate() {
        cards.push(fits_card(
            &format!("ZNAXIS{}", index + 1),
            &length.to_string(),
        ));
    }
    cards.push(fits_card("ZTILE1", &axis[0].to_string()));
    for index in 2..=axis.len() {
        cards.push(fits_card(&format!("ZTILE{index}"), "1"));
    }
    cards.push(fits_string_card("ZCMPTYPE", codec));
    let mut data = descriptors;
    data.extend_from_slice(&heap);
    fits_hdu(&cards, &data)
}

/// One gzip member holding `payload`, which is what a `GZIP_1`/`GZIP_2` tile is.
pub fn fits_gzip(payload: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(9));
    encoder.write_all(payload).unwrap();
    encoder.finish().unwrap()
}

/// The inverse of the convention's shuffle: the most significant byte of every
/// sample first, in sample order, then the next byte of every sample.
pub fn fits_shuffle(samples: &[u8], per_sample: usize) -> Vec<u8> {
    let count = samples.len() / per_sample;
    let mut out = vec![0u8; samples.len()];
    for byte in 0..per_sample {
        for sample in 0..count {
            out[byte * count + sample] = samples[sample * per_sample + byte];
        }
    }
    out
}

/// One HDU as this reader sees it.
pub struct FitsHdu {
    /// The raw 80-byte cards, up to and including `END`.
    pub cards: Vec<String>,
    /// The data, without its padding.
    pub data: Vec<u8>,
    /// The header and data as they appeared in the file, padding included.
    pub raw: Vec<u8>,
}

fn fits_field(card: &str) -> &str {
    card.get(10..30).unwrap_or("").trim()
}

/// The value field of `keyword`, as text.
pub fn fits_value(hdu: &FitsHdu, keyword: &str) -> Option<String> {
    hdu.cards.iter().find_map(|card| {
        let name = card.get(..8).unwrap_or("").trim();
        if name == keyword {
            Some(fits_field(card).to_string())
        } else {
            None
        }
    })
}

/// An integer card.
pub fn fits_int(hdu: &FitsHdu, keyword: &str) -> Option<i64> {
    fits_value(hdu, keyword)?.trim().parse().ok()
}

/// A logical card.
pub fn fits_bool(hdu: &FitsHdu, keyword: &str) -> Option<bool> {
    match fits_value(hdu, keyword)?.trim() {
        "T" => Some(true),
        "F" => Some(false),
        _ => None,
    }
}

/// A character-string card, without its quotes.
pub fn fits_text(hdu: &FitsHdu, keyword: &str) -> Option<String> {
    let value = fits_value(hdu, keyword)?;
    let inner = value.trim().strip_prefix('\'')?;
    let end = inner.rfind('\'')?;
    Some(inner[..end].trim().to_string())
}

/// Walks a file and returns every HDU.
pub fn fits_parse(bytes: &[u8]) -> Vec<FitsHdu> {
    let mut out: Vec<FitsHdu> = Vec::new();
    let mut offset = 0usize;
    while offset + FITS_RECORD <= bytes.len() {
        let start = offset;
        let mut cards = Vec::new();
        let mut end_of_header = None;
        while end_of_header.is_none() {
            assert!(
                offset + FITS_RECORD <= bytes.len(),
                "header runs past the file"
            );
            for within in (0..FITS_RECORD).step_by(80) {
                let card = &bytes[offset + within..offset + within + 80];
                let text = String::from_utf8_lossy(card).into_owned();
                let name = text.get(..8).unwrap_or("").trim().to_string();
                cards.push(text);
                if name == "END" {
                    end_of_header = Some(offset + FITS_RECORD);
                    break;
                }
            }
            offset += FITS_RECORD;
        }
        let data_start = end_of_header.expect("an END card");
        let hdu = FitsHdu {
            cards,
            data: Vec::new(),
            raw: Vec::new(),
        };
        let bitpix = fits_int(&hdu, "BITPIX").expect("BITPIX");
        let naxis = fits_int(&hdu, "NAXIS").expect("NAXIS");
        let mut length = 1usize;
        for axis in 1..=naxis {
            let keyword = format!("NAXIS{axis}");
            length *= fits_int(&hdu, &keyword).expect("NAXISn") as usize;
        }
        let pcount = fits_int(&hdu, "PCOUNT").unwrap_or(0) as usize;
        let gcount = fits_int(&hdu, "GCOUNT").unwrap_or(1) as usize;
        let data_len = if naxis == 0 {
            0
        } else {
            fits_bytes_per_sample(bitpix) * (length + pcount) * gcount
        };
        let padded = data_len.div_ceil(FITS_RECORD) * FITS_RECORD;
        let data_end = data_start + data_len;
        assert!(data_end <= bytes.len(), "data runs past the file");
        offset = data_start + padded;
        out.push(FitsHdu {
            data: bytes[data_start..data_end].to_vec(),
            raw: bytes[start..offset].to_vec(),
            cards: hdu.cards,
        });
    }
    out
}

/// The tiles of a compressed HDU, inflated, in the order the row descriptors
/// name them. Independent of the handler: `flate2` inflates the member and
/// checks its CRC-32 and `ISIZE`.
pub fn fits_tiles(hdu: &FitsHdu) -> Vec<Vec<u8>> {
    let rows = fits_int(hdu, "NAXIS2").expect("NAXIS2") as usize;
    let heap_len = fits_int(hdu, "PCOUNT").expect("PCOUNT") as usize;
    let heap = &hdu.data[8 * rows..8 * rows + heap_len];
    let mut out = Vec::new();
    for row in 0..rows {
        let at = row * 8;
        let length = u32::from_be_bytes([
            hdu.data[at],
            hdu.data[at + 1],
            hdu.data[at + 2],
            hdu.data[at + 3],
        ]) as usize;
        let offset = u32::from_be_bytes([
            hdu.data[at + 4],
            hdu.data[at + 5],
            hdu.data[at + 6],
            hdu.data[at + 7],
        ]) as usize;
        let mut out_tile = Vec::new();
        flate2::read::GzDecoder::new(&heap[offset..offset + length])
            .read_to_end(&mut out_tile)
            .expect("inflate a tile");
        out.push(out_tile);
    }
    out
}

/// The array a compressed HDU carries, concatenated and unshuffled for
/// `GZIP_2`.
pub fn fits_compressed_array(hdu: &FitsHdu) -> Vec<u8> {
    let bitpix = fits_int(hdu, "ZBITPIX").expect("ZBITPIX");
    let per_sample = fits_bytes_per_sample(bitpix);
    let mut out = Vec::new();
    for tile in fits_tiles(hdu) {
        if bitpix < 0 {
            let count = tile.len() / per_sample;
            let mut samples = vec![0u8; tile.len()];
            for byte in 0..per_sample {
                for sample in 0..count {
                    samples[sample * per_sample + byte] = tile[byte * count + sample];
                }
            }
            out.extend_from_slice(&samples);
        } else {
            out.extend_from_slice(&tile);
        }
    }
    out
}

/// A `BINTABLE` whose single column is zero-width (`TFORM1 = '0A'`), the shape
/// `zerowidth.fits` has.
///
/// It is not a rewrite target and never will be — but it is the file the
/// measurement's reference writer lost the array on, so the suite carries the
/// shape and asserts that this handler copies it byte for byte instead.
pub fn fits_zero_width_table(rows: usize) -> Vec<u8> {
    let cards = vec![
        fits_string_card("XTENSION", "BINTABLE"),
        fits_card("BITPIX", "8"),
        fits_card("NAXIS", "2"),
        fits_card("NAXIS1", "0"),
        fits_card("NAXIS2", &rows.to_string()),
        fits_card("PCOUNT", "0"),
        fits_card("GCOUNT", "1"),
        fits_card("TFIELDS", "1"),
        fits_string_card("TTYPE1", "NOTHING"),
        fits_string_card("TFORM1", "0A"),
    ];
    fits_hdu(&cards, &[])
}

/// An `IMAGE` extension carrying a keyword the compressed form has no `Z` card
/// for — the shape of `double_ext.fits`, where the reference writer dropped
/// `EXTNAMX` and the round trip failed.
///
/// This handler copies every non-structural card verbatim, so the equivalent
/// here is accepted rather than refused, and the test is that the card survives.
pub fn fits_image_ext_with_odd_card(bitpix: i64, axis: &[usize], data: &[u8]) -> Vec<u8> {
    fits_image_ext(
        bitpix,
        axis,
        data,
        &[
            fits_string_card("EXTNAME", "SCI"),
            fits_string_card("EXTNAMX", "SCI"),
        ],
    )
}
