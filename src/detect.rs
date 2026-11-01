use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::report::ReasonCode;

/// Formats Tzim can handle. Detection is by content, never by file name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Png,
    Gzip,
    Zip,
    Ico,
    Jpeg,
    Tiff,
    Zlib,
    Fits,
    Flac,
    Pdf,
}

impl Format {
    pub fn id(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Gzip => "gzip",
            Format::Zip => "zip",
            Format::Ico => "ico",
            Format::Jpeg => "jpeg",
            Format::Tiff => "tiff",
            Format::Zlib => "zlib",
            Format::Fits => "fits",
            Format::Flac => "flac",
            Format::Pdf => "pdf",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Gzip => "GZIP",
            Format::Zip => "ZIP",
            Format::Ico => "ICO",
            Format::Jpeg => "JPEG",
            Format::Tiff => "TIFF",
            Format::Zlib => "ZLIB",
            Format::Fits => "FITS",
            Format::Flac => "FLAC",
            Format::Pdf => "PDF",
        }
    }
}

pub const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Detects the format from the first bytes of a file.
///
/// Returns `None` for anything unsupported. A few magic bytes in a random file
/// are not enough: callers run a full structural validation before writing
/// anything, and every handler refuses what it cannot parse completely.
pub fn detect(head: &[u8]) -> Option<Format> {
    if head.len() >= PNG_SIGNATURE.len() && head[..PNG_SIGNATURE.len()] == PNG_SIGNATURE {
        return Some(Format::Png);
    }
    if head.len() >= 3 && head[0] == 0x1f && head[1] == 0x8b && head[2] == 0x08 {
        return Some(Format::Gzip);
    }
    // ICO (type 1) and CUR (type 2) share a 6-byte header. The signature is
    // weak on its own, so the image count is checked as well.
    if head.len() >= 6
        && head[0] == 0
        && head[1] == 0
        && (head[2] == 1 || head[2] == 2)
        && head[3] == 0
    {
        let count = u16::from_le_bytes([head[4], head[5]]);
        if (1..=256).contains(&count) {
            return Some(Format::Ico);
        }
    }
    // JPEG: SOI followed by the first marker. The third byte is always 0xFF in
    // a valid file, which keeps ordinary binary data from matching.
    if head.len() >= 3 && head[0] == 0xff && head[1] == 0xd8 && head[2] == 0xff {
        return Some(Format::Jpeg);
    }
    // TIFF: byte order marker, then magic 42 (classic) or 43 (BigTIFF). BigTIFF
    // is recognised so the handler can refuse it with a reason instead of the
    // file being reported as an unknown format.
    if head.len() >= 4 {
        let tiff = match (head[0], head[1]) {
            (0x49, 0x49) => matches!((head[2], head[3]), (0x2a, 0x00) | (0x2b, 0x00)),
            (0x4d, 0x4d) => matches!((head[2], head[3]), (0x00, 0x2a) | (0x00, 0x2b)),
            _ => false,
        };
        if tiff {
            return Some(Format::Tiff);
        }
    }

    // FITS: the first card of the primary header is `SIMPLE  = ... T`, and a
    // file is a sequence of 2880-byte records. The keyword and the `=` are
    // checked here — ten bytes, which is what the probe reads — and the handler
    // then requires the value to be `T` and every record to be whole. That is
    // the same split as the one zlib gets below: a weak test here, a full
    // structural check before anything is written.
    if head.len() >= 9 && head[..9] == *b"SIMPLE  =" {
        return Some(Format::Fits);
    }

    // FLAC: the four-byte marker is unambiguous at offset zero. The handler
    // still validates every metadata block and frame before writing.
    if head.starts_with(b"fLaC") {
        return Some(Format::Flac);
    }

    // PDF: the version header is permitted within the first 1024 bytes, after a
    // short producer-specific prefix. Detection is only a hint; the PDF handler
    // insists on a complete, single-revision classic-xref or supported xref stream.
    if head.len() >= 5
        && head[..head.len().min(1024)]
            .windows(5)
            .any(|window| window == b"%PDF-")
    {
        return Some(Format::Pdf);
    }

    // zlib: CM = 8 (DEFLATE), CINFO <= 7, FDICT clear, and a header that passes
    // RFC 1950's own check — the two bytes, big-endian, divisible by 31. That is
    // weaker than a magic number, so the handler re-validates everything, and a
    // false positive costs a `failed` line with the file untouched, never a
    // rewrite. Checked after the strong signatures above, so a real PNG or gzip
    // can never be mistaken for a zlib stream.
    if head.len() >= 2
        && head[0] & 0x0f == 8
        && head[0] >> 4 <= 7
        && head[1] & 0x20 == 0
        && ((u16::from(head[0]) << 8) | u16::from(head[1])).is_multiple_of(31)
    {
        return Some(Format::Zlib);
    }

    // A local header (the usual case) or an end-of-central-directory record
    // (an empty archive). Both start with "PK".
    if head.len() >= 4
        && head[0] == b'P'
        && head[1] == b'K'
        && ((head[2] == 0x03 && head[3] == 0x04) || (head[2] == 0x05 && head[3] == 0x06))
    {
        return Some(Format::Zip);
    }
    None
}

/// What the first bytes of a file say about it.
///
/// `pixels` is filled only when the answer is in the header this probe already
/// reads: a PNG carries its dimensions at a fixed offset. Everything else is
/// left to the configured ceilings, because learning it would mean reading the
/// file, and the point of the probe is to happen *before* that.
pub struct Probe {
    pub format: Option<Format>,
    pub pixels: Option<u64>,
}

/// How many bytes of a file [`probe_path`] reads: enough for the format
/// signatures, including a PDF header up to its allowed 1024-byte prefix, and
/// for a PNG's `IHDR` dimensions.
const PROBE_BYTES: usize = 1024;

/// Reads the first bytes of a file without loading it.
pub fn probe_path(path: &Path) -> Result<Probe> {
    use std::io::Read;

    let mut file = fs::File::open(path)?;
    let mut buf = [0u8; PROBE_BYTES];
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    let head = &buf[..filled];
    let format = detect(head);
    // The eight-byte signature, then the IHDR length and type (8), then width
    // and height (8): the dimensions end at offset 24.
    let pixels = match format {
        Some(Format::Png) if filled >= 24 && head[12..16] == *b"IHDR" => {
            let width = u32::from_be_bytes([head[16], head[17], head[18], head[19]]) as u64;
            let height = u32::from_be_bytes([head[20], head[21], head[22], head[23]]) as u64;
            Some(width.saturating_mul(height))
        }
        _ => None,
    };
    Ok(Probe { format, pixels })
}

/// The format alone, for callers that do not need the dimensions.
pub fn detect_path(path: &Path) -> Result<Option<Format>> {
    Ok(probe_path(path)?.format)
}

/// Normalizes an extension for `--include` / `--exclude` matching.
pub fn extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

/// Expands a path argument into candidate file paths. Directories yield their
/// entries (sorted, deterministic), files yield themselves.
pub fn expand(
    path: &Path,
    recursive: bool,
    follow_symlinks: bool,
    out: &mut Vec<PathBuf>,
    problems: &mut Vec<(PathBuf, Error, ReasonCode)>,
    depth: usize,
) {
    // A depth guard, and it is *the* guard: a symlink cycle under
    // `--follow-symlinks` is caught here rather than by a visited set of
    // `(device, inode)` pairs, because the walk holds no such set. The limit is
    // 64 levels; deeper subtrees are reported as refused.
    if depth > 64 {
        problems.push((
            path.to_path_buf(),
            Error::limit("directory nesting deeper than 64 levels; refused"),
            ReasonCode::DirectoryDepthExceeded,
        ));
        return;
    }

    let md = match fs::symlink_metadata(path) {
        Ok(md) => md,
        Err(e) => {
            problems.push((
                path.to_path_buf(),
                Error::Io(e),
                ReasonCode::FilesystemError,
            ));
            return;
        }
    };

    if md.file_type().is_symlink() {
        if !follow_symlinks {
            problems.push((
                path.to_path_buf(),
                Error::parse("symbolic link skipped (use --follow-symlinks)"),
                ReasonCode::SymbolicLinkNotFollowed,
            ));
            return;
        }
        match fs::metadata(path) {
            Ok(target) => {
                if target.is_dir() {
                    expand_dir(path, recursive, follow_symlinks, out, problems, depth);
                } else if target.is_file() {
                    out.push(path.to_path_buf());
                } else {
                    problems.push((
                        path.to_path_buf(),
                        Error::parse("symbolic link does not point to a regular file"),
                        ReasonCode::InvalidSymbolicLinkTarget,
                    ));
                }
            }
            Err(e) => problems.push((
                path.to_path_buf(),
                Error::Io(e),
                ReasonCode::FilesystemError,
            )),
        }
        return;
    }

    if md.is_dir() {
        expand_dir(path, recursive, follow_symlinks, out, problems, depth);
    } else if md.is_file() {
        out.push(path.to_path_buf());
    } else {
        problems.push((
            path.to_path_buf(),
            Error::parse("not a regular file; skipped"),
            ReasonCode::NotARegularFile,
        ));
    }
}

fn expand_dir(
    path: &Path,
    recursive: bool,
    follow_symlinks: bool,
    out: &mut Vec<PathBuf>,
    problems: &mut Vec<(PathBuf, Error, ReasonCode)>,
    depth: usize,
) {
    if depth > 64 {
        problems.push((
            path.to_path_buf(),
            Error::limit("directory nesting deeper than 64 levels; refused"),
            ReasonCode::DirectoryDepthExceeded,
        ));
        return;
    }

    let entries = match fs::read_dir(path) {
        Ok(rd) => rd,
        Err(e) => {
            problems.push((
                path.to_path_buf(),
                Error::Io(e),
                ReasonCode::FilesystemError,
            ));
            return;
        }
    };

    let mut children: Vec<PathBuf> = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) => children.push(e.path()),
            Err(e) => problems.push((
                path.to_path_buf(),
                Error::Io(e),
                ReasonCode::FilesystemError,
            )),
        }
    }
    // Deterministic order: sort by raw path bytes so runs are reproducible.
    children.sort_by(|a, b| {
        a.as_os_str()
            .as_encoded_bytes()
            .cmp(b.as_os_str().as_encoded_bytes())
    });

    for child in children {
        let md = match fs::symlink_metadata(&child) {
            Ok(md) => md,
            Err(e) => {
                problems.push((child, Error::Io(e), ReasonCode::FilesystemError));
                continue;
            }
        };
        if md.file_type().is_symlink() {
            if !follow_symlinks {
                continue;
            }
            match fs::metadata(&child) {
                Ok(t) if t.is_file() => out.push(child),
                Ok(t) if t.is_dir() && recursive => {
                    expand_dir(&child, recursive, follow_symlinks, out, problems, depth + 1)
                }
                Ok(_) => {}
                Err(e) => problems.push((child, Error::Io(e), ReasonCode::FilesystemError)),
            }
            continue;
        }
        if md.is_dir() {
            if recursive {
                expand_dir(&child, recursive, follow_symlinks, out, problems, depth + 1);
            }
        } else if md.is_file() {
            out.push(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_png_and_gzip() {
        assert_eq!(detect(&PNG_SIGNATURE), Some(Format::Png));
        assert_eq!(detect(&[0x1f, 0x8b, 0x08, 0x00]), Some(Format::Gzip));
        assert_eq!(detect(&[0, 0, 1, 0, 2, 0]), Some(Format::Ico));
        assert_eq!(detect(&[0, 0, 2, 0, 1, 0]), Some(Format::Ico));
        assert_eq!(detect(&[0, 0, 1, 0, 0, 0]), None); // zero images
        assert_eq!(detect(&[0, 0, 3, 0, 1, 0]), None); // unknown type
        assert_eq!(detect(b"PK\x03\x04"), Some(Format::Zip));
        assert_eq!(detect(b"PK\x05\x06"), Some(Format::Zip));
        assert_eq!(detect(b"PK\x07\x08"), None);
        assert_eq!(detect(&[0xff, 0xd8, 0xff, 0xe0]), Some(Format::Jpeg));
        assert_eq!(detect(&[0xff, 0xd8, 0x00]), None); // SOI without a marker
        assert_eq!(detect(b"II*\x00\x08\x00\x00\x00"), Some(Format::Tiff));
        assert_eq!(detect(b"MM\x00*\x00\x00\x00\x08"), Some(Format::Tiff));
        assert_eq!(detect(b"II+\x00\x08\x00\x00\x00"), Some(Format::Tiff));
        assert_eq!(detect(b"MM\x00+\x00\x00\x00\x08"), Some(Format::Tiff));
        assert_eq!(detect(b"II*\x01"), None); // magic 42 in the wrong byte order
        assert_eq!(detect(b"hello, world"), None);
        assert_eq!(detect(b"fLaC"), Some(Format::Flac));
        assert_eq!(detect(b"%PDF-1.7"), Some(Format::Pdf));
        let mut prefixed_pdf = vec![b' '; 1019];
        prefixed_pdf.extend_from_slice(b"%PDF-");
        assert_eq!(detect(&prefixed_pdf), Some(Format::Pdf));
        let mut late_pdf = vec![b' '; 1020];
        late_pdf.extend_from_slice(b"%PDF-");
        assert_eq!(detect(&late_pdf), None);
        assert_eq!(detect(&[]), None);
        // Truncated signatures are not a match.
        assert_eq!(detect(&PNG_SIGNATURE[..7]), None);
        assert_eq!(detect(&[0x1f, 0x8b]), None);
    }

    #[test]
    fn detects_fits_and_rejects_near_misses() {
        assert_eq!(
            detect(b"SIMPLE  =                    T / conforms to FITS standard"),
            Some(Format::Fits)
        );
        assert_eq!(detect(b"SIMPLE  = T"), Some(Format::Fits));
        // The keyword is eight columns wide; a shorter spelling is not a FITS
        // card, and neither is the keyword without its `=`.
        assert_eq!(detect(b"SIMPLE = T"), None);
        assert_eq!(detect(b"SIMPLE  T"), None);
        assert_eq!(detect(b"SIMPLE"), None);
        assert_eq!(detect(b"simPle  = T"), None);
    }

    #[test]
    fn detects_zlib_and_rejects_near_misses() {
        // 0x78 0x01 is the canonical "level 0, 32 KiB window" header, and the
        // pair is a valid multiple of 31.
        assert_eq!(detect(&[0x78, 0x01, 0x00, 0x00]), Some(Format::Zlib));
        assert_eq!(detect(&[0x78, 0x9c, 0x00, 0x00]), Some(Format::Zlib));
        assert_eq!(detect(&[0x78, 0xda, 0x00, 0x00]), Some(Format::Zlib));
        // Same bytes with the check value broken.
        assert_eq!(detect(&[0x78, 0x02, 0x00, 0x00]), None);
        // CM = 7 is not DEFLATE.
        assert_eq!(detect(&[0x77, 0x01]), None);
        // CINFO above 7 is invalid in a zlib header.
        assert_eq!(detect(&[0x88, 0x01]), None);
        // FDICT asks for a dictionary we do not have: not something to touch.
        assert_eq!(detect(&[0x78, 0xbb]), None);
        // A single byte is not a header.
        assert_eq!(detect(&[0x78]), None);
    }

    #[test]
    fn gzip_with_other_compression_method_is_not_detected() {
        assert_eq!(detect(&[0x1f, 0x8b, 0x07]), None);
    }

    /// #51: the probe answers the pixel count for a PNG, because its dimensions
    /// sit in the header it already reads. Everything else stays unknown.
    #[test]
    fn the_probe_reads_png_dimensions_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("tzim-probe-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&300u32.to_be_bytes());
        png.extend_from_slice(&200u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        let path = dir.join("a.png");
        fs::write(&path, &png).unwrap();
        let probe = probe_path(&path).unwrap();
        assert_eq!(probe.format, Some(Format::Png));
        assert_eq!(probe.pixels, Some(60_000));

        // A PNG cut off inside its `IHDR` is still a PNG, but its dimensions are
        // unknown — the estimator falls back to `--max-pixels` rather than
        // guessing from a partial number.
        let short = dir.join("short.png");
        fs::write(&short, &png[..20]).unwrap();
        let probe = probe_path(&short).unwrap();
        assert_eq!(probe.format, Some(Format::Png));
        assert_eq!(probe.pixels, None);

        // A JPEG and a TIFF have no dimensions at a fixed offset.
        let jpeg = dir.join("a.jpg");
        fs::write(
            &jpeg,
            [0xff, 0xd8, 0xff, 0xe0, 0, 0x10, b'J', b'F', b'I', b'F'],
        )
        .unwrap();
        let probe = probe_path(&jpeg).unwrap();
        assert_eq!(probe.format, Some(Format::Jpeg));
        assert_eq!(probe.pixels, None);

        // An empty file has neither.
        let empty = dir.join("empty");
        fs::write(&empty, b"").unwrap();
        let probe = probe_path(&empty).unwrap();
        assert_eq!(probe.format, None);
        assert_eq!(probe.pixels, None);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extensions_are_normalized() {
        assert_eq!(extension(Path::new("a.PNG")), Some("png".into()));
        assert_eq!(extension(Path::new("a.tar.gz")), Some("gz".into()));
        assert_eq!(extension(Path::new("noext")), None);
    }
}
