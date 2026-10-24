use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Formats Tzim can handle. Detection is by content, never by file name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Png,
    Gzip,
    Zip,
    Ico,
    Jpeg,
    Tiff,
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
    if head.len() >= 6 && head[0] == 0 && head[1] == 0 && (head[2] == 1 || head[2] == 2) && head[3] == 0
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

/// Reads the first bytes of a file without loading it.
pub fn detect_path(path: &Path) -> Result<Option<Format>> {
    use std::io::Read;

    let mut file = fs::File::open(path)?;
    let mut buf = [0u8; 16];
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(detect(&buf[..filled]))
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
    problems: &mut Vec<(PathBuf, Error)>,
    depth: usize,
) {
    // A depth guard prevents pathological trees from exhausting the stack even
    // when symlink cycles are excluded by the visited set in `walk`.
    if depth > 64 {
        problems.push((
            path.to_path_buf(),
            Error::limit("directory nesting deeper than 64 levels; refused"),
        ));
        return;
    }

    let md = match fs::symlink_metadata(path) {
        Ok(md) => md,
        Err(e) => {
            problems.push((path.to_path_buf(), Error::Io(e)));
            return;
        }
    };

    if md.file_type().is_symlink() {
        if !follow_symlinks {
            problems.push((
                path.to_path_buf(),
                Error::parse("symbolic link skipped (use --follow-symlinks)"),
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
                    ));
                }
            }
            Err(e) => problems.push((path.to_path_buf(), Error::Io(e))),
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
        ));
    }
}

fn expand_dir(
    path: &Path,
    recursive: bool,
    follow_symlinks: bool,
    out: &mut Vec<PathBuf>,
    problems: &mut Vec<(PathBuf, Error)>,
    depth: usize,
) {
    if depth > 64 {
        problems.push((
            path.to_path_buf(),
            Error::limit("directory nesting deeper than 64 levels; refused"),
        ));
        return;
    }

    let entries = match fs::read_dir(path) {
        Ok(rd) => rd,
        Err(e) => {
            problems.push((path.to_path_buf(), Error::Io(e)));
            return;
        }
    };

    let mut children: Vec<PathBuf> = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) => children.push(e.path()),
            Err(e) => problems.push((path.to_path_buf(), Error::Io(e))),
        }
    }
    // Deterministic order: sort by raw path bytes so runs are reproducible.
    children.sort_by(|a, b| a.as_os_str().as_encoded_bytes().cmp(b.as_os_str().as_encoded_bytes()));

    for child in children {
        let md = match fs::symlink_metadata(&child) {
            Ok(md) => md,
            Err(e) => {
                problems.push((child, Error::Io(e)));
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
                Err(e) => problems.push((child, Error::Io(e))),
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
        assert_eq!(detect(&[]), None);
        // Truncated signatures are not a match.
        assert_eq!(detect(&PNG_SIGNATURE[..7]), None);
        assert_eq!(detect(&[0x1f, 0x8b]), None);
    }

    #[test]
    fn gzip_with_other_compression_method_is_not_detected() {
        assert_eq!(detect(&[0x1f, 0x8b, 0x07]), None);
    }

    #[test]
    fn extensions_are_normalized() {
        assert_eq!(extension(Path::new("a.PNG")), Some("png".into()));
        assert_eq!(extension(Path::new("a.tar.gz")), Some("gz".into()));
        assert_eq!(extension(Path::new("noext")), None);
    }
}
