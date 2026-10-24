//! Fixture builders shared by the integration tests.
//!
//! Fixtures are generated in code rather than committed as binaries: each one is
//! described by what it contains, so a failing test points at a known structure.

#![allow(dead_code)]

use std::io::Write;
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
        let mut actl = Vec::new();
        actl.extend_from_slice(&1u32.to_be_bytes()); // frames
        actl.extend_from_slice(&0u32.to_be_bytes()); // plays
        out.extend_from_slice(&chunk(b"acTL", &actl));
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
